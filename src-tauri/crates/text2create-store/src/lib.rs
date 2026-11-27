//! SQLite persistence for Text2Create projects (#12, ADR-0015).
//!
//! Every project is a self-contained directory under a root directory:
//!
//! ```text
//! <root>/
//! └── prj_<uuid>/            one directory per project, named by its ID
//!     ├── project.sqlite     the project database (with -wal/-shm while open)
//!     └── backups/           copies taken before Project IR migrations
//! ```
//!
//! The database stores the Project IR document in canonical JSON. It is
//! written only with the output of [`Project::create`] or [`Project::apply`],
//! and read only through [`Project::load`], so every IR rule applies to stored
//! data. A document written with an older format or domain version is opened
//! only after the user approved a [`MigrationPlan`] and [`ProjectStore::migrate`]
//! has backed it up and migrated it with [`Project::migrate`]. This crate adds no other way to change a project,
//! and it has no Tauri dependency.

#![forbid(unsafe_code)]

mod schema;

pub use schema::{APPLICATION_ID, SCHEMA_VERSION};

use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use text2create_ir::change::ChangeSet;
use text2create_ir::domain::DomainRegistry;
use text2create_ir::ids::ProjectId;
use text2create_ir::scalar::{Text, Timestamp};
use text2create_ir::source::{Clock, IdGenerator, Source};
use text2create_ir::{
    Applied, LoadError, MigrateError, MigrationStep, PendingStep, Project, WriteEnv, WriteError,
};

/// File name of the project database inside a project directory.
pub const DATABASE_FILE: &str = "project.sqlite";

/// Prefix of directories that are still being created. They are never listed.
const CREATING_PREFIX: &str = ".creating-";

/// Directory inside a project directory that holds backups.
pub const BACKUPS_DIRECTORY: &str = "backups";

/// Why a store operation failed. A failed operation never changes stored data.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("project `{0}` does not exist")]
    NotFound(ProjectId),
    #[error("a project with identifier `{0}` already exists")]
    AlreadyExists(ProjectId),
    #[error("`{}` is not a Text2Create project database", .0.display())]
    NotAProject(PathBuf),
    #[error(
        "the project database uses schema version {found}, newer than the supported version {supported}; it was not modified"
    )]
    NewerSchema { found: u32, supported: u32 },
    #[error("the project database is damaged: {0}")]
    Corrupt(String),
    /// The stored document failed the Project IR loading checks, for example
    /// because it comes from a newer version of Text2Create.
    #[error("the stored Project IR was rejected: {0}")]
    InvalidIr(#[source] LoadError),
    /// The project was written with an older format or domain version. It
    /// opens only after the user approved its [`ProjectStore::plan_migration`].
    #[error("the project must be migrated before it can be opened: {0}")]
    NeedsMigration(#[source] LoadError),
    /// The migration failed; the project was not changed.
    #[error("the project could not be migrated: {0}")]
    Migration(#[source] MigrateError),
    /// The backup before a migration failed; the project was not changed.
    #[error("the backup before the migration failed: {0}")]
    Backup(String),
    /// The write path rejected the change set.
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error("the project was saved by another connection since it was opened; reopen it")]
    Conflict,
    /// The project or the available versions changed after the migration
    /// plan was made, so the approved plan no longer describes the migration.
    #[error("the project changed after the migration was approved; review the migration again")]
    PlanOutdated,
    #[error("database error: {0}")]
    Sqlite(#[source] rusqlite::Error),
    #[error("file system error: {0}")]
    Io(#[from] io::Error),
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseCorrupt) => Self::Corrupt(error.to_string()),
            Some(ErrorCode::NotADatabase) => Self::Corrupt(error.to_string()),
            _ => Self::Sqlite(error),
        }
    }
}

/// Basic information about a stored project, read without loading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: Text<120>,
    /// The time of the last applied change set.
    pub updated_at: Timestamp,
}

/// A project directory that could not be listed.
#[derive(Debug)]
pub struct UnreadableProject {
    pub directory: PathBuf,
    pub error: StoreError,
}

/// The projects under a store root.
#[derive(Debug, Default)]
pub struct ProjectList {
    /// Most recently updated first.
    pub projects: Vec<ProjectSummary>,
    pub unreadable: Vec<UnreadableProject>,
}

/// What a migration of a stored project will do, to be shown to the user
/// for approval before [`ProjectStore::migrate`] runs it (ADR-0011).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    pub project: ProjectId,
    /// The stored revision the plan was made for.
    pub revision: i64,
    /// In the order they will be applied.
    pub steps: Vec<PendingStep>,
}

/// The result of [`ProjectStore::migrate`].
#[derive(Debug)]
pub struct MigrationReport {
    /// The verified copy of the project database taken before the migration.
    pub backup: PathBuf,
    pub steps: Vec<MigrationStep>,
}

/// The projects stored under one root directory.
pub struct ProjectStore {
    root: PathBuf,
    registry: Arc<DomainRegistry>,
}

impl ProjectStore {
    /// A store rooted at `root`. The directory is created when the first
    /// project is created.
    pub fn new(root: impl Into<PathBuf>, registry: Arc<DomainRegistry>) -> Self {
        Self {
            root: root.into(),
            registry,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of project `id`.
    pub fn directory(&self, id: ProjectId) -> PathBuf {
        self.root.join(id.to_string())
    }

    /// Creates a project through [`Project::create`] and stores it.
    ///
    /// The project is written to a temporary directory that is renamed into
    /// place only after the database has been committed, so a failure never
    /// leaves a partial project behind.
    pub fn create(
        &self,
        source: &Source,
        name: Text<120>,
        description: Option<Text<2000>>,
        clock: &mut dyn Clock,
        ids: &mut dyn IdGenerator,
    ) -> Result<OpenProject, StoreError> {
        let mut env = WriteEnv {
            registry: &self.registry,
            clock,
            ids,
        };
        let (project, provenance) = Project::create(source, name, description, &mut env)?;
        let id = project.document().project.id;
        let updated_at = provenance_time(&project, provenance);
        let target = self.directory(id);
        if target.exists() {
            return Err(StoreError::AlreadyExists(id));
        }
        fs::create_dir_all(&self.root)?;
        let staging = self.root.join(format!("{CREATING_PREFIX}{id}"));
        fs::create_dir(&staging)?;
        let written = write_new(&staging.join(DATABASE_FILE), &project, &updated_at)
            .and_then(|()| fs::rename(&staging, &target).map_err(StoreError::from));
        if let Err(error) = written {
            remove_staging(&staging);
            return Err(error);
        }
        self.open(id)
    }

    /// Opens a stored project: checks the database, runs pending schema
    /// migrations, and loads the document with [`Project::load`].
    ///
    /// A document from an older format or domain version is not opened; the
    /// error is [`StoreError::NeedsMigration`].
    pub fn open(&self, id: ProjectId) -> Result<OpenProject, StoreError> {
        let (connection, row, directory) = self.connect(id)?;
        let project = Project::load(row.document.as_bytes(), &self.registry).map_err(|error| {
            if error.needs_migration() {
                StoreError::NeedsMigration(error)
            } else {
                StoreError::InvalidIr(error)
            }
        })?;
        check_metadata(&project, &row, id)?;
        Ok(OpenProject {
            connection,
            project,
            revision: row.revision,
            directory,
            registry: Arc::clone(&self.registry),
        })
    }

    /// The migration a project written with an older format or domain
    /// version needs before it can be opened. Changes nothing.
    pub fn plan_migration(&self, id: ProjectId) -> Result<MigrationPlan, StoreError> {
        let (connection, row, _) = self.connect(id)?;
        let steps = pending(&row, &self.registry)?;
        drop(connection);
        Ok(MigrationPlan {
            project: id,
            revision: row.revision,
            steps,
        })
    }

    /// Backs up and migrates a project as described by `plan`, so that it can
    /// be opened.
    ///
    /// A migration changes the stored project permanently, and older builds
    /// can no longer open it. Call this only after the user approved `plan`
    /// (ADR-0011). If the project or the available versions changed since the
    /// plan was made, nothing is changed and the error is
    /// [`StoreError::PlanOutdated`].
    ///
    /// The migration runs in memory first; a failing migration changes
    /// nothing and creates no backup. Then the database is copied with SQLite
    /// `VACUUM INTO`, which includes committed data that is still in the WAL
    /// file, the copy is verified and moved into `backups/`, and only then is
    /// the migrated document saved in one transaction. If the save fails, the
    /// project is unchanged and the backup is kept.
    pub fn migrate(
        &self,
        plan: &MigrationPlan,
        clock: &mut dyn Clock,
        ids: &mut dyn IdGenerator,
    ) -> Result<MigrationReport, StoreError> {
        let id = plan.project;
        let (mut connection, row, directory) = self.connect(id)?;
        if row.revision != plan.revision || pending(&row, &self.registry)? != plan.steps {
            return Err(StoreError::PlanOutdated);
        }
        let mut env = WriteEnv {
            registry: &self.registry,
            clock,
            ids,
        };
        let migrated =
            Project::migrate(row.document.as_bytes(), &mut env).map_err(StoreError::Migration)?;
        check_metadata(&migrated.project, &row, id)?;
        if migrated
            .steps
            .iter()
            .map(MigrationStep::pending)
            .ne(plan.steps.iter().cloned())
        {
            return Err(StoreError::PlanOutdated);
        }
        let last = migrated
            .steps
            .last()
            .expect("a migration has steps")
            .provenance;
        let updated_at = provenance_time(&migrated.project, last);
        let backup = backup(&connection, &directory, &updated_at, &row.document)?;

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE project_ir
             SET name = ?1, updated_at = ?2, revision = revision + 1, document = ?3
             WHERE id = 1 AND revision = ?4",
            params![
                migrated.project.document().project.name.as_str(),
                updated_at.as_str(),
                migrated.project.to_canonical_json(),
                row.revision
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict);
        }
        transaction.commit()?;
        connection
            .close()
            .map_err(|(_, error)| StoreError::from(error))?;
        Ok(MigrationReport {
            backup,
            steps: migrated.steps,
        })
    }

    /// Opens the database of project `id`: checks it, runs pending schema
    /// migrations, and reads the stored record.
    fn connect(&self, id: ProjectId) -> Result<(Connection, Row, PathBuf), StoreError> {
        let directory = self.directory(id);
        let path = directory.join(DATABASE_FILE);
        if !path.is_file() {
            return Err(StoreError::NotFound(id));
        }
        let mut connection = open_existing(&path)?;
        let check: String = connection.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(StoreError::Corrupt(check));
        }
        schema::migrate(&mut connection, &path)?;
        let row = read_row(&connection)?;
        Ok((connection, row, directory))
    }

    /// Lists the stored projects without loading or modifying them.
    pub fn list(&self) -> Result<ProjectList, StoreError> {
        let mut list = ProjectList::default();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(list),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            // Only directories named like a project ID belong to the store.
            let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<ProjectId>().ok())
            else {
                continue;
            };
            if !entry.file_type()?.is_dir() {
                continue;
            }
            match summary(&entry.path().join(DATABASE_FILE), id) {
                Ok(summary) => list.projects.push(summary),
                Err(error) => list.unreadable.push(UnreadableProject {
                    directory: entry.path(),
                    error,
                }),
            }
        }
        list.projects
            .sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok(list)
    }
}

/// An open project. Changes go through [`OpenProject::apply`] only.
pub struct OpenProject {
    connection: Connection,
    project: Project,
    revision: i64,
    directory: PathBuf,
    registry: Arc<DomainRegistry>,
}

impl OpenProject {
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn id(&self) -> ProjectId {
        self.project.document().project.id
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Applies a change set with [`Project::apply`] and saves the result in
    /// one transaction.
    ///
    /// The change set is applied to a copy. The open project and the database
    /// change only if both the write path and the database write succeed.
    pub fn apply(
        &mut self,
        source: &Source,
        changes: &ChangeSet,
        clock: &mut dyn Clock,
        ids: &mut dyn IdGenerator,
    ) -> Result<Applied, StoreError> {
        let mut next = self.project.clone();
        let mut env = WriteEnv {
            registry: &self.registry,
            clock,
            ids,
        };
        let applied = next.apply(source, changes, &mut env)?;
        let updated_at = provenance_time(&next, applied.provenance);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE project_ir
             SET name = ?1, updated_at = ?2, revision = revision + 1, document = ?3
             WHERE id = 1 AND revision = ?4",
            params![
                next.document().project.name.as_str(),
                updated_at.as_str(),
                next.to_canonical_json(),
                self.revision
            ],
        )?;
        if changed != 1 {
            // Dropping the transaction rolls it back.
            return Err(StoreError::Conflict);
        }
        transaction.commit()?;
        self.project = next;
        self.revision += 1;
        Ok(applied)
    }

    /// Closes the database connection.
    pub fn close(self) -> Result<(), StoreError> {
        self.connection
            .close()
            .map_err(|(_, error)| StoreError::from(error))
    }
}

struct Row {
    project_id: String,
    name: String,
    revision: i64,
    document: String,
}

/// The pending migration steps of a stored document.
fn pending(row: &Row, registry: &DomainRegistry) -> Result<Vec<PendingStep>, StoreError> {
    Project::pending_migrations(row.document.as_bytes(), registry).map_err(|error| match error {
        MigrateError::Load(error) => StoreError::InvalidIr(error),
        other => StoreError::Migration(other),
    })
}

/// The copied listing columns must match the document.
fn check_metadata(project: &Project, row: &Row, id: ProjectId) -> Result<(), StoreError> {
    let info = &project.document().project;
    if info.id != id || row.project_id != id.to_string() || row.name != info.name.as_str() {
        return Err(StoreError::Corrupt(
            "the stored project information does not match the document".to_owned(),
        ));
    }
    Ok(())
}

/// Copies the database into `backups/` and verifies the copy. The name is
/// derived from `at`; an existing backup is never replaced.
fn backup(
    connection: &Connection,
    directory: &Path,
    at: &Timestamp,
    document: &str,
) -> Result<PathBuf, StoreError> {
    let failed =
        |what: &str, error: &dyn std::fmt::Display| StoreError::Backup(format!("{what}: {error}"));
    let backups = directory.join(BACKUPS_DIRECTORY);
    fs::create_dir_all(&backups).map_err(|e| failed("creating the backup directory", &e))?;
    let stamp: String = at
        .as_str()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let name = format!("before-migration-{stamp}.sqlite");
    let target = backups.join(&name);
    if target.exists() {
        return Err(StoreError::Backup(format!("`{name}` already exists")));
    }
    // A partial copy from an interrupted attempt has this name; it is never
    // mistaken for a backup and is replaced.
    let partial = backups.join(format!(".{name}.partial"));
    if partial.exists() {
        fs::remove_file(&partial).map_err(|e| failed("removing a partial copy", &e))?;
    }
    let partial_name = partial
        .to_str()
        .ok_or_else(|| StoreError::Backup("the backup path is not valid UTF-8".to_owned()))?;
    connection
        .execute("VACUUM INTO ?1", [partial_name])
        .map_err(|e| failed("copying the database", &e))?;
    let verified = (|| -> Result<bool, rusqlite::Error> {
        let copy = Connection::open_with_flags(
            &partial,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let check: String = copy.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        let copied: String =
            copy.query_row("SELECT document FROM project_ir WHERE id = 1", [], |r| {
                r.get(0)
            })?;
        Ok(check == "ok" && copied == document)
    })();
    match verified {
        Ok(true) => {}
        Ok(false) => {
            let _ = fs::remove_file(&partial);
            return Err(StoreError::Backup(
                "the copy does not match the database".to_owned(),
            ));
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            return Err(failed("verifying the copy", &error));
        }
    }
    let sync = |path: &Path| fs::File::open(path).and_then(|file| file.sync_all());
    sync(&partial).map_err(|e| failed("writing the copy", &e))?;
    fs::rename(&partial, &target).map_err(|e| failed("moving the copy into place", &e))?;
    sync(&backups).map_err(|e| failed("writing the backup directory", &e))?;
    Ok(target)
}

fn read_row(connection: &Connection) -> Result<Row, StoreError> {
    connection
        .query_row(
            "SELECT project_id, name, revision, document FROM project_ir WHERE id = 1",
            [],
            |r| {
                Ok(Row {
                    project_id: r.get(0)?,
                    name: r.get(1)?,
                    revision: r.get(2)?,
                    document: r.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::Corrupt("the project record is missing".to_owned()))
}

fn provenance_time(project: &Project, provenance: text2create_ir::ids::ProvenanceId) -> Timestamp {
    project
        .document()
        .provenance
        .iter()
        .find(|p| p.id == provenance)
        .expect("the write path created this provenance record")
        .at
        .clone()
}

fn open_existing(path: &Path) -> Result<Connection, StoreError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // The first statement reads the file header; a file that is not a SQLite
    // database fails here. The header is checked before any setting is
    // changed, so a database that is refused is never modified.
    match schema::check_header(&connection, path) {
        Err(StoreError::Corrupt(_)) => return Err(StoreError::NotAProject(path.to_owned())),
        result => result?,
    };
    schema::configure(&connection)?;
    Ok(connection)
}

fn write_new(path: &Path, project: &Project, updated_at: &Timestamp) -> Result<(), StoreError> {
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    schema::configure(&connection)?;
    schema::initialize(&mut connection)?;
    connection.execute(
        "INSERT INTO project_ir (id, project_id, name, updated_at, revision, document)
         VALUES (1, ?1, ?2, ?3, 1, ?4)",
        params![
            project.document().project.id.to_string(),
            project.document().project.name.as_str(),
            updated_at.as_str(),
            project.to_canonical_json()
        ],
    )?;
    connection
        .close()
        .map_err(|(_, error)| StoreError::from(error))
}

/// Removes a staging directory created by [`ProjectStore::create`]. Only the
/// database files are removed, and the directory only if it is then empty.
fn remove_staging(staging: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(staging.join(format!("{DATABASE_FILE}{suffix}")));
    }
    let _ = fs::remove_dir(staging);
}

fn summary(path: &Path, id: ProjectId) -> Result<ProjectSummary, StoreError> {
    if !path.is_file() {
        return Err(StoreError::NotFound(id));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    match schema::check_header(&connection, path) {
        Err(StoreError::Corrupt(_)) => return Err(StoreError::NotAProject(path.to_owned())),
        result => result?,
    };
    let (project_id, name, updated_at): (String, String, String) = connection
        .query_row(
            "SELECT project_id, name, updated_at FROM project_ir WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| StoreError::Corrupt("the project record is missing".to_owned()))?;
    let corrupt = |what: &str| StoreError::Corrupt(format!("invalid stored {what}"));
    if project_id != id.to_string() {
        return Err(corrupt("project identifier"));
    }
    Ok(ProjectSummary {
        id,
        name: Text::try_from(name).map_err(|_| corrupt("project name"))?,
        updated_at: Timestamp::try_from(updated_at).map_err(|_| corrupt("timestamp"))?,
    })
}
