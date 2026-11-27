//! Integration tests against real SQLite files in temporary directories.

use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use text2create_ir::change::ChangeSet;
use text2create_ir::domain::DomainRegistry;
use text2create_ir::ids::ProjectId;
use text2create_ir::model::RequirementStatus;
use text2create_ir::scalar::{Text, Timestamp};
use text2create_ir::source::{Clock, IdGenerator, Source};
use text2create_store::{
    APPLICATION_ID, DATABASE_FILE, OpenProject, ProjectStore, SCHEMA_VERSION, StoreError,
};

// ------------------------------------------------------------------ helpers

/// A temporary directory removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "text2create-store-test-{}-{}",
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
            "2026-11-28T10:{:02}:{:02}.000Z",
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

    fn root(&self) -> PathBuf {
        self.dir.0.join("projects")
    }

    /// A new store instance, as after an application restart.
    fn store(&self) -> ProjectStore {
        ProjectStore::new(self.root(), Arc::new(DomainRegistry::new()))
    }

    fn create(&mut self, name: &str) -> OpenProject {
        let store = self.store();
        store
            .create(
                &Source::user_edit(),
                text(name),
                None,
                &mut self.clock,
                &mut self.ids,
            )
            .unwrap()
    }

    fn apply(
        &mut self,
        project: &mut OpenProject,
        source: &Source,
        json: &str,
    ) -> Result<(), StoreError> {
        let changes = ChangeSet::from_json(json.as_bytes()).unwrap();
        project
            .apply(source, &changes, &mut self.clock, &mut self.ids)
            .map(|_| ())
    }
}

fn text<const N: usize>(value: &str) -> Text<N> {
    Text::try_from(value.to_owned()).unwrap()
}

fn database(store: &ProjectStore, id: ProjectId) -> PathBuf {
    store.directory(id).join(DATABASE_FILE)
}

/// A second, raw connection to a project database.
fn raw(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

fn stored_document(path: &Path) -> String {
    raw(path)
        .query_row("SELECT document FROM project_ir", [], |r| r.get(0))
        .unwrap()
}

const ADD_REQUIREMENT: &str = r#"{"inputs": [], "operations": [{"op": "add_requirement", "temp": "r",
    "kind": "functional", "priority": "must", "title": "Measure temperature",
    "statement": "Read the temperature every 5 seconds.",
    "origin": {"type": "user", "quote": "Measure the temperature."}, "acceptance": []},
    {"op": "decide_requirement", "requirement": {"temp": "r"}, "decision": "confirm"}]}"#;

const RENAME: &str = r#"{"inputs": [], "operations": [{"op": "set_project_info", "name": "Renamed", "description": "Updated."}]}"#;

fn model() -> Source {
    let digest = |c: char| {
        text2create_ir::scalar::Digest::try_from(format!("sha256:{}", c.to_string().repeat(64)))
            .unwrap()
    };
    Source::model_interpretation(
        text("ollama"),
        text("example-model"),
        text2create_ir::model::ModelInvocation {
            prompt_digest: digest('a'),
            response_digest: digest('b'),
        },
    )
}

// ------------------------------------------------- create, reopen, update, list

#[test]
fn create_save_close_and_reopen_after_a_restart() {
    let mut env = Env::new();
    let mut project = env.create("Fan controller");
    let id = project.id();
    env.apply(&mut project, &Source::user_edit(), ADD_REQUIREMENT)
        .unwrap();
    let saved = project.project().to_canonical_json();
    project.close().unwrap();

    // A new store instance represents a restarted application.
    let store = env.store();
    let reopened = store.open(id).unwrap();
    assert_eq!(reopened.project().to_canonical_json(), saved);
    let requirement = &reopened.project().document().requirements[0];
    assert!(matches!(
        requirement.status,
        RequirementStatus::Confirmed { .. }
    ));
    // The database file holds exactly the canonical document.
    assert_eq!(stored_document(&database(&store, id)), saved);
}

#[test]
fn updates_are_persisted_in_order() {
    let mut env = Env::new();
    let mut project = env.create("Original");
    let id = project.id();
    env.apply(&mut project, &Source::user_edit(), RENAME)
        .unwrap();
    env.apply(&mut project, &Source::user_edit(), ADD_REQUIREMENT)
        .unwrap();
    let expected = project.project().to_canonical_json();
    drop(project);
    let reopened = env.store().open(id).unwrap();
    let document = reopened.project().document();
    assert_eq!(document.project.name.as_str(), "Renamed");
    assert_eq!(document.project.provenance.rev, 2);
    assert_eq!(document.provenance.len(), 3);
    assert_eq!(reopened.project().to_canonical_json(), expected);
}

#[test]
fn list_returns_basic_information_most_recent_first() {
    let mut env = Env::new();
    assert!(
        env.store().list().unwrap().projects.is_empty(),
        "a missing root is an empty list"
    );
    let first = env.create("First");
    let mut second = env.create("Second");
    let third = env.create("Third");
    env.apply(&mut second, &Source::user_edit(), RENAME)
        .unwrap();
    let (first, second, third) = (first.id(), second.id(), third.id());
    // Directories that do not belong to the store are ignored.
    fs::create_dir_all(env.root().join("notes")).unwrap();
    fs::create_dir_all(env.root().join(format!(".creating-{third}"))).unwrap();
    fs::write(env.root().join("file.txt"), "x").unwrap();

    let list = env.store().list().unwrap();
    let names: Vec<_> = list
        .projects
        .iter()
        .map(|p| (p.id, p.name.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![(second, "Renamed"), (third, "Third"), (first, "First")]
    );
    assert!(list.unreadable.is_empty());
    assert!(list.projects[0].updated_at > list.projects[1].updated_at);
}

#[test]
fn listing_reports_damaged_projects_without_failing() {
    let mut env = Env::new();
    let good = env.create("Good").id();
    let bad = env.create("Bad").id();
    let store = env.store();
    fs::write(database(&store, bad), b"not a database").unwrap();
    let list = store.list().unwrap();
    assert_eq!(list.projects.len(), 1);
    assert_eq!(list.projects[0].id, good);
    assert_eq!(list.unreadable.len(), 1);
    assert!(matches!(
        list.unreadable[0].error,
        StoreError::NotAProject(_)
    ));
}

// ------------------------------------------------------- missing and invalid

#[test]
fn opening_a_missing_project_fails() {
    let env = Env::new();
    let id: ProjectId = "prj_01930000-0000-7000-8000-0000000000ff".parse().unwrap();
    assert!(matches!(env.store().open(id), Err(StoreError::NotFound(missing)) if missing == id));
}

/// Opens a project after `damage` was applied to its database with a raw
/// connection, and checks that the failed open did not modify the file.
fn open_damaged(damage: &str) -> StoreError {
    let mut env = Env::new();
    let id = env.create("Damaged").id();
    let store = env.store();
    let path = database(&store, id);
    raw(&path).execute_batch(damage).unwrap();
    let before = fs::read(&path).unwrap();
    let error = store.open(id).err().expect("expected the open to fail");
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a failed open modified the database"
    );
    error
}

#[test]
fn invalid_stored_ir_is_rejected() {
    assert!(matches!(
        open_damaged("UPDATE project_ir SET document = '{\"format\": 1}'"),
        StoreError::InvalidIr(_)
    ));
    // A repeated key is rejected by the strict parser, not silently collapsed.
    assert!(matches!(
        open_damaged(
            "UPDATE project_ir SET document = replace(document, '\"name\": \"Damaged\",', '\"name\": \"Damaged\", \"name\": \"Other\",')"
        ),
        StoreError::InvalidIr(text2create_ir::LoadError::Json(_))
    ));
    // An invariant violation in the stored document.
    assert!(matches!(
        open_damaged(
            "UPDATE project_ir SET document = replace(document, '\"rev\": 1', '\"rev\": 0')"
        ),
        StoreError::InvalidIr(text2create_ir::LoadError::Invariant(_))
    ));
}

#[test]
fn unsupported_ir_versions_are_rejected_without_modification() {
    assert!(matches!(
        open_damaged(
            "UPDATE project_ir SET document = replace(document, '\"version\": 1', '\"version\": 2')"
        ),
        StoreError::InvalidIr(text2create_ir::LoadError::NewerFormat { found: 2, .. })
    ));
}

#[test]
fn inconsistent_or_missing_records_are_reported_as_damage() {
    assert!(matches!(
        open_damaged("UPDATE project_ir SET name = 'Something else'"),
        StoreError::Corrupt(_)
    ));
    assert!(matches!(
        open_damaged("DELETE FROM project_ir"),
        StoreError::Corrupt(_)
    ));
}

#[test]
fn newer_or_foreign_databases_are_refused_without_modification() {
    assert!(matches!(
        open_damaged("PRAGMA user_version = 99"),
        StoreError::NewerSchema {
            found: 99,
            supported: SCHEMA_VERSION
        }
    ));
    assert!(matches!(
        open_damaged("PRAGMA application_id = 1"),
        StoreError::NotAProject(_)
    ));
}

#[test]
fn other_sqlite_databases_are_refused_without_modification() {
    let mut env = Env::new();
    let id = env.create("Replaced").id();
    let store = env.store();
    let path = database(&store, id);
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", path.display()));
    }
    // A database of another application, in rollback-journal mode.
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE notes (text TEXT); INSERT INTO notes VALUES ('keep'); PRAGMA user_version = 1;")
        .unwrap();
    let before = fs::read(&path).unwrap();
    assert!(matches!(store.open(id), Err(StoreError::NotAProject(_))));
    assert_eq!(fs::read(&path).unwrap(), before);
    let mode: String = raw(&path)
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "delete", "the refused database was switched to WAL");
}

#[test]
fn files_that_are_not_databases_are_refused() {
    let mut env = Env::new();
    let id = env.create("Replaced").id();
    let store = env.store();
    let path = database(&store, id);
    fs::write(&path, b"plain text, not SQLite").unwrap();
    assert!(matches!(store.open(id), Err(StoreError::NotAProject(_))));
    assert_eq!(fs::read(&path).unwrap(), b"plain text, not SQLite");
}

// ------------------------------------------------------------- failed writes

#[test]
fn a_rejected_change_set_changes_neither_the_project_nor_the_database() {
    let mut env = Env::new();
    let mut project = env.create("Protected");
    env.apply(
        &mut project,
        &model(),
        r#"{"inputs": [], "operations": [{"op": "add_requirement",
        "kind": "functional", "priority": "must", "title": "T", "statement": "S.",
        "origin": {"type": "inferred", "rationale": "R."}, "acceptance": []}]}"#,
    )
    .unwrap();
    let id = project.id();
    let path = database(&env.store(), id);
    let before = (
        project.project().to_canonical_json(),
        stored_document(&path),
    );
    let requirement = project.project().document().requirements[0].id;
    // A model cannot confirm a requirement.
    let error = env.apply(&mut project, &model(), &format!(
        r#"{{"inputs": [], "operations": [{{"op": "decide_requirement", "requirement": {{"id": "{requirement}"}}, "decision": "confirm"}}]}}"#
    )).unwrap_err();
    assert!(matches!(error, StoreError::Write(_)));
    assert_eq!(
        (
            project.project().to_canonical_json(),
            stored_document(&path)
        ),
        before
    );
}

#[test]
fn a_failing_database_write_keeps_the_previous_state() {
    let mut env = Env::new();
    let mut project = env.create("Injected");
    let id = project.id();
    let path = database(&env.store(), id);
    let before = project.project().to_canonical_json();
    // Failure injection: every update of the stored record fails.
    raw(&path)
        .execute_batch("CREATE TRIGGER fail BEFORE UPDATE ON project_ir BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")
        .unwrap();
    let error = env
        .apply(&mut project, &Source::user_edit(), RENAME)
        .unwrap_err();
    assert!(error.to_string().contains("injected failure"), "{error}");
    // Neither the open project nor the database changed.
    assert_eq!(project.project().to_canonical_json(), before);
    assert_eq!(stored_document(&path), before);
    // After the fault is removed, the same project can be saved again.
    raw(&path).execute_batch("DROP TRIGGER fail;").unwrap();
    env.apply(&mut project, &Source::user_edit(), RENAME)
        .unwrap();
    drop(project);
    let reopened = env.store().open(id).unwrap();
    assert_eq!(
        reopened.project().document().project.name.as_str(),
        "Renamed"
    );
}

#[test]
fn an_interrupted_write_is_not_visible_after_reopening() {
    let mut env = Env::new();
    let project = env.create("Interrupted");
    let id = project.id();
    let before = project.project().to_canonical_json();
    project.close().unwrap();
    let path = database(&env.store(), id);
    // A writer that stops before committing, as in a crash.
    {
        let mut connection = raw(&path);
        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "UPDATE project_ir SET document = 'garbage', revision = revision + 1",
                [],
            )
            .unwrap();
        std::mem::forget(transaction);
        // The connection is closed without a commit.
    }
    let reopened = env.store().open(id).unwrap();
    assert_eq!(reopened.project().to_canonical_json(), before);
}

#[test]
fn concurrent_writers_cannot_overwrite_each_other() {
    let mut env = Env::new();
    let mut first = env.create("Shared");
    let id = first.id();
    let mut second = env.store().open(id).unwrap();
    env.apply(&mut first, &Source::user_edit(), RENAME).unwrap();
    let stale = second.project().to_canonical_json();
    let error = env
        .apply(&mut second, &Source::user_edit(), ADD_REQUIREMENT)
        .unwrap_err();
    assert!(matches!(error, StoreError::Conflict));
    assert_eq!(second.project().to_canonical_json(), stale);
    // The first writer's change is kept.
    assert_eq!(
        stored_document(&database(&env.store(), id)),
        first.project().to_canonical_json()
    );
}

#[test]
fn a_failed_create_leaves_no_project_behind() {
    let mut env = Env::new();
    // The root path is a file, so no directory can be created in it.
    fs::write(env.root(), b"in the way").unwrap();
    let store = env.store();
    let result = store.create(
        &Source::user_edit(),
        text("X"),
        None,
        &mut env.clock,
        &mut env.ids,
    );
    assert!(matches!(result, Err(StoreError::Io(_))));
    assert_eq!(fs::read(env.root()).unwrap(), b"in the way");
}

#[test]
fn create_never_overwrites_an_existing_project() {
    let mut env = Env::new();
    let original = env.create("Original");
    let id = original.id();
    let path = database(&env.store(), id);
    let before = fs::read(&path).unwrap();
    drop(original);
    // The same identifier generator state produces the same project ID again.
    env.ids = TestIds(0);
    let store = env.store();
    let result = store.create(
        &Source::user_edit(),
        text("Copy"),
        None,
        &mut env.clock,
        &mut env.ids,
    );
    assert!(matches!(result, Err(StoreError::AlreadyExists(existing)) if existing == id));
    assert_eq!(fs::read(&path).unwrap(), before);
}

// ---------------------------------------------------------------- schema

#[test]
fn new_databases_record_the_application_and_schema_version() {
    let mut env = Env::new();
    let id = env.create("Versioned").id();
    let connection = raw(&database(&env.store(), id));
    let application_id: i32 = connection
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let mode: String = connection
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        (application_id, version, mode.as_str()),
        (APPLICATION_ID, SCHEMA_VERSION, "wal")
    );
}
