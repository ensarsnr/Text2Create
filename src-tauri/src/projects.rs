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
use text2create_store::{OpenProject, ProjectStore};

/// Directory under the application data directory that holds the projects.
pub const PROJECTS_DIRECTORY: &str = "projects";

/// The open project as returned to the frontend.
#[derive(Debug, Serialize)]
pub struct ProjectView {
    pub id: String,
    pub document: ProjectDocument,
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
    }
}

impl ProjectService {
    pub fn new(root: PathBuf) -> Self {
        Self {
            store: ProjectStore::new(root, Arc::new(DomainRegistry::new())),
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

    /// Opens a project. The previously open project is closed first.
    pub fn open(&mut self, id: &str) -> Result<ProjectView, String> {
        let id: ProjectId = id.parse().map_err(|e| format!("Project: {e}"))?;
        self.close()?;
        let project = self.store.open(id).map_err(|e| e.to_string())?;
        let result = view(&project);
        self.current = Some(project);
        Ok(result)
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
}
