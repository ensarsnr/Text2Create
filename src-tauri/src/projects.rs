//! Project commands: the frontend's interface to project persistence.
//!
//! The logic lives in [`ProjectService`], which has no Tauri dependency and is
//! tested directly. The commands are thin wrappers around it. All input from
//! the frontend is validated here, and the source of every change is assigned
//! in Rust: changes requested by the frontend are user edits.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use text2create_ir::change::ChangeSet;
use text2create_ir::domain::DomainRegistry;
use text2create_ir::ids::ProjectId;
use text2create_ir::model::ProjectDocument;
use text2create_ir::scalar::Text;
use text2create_ir::source::{Source, SystemClock, UuidV7Generator};
use text2create_store::{ArtifactStatus, OpenProject, ProjectStore, StoreError};

/// Directory under the application data directory that holds the projects.
pub const PROJECTS_DIRECTORY: &str = "projects";

/// The open project as returned to the frontend.
#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: String,
    pub document: ProjectDocument,
    /// The state of every artifact file, checked when the project is opened.
    /// `None` for results of other operations, which do not check files.
    pub artifacts: Option<Vec<ArtifactCheckView>>,
}

/// The state of an artifact file compared with its recorded content. A
/// difference is only reported; nothing is recorded or repaired.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ArtifactCheckView {
    pub artifact: String,
    pub path: String,
    /// `unchanged`, `not_produced`, `missing`, `changed` (outside
    /// Text2Create), `unrecorded` (a file exists without recorded content), or
    /// `unreadable`.
    pub status: &'static str,
    /// The digest of the file as found, for `changed` and `unrecorded`.
    pub digest: Option<String>,
    /// Why the file could not be checked, for `unreadable`.
    pub error: Option<String>,
}

fn artifact_checks(project: &OpenProject) -> Vec<ArtifactCheckView> {
    project
        .check_artifacts()
        .into_iter()
        .map(|check| {
            let (status, digest, error) = match check.status {
                ArtifactStatus::Unchanged => ("unchanged", None, None),
                ArtifactStatus::NotProduced => ("not_produced", None, None),
                ArtifactStatus::Missing => ("missing", None, None),
                ArtifactStatus::Changed { digest, .. } => ("changed", Some(digest), None),
                ArtifactStatus::Unrecorded { digest, .. } => ("unrecorded", Some(digest), None),
                ArtifactStatus::Unreadable(error) => ("unreadable", None, Some(error.to_string())),
            };
            ArtifactCheckView {
                artifact: check.artifact.to_string(),
                path: check.path.as_str().to_owned(),
                status,
                digest: digest.map(|d| d.as_str().to_owned()),
                error,
            }
        })
        .collect()
}

/// An entry of the project list.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ProjectListEntry {
    pub id: String,
    pub name: String,
    pub updated_at: String,
}

/// The project list returned to the frontend.
#[derive(Debug, Serialize)]
pub struct ProjectListView {
    pub projects: Vec<ProjectListEntry>,
    /// Project directories that could not be read, with the reason.
    pub unreadable: Vec<String>,
}

/// Creates, lists, opens, changes, and closes projects. At most one project
/// is open at a time.
pub struct ProjectService {
    store: ProjectStore,
    current: Option<OpenProject>,
    clock: SystemClock,
    ids: UuidV7Generator,
}

fn view(project: &OpenProject) -> ProjectView {
    ProjectView {
        id: project.id().to_string(),
        document: project.project().document().clone(),
        artifacts: None,
    }
}

impl ProjectService {
    pub fn new(root: PathBuf) -> Self {
        Self::with_registry(root, DomainRegistry::new())
    }

    fn with_registry(root: PathBuf, registry: DomainRegistry) -> Self {
        Self {
            store: ProjectStore::new(root, Arc::new(registry)),
            current: None,
            clock: SystemClock,
            ids: UuidV7Generator,
        }
    }

    /// Creates a project and opens it. The previously open project is closed.
    pub fn create(&mut self, name: &str, description: Option<&str>) -> Result<ProjectView, String> {
        let name = Text::try_from(name.to_owned()).map_err(|e| format!("Project name: {e}"))?;
        let description = description
            .map(|d| Text::try_from(d.to_owned()))
            .transpose()
            .map_err(|e| format!("Project description: {e}"))?;
        self.close()?;
        let project = self
            .store
            .create(
                &Source::user_edit(),
                name,
                description,
                &mut self.clock,
                &mut self.ids,
            )
            .map_err(|e| e.to_string())?;
        let result = view(&project);
        self.current = Some(project);
        Ok(result)
    }

    pub fn list(&self) -> Result<ProjectListView, String> {
        let list = self.store.list().map_err(|e| e.to_string())?;
        Ok(ProjectListView {
            projects: list
                .projects
                .into_iter()
                .map(|p| ProjectListEntry {
                    id: p.id.to_string(),
                    name: p.name.as_str().to_owned(),
                    updated_at: p.updated_at.as_str().to_owned(),
                })
                .collect(),
            unreadable: list
                .unreadable
                .into_iter()
                .map(|u| {
                    let directory = u.directory.file_name().map_or_else(
                        || u.directory.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    );
                    format!("{directory}: {}", u.error)
                })
                .collect(),
        })
    }

    /// Opens a project. The previously open project is closed first. Every
    /// artifact file is compared with its recorded content, so that changes
    /// made outside Text2Create are reported (#13).
    ///
    /// A project written with an older format or domain version is not
    /// opened and not changed. Migrating it changes the stored project
    /// permanently, so it requires the user's explicit approval (ADR-0011).
    /// The approval flow does not exist yet (#26), so the application never
    /// migrates; the error describes the migration that would be needed.
    pub fn open(&mut self, id: &str) -> Result<ProjectView, String> {
        let id: ProjectId = id.parse().map_err(|e| format!("Project: {e}"))?;
        self.close()?;
        let project = match self.store.open(id) {
            Ok(project) => project,
            Err(StoreError::NeedsMigration(_)) => return Err(self.migration_required(id)),
            Err(error) => return Err(error.to_string()),
        };
        let mut result = view(&project);
        result.artifacts = Some(artifact_checks(&project));
        self.current = Some(project);
        Ok(result)
    }

    /// The message for a project that needs a migration that cannot be
    /// approved yet. Planning reads the project without changing it.
    fn migration_required(&self, id: ProjectId) -> String {
        let steps = match self.store.plan_migration(id) {
            Ok(plan) => plan
                .steps
                .iter()
                .map(|step| match &step.domain {
                    None => format!("format {} to {}", step.from, step.to),
                    Some(domain) => {
                        format!("domain `{}` {} to {}", domain.as_str(), step.from, step.to)
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            Err(error) => return error.to_string(),
        };
        format!(
            "This project was saved by an older version of Text2Create and must be migrated \
             ({steps}) before it can be opened. A migration needs your approval, which this \
             version cannot ask for yet. The project was not changed."
        )
    }

    /// Applies a change set to the open project as a user edit and saves it.
    ///
    /// The change set is passed as a JSON string and parsed with the strict
    /// parser, because Tauri would otherwise parse it into a JSON value that
    /// silently drops repeated keys.
    pub fn apply_user_changes(&mut self, changes: &str) -> Result<ProjectView, String> {
        let changes = ChangeSet::from_json(changes.as_bytes()).map_err(|e| e.to_string())?;
        let project = self.current.as_mut().ok_or("No project is open.")?;
        project
            .apply(
                &Source::user_edit(),
                &changes,
                &mut self.clock,
                &mut self.ids,
            )
            .map_err(|e| e.to_string())?;
        Ok(view(project))
    }

    /// Closes the open project, if any.
    pub fn close(&mut self) -> Result<(), String> {
        match self.current.take() {
            Some(project) => project.close().map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }
}

/// The managed state of the project commands.
pub type ProjectState = Mutex<ProjectService>;

fn lock(state: &ProjectState) -> Result<std::sync::MutexGuard<'_, ProjectService>, String> {
    state
        .lock()
        .map_err(|_| "The project service is unavailable.".to_owned())
}

#[tauri::command]
pub fn create_project(
    state: tauri::State<'_, ProjectState>,
    name: String,
    description: Option<String>,
) -> Result<ProjectView, String> {
    lock(&state)?.create(&name, description.as_deref())
}

#[tauri::command]
pub fn list_projects(state: tauri::State<'_, ProjectState>) -> Result<ProjectListView, String> {
    lock(&state)?.list()
}

#[tauri::command]
pub fn open_project(
    state: tauri::State<'_, ProjectState>,
    id: String,
) -> Result<ProjectView, String> {
    lock(&state)?.open(&id)
}

#[tauri::command]
pub fn apply_project_changes(
    state: tauri::State<'_, ProjectState>,
    changes: String,
) -> Result<ProjectView, String> {
    lock(&state)?.apply_user_changes(&changes)
}

#[tauri::command]
pub fn close_project(state: tauri::State<'_, ProjectState>) -> Result<(), String> {
    lock(&state)?.close()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "text2create-app-test-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const RENAME: &str = r#"{"inputs": [], "operations": [{"op": "set_project_info", "name": "Renamed", "description": null}]}"#;

    #[test]
    fn create_change_close_and_reopen_after_restart() {
        let dir = TempDir::new("roundtrip");
        let mut service = ProjectService::new(dir.0.clone());
        let created = service.create("Fan controller", Some("Example.")).unwrap();
        let changed = service.apply_user_changes(RENAME).unwrap();
        assert_eq!(changed.document.project.name.as_str(), "Renamed");
        service.close().unwrap();
        drop(service);

        let mut restarted = ProjectService::new(dir.0.clone());
        let list = restarted.list().unwrap();
        assert_eq!(list.projects.len(), 1);
        assert_eq!(
            (list.projects[0].id.as_str(), list.projects[0].name.as_str()),
            (created.id.as_str(), "Renamed")
        );
        let reopened = restarted.open(&created.id).unwrap();
        assert_eq!(reopened.document, changed.document);
    }

    #[test]
    fn frontend_input_is_validated() {
        let dir = TempDir::new("validation");
        let mut service = ProjectService::new(dir.0.clone());
        assert!(service.create(" padded", None).is_err());
        assert!(service.create(&"x".repeat(121), None).is_err());
        assert!(service.open("../outside").is_err());
        assert!(
            service
                .open("prj_01930000-0000-7000-8000-0000000000ff")
                .unwrap_err()
                .contains("does not exist")
        );
        assert_eq!(
            service.apply_user_changes(RENAME).unwrap_err(),
            "No project is open."
        );
        service.create("Valid", None).unwrap();
        // Repeated keys are rejected, not collapsed.
        let repeated = RENAME.replace(
            r#""name": "Renamed","#,
            r#""name": "Renamed", "name": "Other","#,
        );
        assert!(
            service
                .apply_user_changes(&repeated)
                .unwrap_err()
                .contains("repeated key")
        );
        // The frontend cannot choose the actor.
        let with_actor = RENAME.replace(
            r#"{"inputs": [],"#,
            r#"{"actor": {"type": "system", "component": "x"}, "inputs": [],"#,
        );
        assert!(
            service
                .apply_user_changes(&with_actor)
                .unwrap_err()
                .contains("unknown field")
        );
        // Nothing was written to the database by the rejected calls.
        assert_eq!(service.list().unwrap().projects[0].name, "Valid");
    }

    /// A domain whose version 2 renamed the payload field `pin` to `gpio`.
    struct Sensor(u32);

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct V1 {
        pin: u32,
    }
    impl text2create_ir::domain::EntityPayload for V1 {}

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct V2 {
        gpio: u32,
    }
    impl text2create_ir::domain::EntityPayload for V2 {}

    impl text2create_ir::domain::DomainCodec for Sensor {
        fn id(&self) -> &'static str {
            "sensor"
        }
        fn version(&self) -> u32 {
            self.0
        }
        fn entity_kinds(&self) -> &'static [&'static str] {
            &["probe"]
        }
        fn normalize_entity(
            &self,
            _kind: &str,
            data: &serde_json::Value,
        ) -> Result<text2create_ir::domain::NormalizedPayload, text2create_ir::domain::DomainError>
        {
            use text2create_ir::domain::normalize_typed;
            if self.0 == 1 {
                normalize_typed::<V1>(data)
            } else {
                normalize_typed::<V2>(data)
            }
        }
        fn migrate_entity(
            &self,
            _kind: &str,
            _from: u32,
            data: serde_json::Value,
        ) -> Result<serde_json::Value, text2create_ir::domain::DomainError> {
            Ok(serde_json::json!({"gpio": data["pin"]}))
        }
    }

    fn sensor(version: u32) -> DomainRegistry {
        DomainRegistry::new().register(Sensor(version)).unwrap()
    }

    #[test]
    fn an_older_project_is_never_migrated_without_approval() {
        let dir = TempDir::new("migration");
        let mut old = ProjectService::with_registry(dir.0.clone(), sensor(1));
        let created = old.create("Probe", None).unwrap();
        old.apply_user_changes(
            r#"{"inputs": [], "operations": [{"op": "declare_domain", "domain": "sensor"},
            {"op": "add_entity", "kind": "sensor.probe", "name": "Probe", "data": {"pin": 4}}]}"#,
        )
        .unwrap();
        old.close().unwrap();
        let project = dir.0.join(&created.id);
        let database = project.join(text2create_store::DATABASE_FILE);
        let before = fs::read(&database).unwrap();

        let mut new = ProjectService::with_registry(dir.0.clone(), sensor(2));
        let error = new.open(&created.id).unwrap_err();
        assert!(error.contains("domain `sensor` 1 to 2"), "{error}");
        assert!(error.contains("approval"), "{error}");
        assert!(error.contains("not changed"), "{error}");
        // Opening again gives the same answer; nothing was migrated or backed up.
        assert_eq!(new.open(&created.id).unwrap_err(), error);
        assert_eq!(fs::read(&database).unwrap(), before);
        assert!(!project.join(text2create_store::BACKUPS_DIRECTORY).exists());
        assert!(new.apply_user_changes("{}").is_err(), "no project is open");
        // The version that saved it still opens it.
        old.open(&created.id).unwrap();
    }

    #[test]
    fn changes_requested_by_the_frontend_are_user_edits() {
        let dir = TempDir::new("source");
        let mut service = ProjectService::new(dir.0.clone());
        service.create("Attributed", None).unwrap();
        let view = service.apply_user_changes(RENAME).unwrap();
        assert_eq!(view.document.provenance.len(), 2);
        assert!(
            view.document
                .provenance
                .iter()
                .all(|p| p.actor == text2create_ir::model::Actor::User {})
        );
    }

    #[test]
    fn opening_a_project_flags_artifacts_changed_outside_text2create() {
        use text2create_store::{ArtifactWrite, FILES_DIRECTORY, WriteMode};
        let dir = TempDir::new("artifacts");
        let mut service = ProjectService::new(dir.0.clone());
        let created = service.create("Artifacts", None).unwrap();
        let view = service
            .apply_user_changes(
                r#"{"inputs": [], "operations": [
                {"op": "add_artifact", "kind": "core.document", "path": "a.md", "ownership": "generated"},
                {"op": "add_artifact", "kind": "core.document", "path": "b.md", "ownership": "generated"}]}"#,
            )
            .unwrap();
        assert_eq!(view.artifacts, None);
        let ids: Vec<_> = view.document.artifacts.iter().map(|a| a.id).collect();
        for &artifact in &ids {
            let (clock, generator) = (&mut service.clock, &mut service.ids);
            service
                .current
                .as_mut()
                .unwrap()
                .write_artifact(
                    &Source::user_edit(),
                    ArtifactWrite {
                        artifact,
                        bytes: b"generated",
                        mode: WriteMode::Create,
                        inputs: Vec::new(),
                    },
                    clock,
                    generator,
                )
                .unwrap();
        }
        let recorded = service
            .current
            .as_ref()
            .unwrap()
            .project()
            .document()
            .clone();
        service.close().unwrap();
        let files = dir.0.join(&created.id).join(FILES_DIRECTORY);
        fs::write(files.join("b.md"), b"edited").unwrap();

        let opened = service.open(&created.id).unwrap();
        let statuses: Vec<_> = opened
            .artifacts
            .unwrap()
            .into_iter()
            .map(|c| (c.path, c.status, c.digest.is_some()))
            .collect();
        assert_eq!(
            statuses,
            [
                ("a.md".to_owned(), "unchanged", false),
                ("b.md".to_owned(), "changed", true)
            ]
        );
        // The difference is reported, not recorded.
        assert_eq!(opened.document, recorded);
    }
}
