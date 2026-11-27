//! SQLite persistence for Text2Create projects (#12, ADR-0015).
//!
//! Every project is a self-contained directory under a root directory:
//!
//! ```text
//! <root>/
//! └── prj_<uuid>/            one directory per project, named by its ID
//!     └── project.sqlite     the project database (with -wal/-shm while open)
//! ```
//!
//! The database stores the Project IR document in canonical JSON. It is
//! written only with the output of [`Project::create`] or [`Project::apply`],
//! and read only through [`Project::load`], so every IR rule applies to stored
//! data. This crate adds no other way to change a project, and it has no
//! Tauri dependency.

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
use text2create_ir::{Applied, LoadError, Project, WriteEnv, WriteError};

/// File name of the project database inside a project directory.
pub const DATABASE_FILE: &str = "project.sqlite";

/// Prefix of directories that are still being created. They are never listed.
const CREATING_PREFIX: &str = ".creating-";

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
    /// The write path rejected the change set.
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error("the project was saved by another connection since it was opened; reopen it")]
    Conflict,
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
    pub fn open(&self, id: ProjectId) -> Result<OpenProject, StoreError> {
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
        let project = Project::load(row.document.as_bytes(), &self.registry)
            .map_err(StoreError::InvalidIr)?;
        let info = &project.document().project;
        if info.id != id || row.project_id != id.to_string() || row.name != info.name.as_str() {
            return Err(StoreError::Corrupt(
                "the stored project information does not match the document".to_owned(),
            ));
        }
        Ok(OpenProject {
            connection,
            project,
            revision: row.revision,
            directory,
            registry: Arc::clone(&self.registry),
        })
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
