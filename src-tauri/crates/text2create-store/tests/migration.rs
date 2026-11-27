//! Project IR migrations of stored projects: backup, migration, and save.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use text2create_ir::change::ChangeSet;
use text2create_ir::domain::{
    DomainCodec, DomainError, DomainRegistry, EntityPayload, NormalizedPayload, normalize_typed,
};
use text2create_ir::ids::ProjectId;
use text2create_ir::model::Activity;
use text2create_ir::scalar::Timestamp;
use text2create_ir::source::{Clock, IdGenerator, Source};
use text2create_ir::{LoadError, MigrateError};
use text2create_store::{
    BACKUPS_DIRECTORY, DATABASE_FILE, MigrationPlan, MigrationReport, ProjectStore, StoreError,
};

/// Plans a migration and runs it as if the user approved the plan.
trait Approve {
    fn migrate_approved(
        &self,
        id: ProjectId,
        clock: &mut dyn Clock,
        ids: &mut dyn IdGenerator,
    ) -> Result<MigrationReport, StoreError>;
}

impl Approve for ProjectStore {
    fn migrate_approved(
        &self,
        id: ProjectId,
        clock: &mut dyn Clock,
        ids: &mut dyn IdGenerator,
    ) -> Result<MigrationReport, StoreError> {
        let plan = self.plan_migration(id)?;
        self.migrate(&plan, clock, ids)
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "text2create-migration-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestClock(u32);

impl Clock for TestClock {
    fn now(&mut self) -> Timestamp {
        self.0 += 1;
        Timestamp::try_from(format!(
            "2026-11-28T11:{:02}:{:02}.000Z",
            self.0 / 60,
            self.0 % 60
        ))
        .unwrap()
    }
}

struct TestIds(u128);

impl IdGenerator for TestIds {
    fn next_uuid(&mut self) -> uuid::Uuid {
        self.0 += 1;
        uuid::Uuid::from_u128(0x0193_0000_0000_7000_8000_0000_0000_0000 + self.0)
    }
}

/// `sensor.probe` in version 1.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeV1 {
    pin: u32,
}
impl EntityPayload for ProbeV1 {}

/// `sensor.probe` in version 2: `pin` was renamed to `gpio`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeV2 {
    gpio: u32,
}
impl EntityPayload for ProbeV2 {}

#[derive(Clone, Copy, PartialEq)]
enum Sensor {
    V1,
    V2,
    /// Version 2 with a migration that fails.
    V2Failing,
}

impl DomainCodec for Sensor {
    fn id(&self) -> &'static str {
        "sensor"
    }
    fn version(&self) -> u32 {
        if *self == Sensor::V1 { 1 } else { 2 }
    }
    fn entity_kinds(&self) -> &'static [&'static str] {
        &["probe"]
    }
    fn normalize_entity(&self, kind: &str, data: &Value) -> Result<NormalizedPayload, DomainError> {
        match (kind, self) {
            ("probe", Sensor::V1) => normalize_typed::<ProbeV1>(data),
            ("probe", _) => normalize_typed::<ProbeV2>(data),
            (other, _) => Err(DomainError::UnknownKind(other.to_owned())),
        }
    }
    fn migrate_entity(
        &self,
        _kind: &str,
        from: u32,
        mut data: Value,
    ) -> Result<Value, DomainError> {
        if *self != Sensor::V2 || from != 1 {
            return Err(DomainError::invalid("migration failed on purpose"));
        }
        let pin = data.as_object_mut().and_then(|o| o.remove("pin"));
        let pin = pin.ok_or(DomainError::invalid("missing pin"))?;
        data["gpio"] = pin;
        Ok(data)
    }
}

struct Env {
    dir: TempDir,
    clock: TestClock,
    ids: TestIds,
}

impl Env {
    fn new() -> Self {
        Self {
            dir: TempDir::new(),
            clock: TestClock(0),
            ids: TestIds(0),
        }
    }

    /// A store of an application build with `sensor` at `version`.
    fn store(&self, version: Sensor) -> ProjectStore {
        let registry = DomainRegistry::new().register(version).unwrap();
        ProjectStore::new(self.dir.0.join("projects"), Arc::new(registry))
    }

    /// A project with one `sensor.probe` entity, saved by a version 1 build.
    fn v1_project(&mut self) -> ProjectId {
        let store = self.store(Sensor::V1);
        let mut project = store
            .create(
                &Source::user_edit(),
                "Probe".to_owned().try_into().unwrap(),
                None,
                &mut self.clock,
                &mut self.ids,
            )
            .unwrap();
        let changes = ChangeSet::from_json(
            br#"{"inputs": [], "operations": [{"op": "declare_domain", "domain": "sensor"},
            {"op": "add_entity", "kind": "sensor.probe", "name": "Probe", "data": {"pin": 4}}]}"#,
        )
        .unwrap();
        project
            .apply(
                &Source::user_edit(),
                &changes,
                &mut self.clock,
                &mut self.ids,
            )
            .unwrap();
        project.id()
    }

    fn database(&self, id: ProjectId) -> PathBuf {
        self.store(Sensor::V1).directory(id).join(DATABASE_FILE)
    }

    fn backups(&self, id: ProjectId) -> Vec<PathBuf> {
        let dir = self.store(Sensor::V1).directory(id).join(BACKUPS_DIRECTORY);
        let mut files: Vec<_> = match fs::read_dir(dir) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => Vec::new(),
        };
        files.sort();
        files
    }
}

fn stored(path: &Path) -> (String, i64) {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT document, revision FROM project_ir", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
}

#[test]
fn an_older_project_is_backed_up_migrated_and_opened() {
    let mut env = Env::new();
    let id = env.v1_project();
    let (before, revision) = stored(&env.database(id));

    // A build with version 2 refuses to open it, but still lists it.
    let store = env.store(Sensor::V2);
    let error = store.open(id).err().unwrap();
    assert!(
        matches!(&error, StoreError::NeedsMigration(e) if e.needs_migration()),
        "{error}"
    );
    assert_eq!(store.list().unwrap().projects.len(), 1);

    let report = store
        .migrate_approved(id, &mut env.clock, &mut env.ids)
        .unwrap();
    assert_eq!(report.steps.len(), 1);
    assert!(
        report
            .backup
            .starts_with(store.directory(id).join(BACKUPS_DIRECTORY))
    );
    assert_eq!(env.backups(id), vec![report.backup.clone()]);
    // The backup is a complete, standalone copy of the previous state.
    assert_eq!(stored(&report.backup), (before, revision));

    // After a restart, the project opens with the migrated payload.
    drop(store);
    let reopened = env.store(Sensor::V2).open(id).unwrap();
    let document = reopened.project().document();
    assert_eq!(document.entities[0].data.as_value(), &json!({"gpio": 4}));
    assert_eq!(document.domains[0].version, 2);
    let last = document
        .provenance
        .iter()
        .max_by_key(|p| p.at.clone())
        .unwrap();
    assert_eq!(last.activity, Activity::Migration { from: 1, to: 2 });
    assert_eq!(stored(&env.database(id)).1, revision + 1);
    let listed = env.store(Sensor::V2).list().unwrap().projects;
    assert_eq!(listed[0].updated_at, last.at);
}

#[test]
fn the_backup_includes_data_that_is_still_in_the_wal_file() {
    let mut env = Env::new();
    let store = env.store(Sensor::V1);
    let id = env.v1_project();
    // Keep a connection open, so that the last save is not checkpointed.
    let mut open = store.open(id).unwrap();
    let rename = ChangeSet::from_json(
        br#"{"inputs": [], "operations": [{"op": "set_project_info", "name": "Latest", "description": null}]}"#,
    )
    .unwrap();
    open.apply(&Source::user_edit(), &rename, &mut env.clock, &mut env.ids)
        .unwrap();
    let latest = open.project().to_canonical_json();
    // Copying only the main database file would miss the last save.
    let naive = env.dir.0.join("naive.sqlite");
    fs::copy(env.database(id), &naive).unwrap();
    let naive_document = Connection::open(&naive)
        .unwrap()
        .query_row("SELECT document FROM project_ir", [], |r| {
            r.get::<_, String>(0)
        })
        .ok();
    assert_ne!(naive_document.as_deref(), Some(latest.as_str()));

    let report = env
        .store(Sensor::V2)
        .migrate_approved(id, &mut env.clock, &mut env.ids)
        .unwrap();
    assert_eq!(stored(&report.backup).0, latest);
    // The connection that opened the old version can no longer save.
    let error = open
        .apply(&Source::user_edit(), &rename, &mut env.clock, &mut env.ids)
        .unwrap_err();
    assert!(matches!(error, StoreError::Conflict), "{error}");
}

#[test]
fn a_failing_migration_changes_nothing_and_creates_no_backup() {
    let mut env = Env::new();
    let id = env.v1_project();
    let before = stored(&env.database(id));
    let store = env.store(Sensor::V2Failing);
    let error = store
        .migrate_approved(id, &mut env.clock, &mut env.ids)
        .unwrap_err();
    assert!(
        matches!(error, StoreError::Migration(MigrateError::Domain { .. })),
        "{error}"
    );
    assert_eq!(stored(&env.database(id)), before);
    assert!(env.backups(id).is_empty());
    // The previous version still opens it.
    env.store(Sensor::V1).open(id).unwrap();
}

#[test]
fn a_failing_save_keeps_the_project_and_the_backup() {
    let mut env = Env::new();
    let id = env.v1_project();
    let path = env.database(id);
    let before = stored(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TRIGGER fail BEFORE UPDATE ON project_ir BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")
        .unwrap();
    let store = env.store(Sensor::V2);
    let error = store
        .migrate_approved(id, &mut env.clock, &mut env.ids)
        .unwrap_err();
    assert!(error.to_string().contains("injected failure"), "{error}");
    assert_eq!(stored(&path), before);
    let backups = env.backups(id);
    assert_eq!(backups.len(), 1);
    assert_eq!(stored(&backups[0]), before);
    assert!(matches!(store.open(id), Err(StoreError::NeedsMigration(_))));

    // After the fault is removed, the migration succeeds with a second backup.
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TRIGGER fail;")
        .unwrap();
    store
        .migrate_approved(id, &mut env.clock, &mut env.ids)
        .unwrap();
    assert_eq!(env.backups(id).len(), 2);
    store.open(id).unwrap();
}

#[test]
fn newer_and_current_projects_are_not_migrated_or_modified() {
    let mut env = Env::new();
    let id = env.v1_project();
    env.store(Sensor::V2)
        .migrate_approved(id, &mut env.clock, &mut env.ids)
        .unwrap();
    let path = env.database(id);
    let before = fs::read(&path).unwrap();
    let backups = env.backups(id);

    // A version 1 build cannot open or migrate a version 2 project.
    let old = env.store(Sensor::V1);
    assert!(matches!(
        old.open(id),
        Err(StoreError::InvalidIr(LoadError::NewerDomain {
            found: 2,
            supported: 1,
            ..
        }))
    ));
    assert!(matches!(
        old.migrate_approved(id, &mut env.clock, &mut env.ids),
        Err(StoreError::InvalidIr(LoadError::NewerDomain { .. }))
    ));
    // A current project needs no migration.
    assert!(matches!(
        env.store(Sensor::V2)
            .migrate_approved(id, &mut env.clock, &mut env.ids),
        Err(StoreError::Migration(MigrateError::UpToDate))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(env.backups(id), backups);
}

#[test]
fn planning_a_migration_changes_nothing() {
    let mut env = Env::new();
    let id = env.v1_project();
    let path = env.database(id);
    let before = fs::read(&path).unwrap();
    let store = env.store(Sensor::V2);
    let plan = store.plan_migration(id).unwrap();
    let sensor = "sensor".to_owned().try_into().unwrap();
    assert_eq!(
        plan.steps,
        vec![text2create_ir::PendingStep {
            domain: Some(sensor),
            from: 1,
            to: 2
        }]
    );
    assert_eq!(plan.revision, stored(&path).1);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(env.backups(id).is_empty());
    assert!(matches!(store.open(id), Err(StoreError::NeedsMigration(_))));
}

#[test]
fn an_outdated_or_altered_plan_is_refused_without_changes() {
    let mut env = Env::new();
    let id = env.v1_project();
    let path = env.database(id);
    let store = env.store(Sensor::V2);
    let plan = store.plan_migration(id).unwrap();

    // A plan whose steps differ from the migration is not an approval of it.
    let altered = MigrationPlan {
        steps: Vec::new(),
        ..plan.clone()
    };
    let before = stored(&path);
    assert!(matches!(
        store.migrate(&altered, &mut env.clock, &mut env.ids),
        Err(StoreError::PlanOutdated)
    ));

    // The project changed after the plan was made.
    Connection::open(&path)
        .unwrap()
        .execute("UPDATE project_ir SET revision = revision + 1", [])
        .unwrap();
    let changed = stored(&path);
    assert_eq!(changed.0, before.0);
    assert!(matches!(
        store.migrate(&plan, &mut env.clock, &mut env.ids),
        Err(StoreError::PlanOutdated)
    ));
    assert_eq!(stored(&path), changed);
    assert!(env.backups(id).is_empty());

    // A fresh plan works.
    store
        .migrate_approved(id, &mut env.clock, &mut env.ids)
        .unwrap();
}
