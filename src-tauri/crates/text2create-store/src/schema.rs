//! Database identification and schema migrations.
//!
//! A project database is identified by its SQLite `application_id`, and its
//! schema version is the SQLite `user_version`. Migrations are ordered SQL
//! scripts compiled into the application; migration `n` (1-based) upgrades
//! the schema from version `n - 1` to `n`. They run automatically when a
//! project is opened, all in one transaction, so a failing migration leaves
//! the database unchanged.

use crate::StoreError;
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;

/// `application_id` of Text2Create project databases (ASCII `T2CP`).
pub const APPLICATION_ID: i32 = 0x5432_4350;

/// Schema migrations, in order. Never edit a released migration; add a new one.
pub(crate) const MIGRATIONS: &[&str] = &[
    // 1: the Project IR document in canonical JSON. `project_id`, `name`, and
    // `updated_at` duplicate document fields so that projects can be listed
    // without loading them; they are written in the same transaction.
    // `revision` counts saves and detects concurrent writers.
    "CREATE TABLE project_ir (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        project_id TEXT NOT NULL,
        name TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        revision INTEGER NOT NULL CHECK (revision >= 1),
        document TEXT NOT NULL
    ) STRICT;",
];

/// The schema version this build creates and reads.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Per-connection settings. WAL keeps readers consistent during writes, and
/// `synchronous = FULL` makes every committed transaction durable.
pub(crate) fn configure(connection: &Connection) -> Result<(), StoreError> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let mode: String = connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Corrupt(format!(
            "could not enable write-ahead logging (journal mode `{mode}`)"
        )));
    }
    connection.pragma_update(None, "synchronous", "FULL")?;
    Ok(())
}

/// The `application_id` and `user_version` of a database.
pub(crate) fn header(connection: &Connection) -> Result<(i32, u32), StoreError> {
    let application_id: i32 = connection.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = u32::try_from(version)
        .map_err(|_| StoreError::Corrupt(format!("invalid schema version {version}")))?;
    Ok((application_id, version))
}

/// Checks that `connection` is a Text2Create project database whose schema
/// this build can read, without modifying it.
pub(crate) fn check_header(connection: &Connection, path: &Path) -> Result<u32, StoreError> {
    let (application_id, version) = header(connection)?;
    if application_id != APPLICATION_ID || version == 0 {
        return Err(StoreError::NotAProject(path.to_owned()));
    }
    if version > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(version)
}

/// Initializes an empty database with the current schema.
pub(crate) fn initialize(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
    run(&transaction, MIGRATIONS, 0)?;
    transaction.commit()?;
    Ok(())
}

/// Upgrades an existing project database to the current schema.
pub(crate) fn migrate(connection: &mut Connection, path: &Path) -> Result<(), StoreError> {
    migrate_with(connection, path, MIGRATIONS)
}

fn migrate_with(
    connection: &mut Connection,
    path: &Path,
    migrations: &[&str],
) -> Result<(), StoreError> {
    let supported = migrations.len() as u32;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Read the header inside the write transaction, so that a concurrent
    // migration cannot run the same steps twice.
    let (application_id, version) = header(&transaction)?;
    if application_id != APPLICATION_ID || version == 0 {
        return Err(StoreError::NotAProject(path.to_owned()));
    }
    if version > supported {
        return Err(StoreError::NewerSchema {
            found: version,
            supported,
        });
    }
    if version < supported {
        run(&transaction, migrations, version)?;
        transaction.commit()?;
    }
    Ok(())
}

/// Runs the migrations after `from` and sets `user_version`, inside the
/// caller's transaction.
fn run(connection: &Connection, migrations: &[&str], from: u32) -> Result<(), StoreError> {
    for (index, sql) in migrations.iter().enumerate().skip(from as usize) {
        connection.execute_batch(sql)?;
        connection.pragma_update(None, "user_version", index as i64 + 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2: &str = "ALTER TABLE project_ir ADD COLUMN note TEXT;";

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize(&mut connection).unwrap();
        connection
    }

    fn columns(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare("SELECT name FROM pragma_table_info('project_ir')")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn initialize_sets_the_header_and_current_schema() {
        let connection = database();
        assert_eq!(
            header(&connection).unwrap(),
            (APPLICATION_ID, SCHEMA_VERSION)
        );
        assert!(columns(&connection).contains(&"document".to_owned()));
    }

    #[test]
    fn migrations_upgrade_once_and_record_the_version() {
        let mut connection = database();
        let path = Path::new("test");
        migrate_with(&mut connection, path, &[MIGRATIONS[0], V2]).unwrap();
        assert_eq!(header(&connection).unwrap().1, 2);
        assert!(columns(&connection).contains(&"note".to_owned()));
        // Running again is a no-op.
        migrate_with(&mut connection, path, &[MIGRATIONS[0], V2]).unwrap();
        assert_eq!(header(&connection).unwrap().1, 2);
    }

    #[test]
    fn a_failing_migration_leaves_the_database_unchanged() {
        let mut connection = database();
        let broken = [MIGRATIONS[0], V2, "THIS IS NOT SQL;"];
        assert!(migrate_with(&mut connection, Path::new("test"), &broken).is_err());
        // Neither the second nor the third migration was applied.
        assert_eq!(header(&connection).unwrap().1, 1);
        assert!(!columns(&connection).contains(&"note".to_owned()));
    }

    #[test]
    fn newer_or_foreign_databases_are_refused_without_changes() {
        let mut connection = database();
        connection.pragma_update(None, "user_version", 7).unwrap();
        assert!(matches!(
            migrate(&mut connection, Path::new("test")),
            Err(StoreError::NewerSchema {
                found: 7,
                supported: SCHEMA_VERSION
            })
        ));
        assert_eq!(header(&connection).unwrap().1, 7);
        let mut foreign = Connection::open_in_memory().unwrap();
        foreign
            .execute_batch("CREATE TABLE t (x); PRAGMA user_version = 1;")
            .unwrap();
        assert!(matches!(
            migrate(&mut foreign, Path::new("test")),
            Err(StoreError::NotAProject(_))
        ));
    }
}
