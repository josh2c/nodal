//! The migration runner and the list of migrations.
//!
//! Migrations are data: a table of numbered SQL files, applied in order, with the
//! version SQLite already carries (`PRAGMA user_version`) as the record of how far a
//! database has come. There is no code branch per migration and no bookkeeping table of
//! our own to keep consistent with the schema it describes.

use crate::store::Store;
use crate::{Error, Result};

/// One numbered step from an older schema to the next.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Its position in the sequence, counting from one.
    pub version: u32,
    /// What the step does, for the error when it fails.
    pub name: &'static str,
    /// The statements, applied as one batch inside the runner's transaction.
    pub sql: &'static str,
}

/// Every migration, in order. Appending is the only permitted edit.
pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "init", sql: include_str!("migrations/0001_init.sql") },
    Migration { version: 2, name: "journal", sql: include_str!("migrations/0002_journal.sql") },
    Migration { version: 3, name: "ports", sql: include_str!("migrations/0003_ports.sql") },
    Migration { version: 4, name: "trash", sql: include_str!("migrations/0004_trash.sql") },
];

/// The schema version a database is brought to by [`run`]. Kept as a literal rather
/// than derived from the table's length, so that a version appears in a diff.
pub const SCHEMA_VERSION: u32 = 4;

/// Bring `store` up to [`SCHEMA_VERSION`].
///
/// The common case is a database that is already current, and that case takes no write
/// lock at all: the version is read first, and only a database that is behind is opened
/// for writing. Without that, every `nodal` invocation would queue behind every other
/// one just to open the registry.
///
/// When a migration is needed it happens inside one `IMMEDIATE` transaction, and the
/// version is read again inside it, so a second process that migrated the file while
/// this one was waiting for the lock leaves nothing to redo.
///
/// # Errors
/// [`Error::StoreTooNew`] when the database was written by a later version of Nodal,
/// [`Error::StoreMigration`] when a step failed, [`Error::Store`] when the version
/// could not be read or the write lock could not be taken.
pub fn run(store: &mut Store) -> Result<()> {
    let path = store.path().to_path_buf();
    match version(store.conn(), &path)? {
        applied if applied == SCHEMA_VERSION => return Ok(()),
        applied if applied > SCHEMA_VERSION => {
            return Err(Error::StoreTooNew { path, found: applied, supported: SCHEMA_VERSION });
        }
        _ => {}
    }
    let tx = store.transaction()?;
    let applied = version(&tx, &path)?;
    if applied > SCHEMA_VERSION {
        return Err(Error::StoreTooNew { path, found: applied, supported: SCHEMA_VERSION });
    }
    for migration in MIGRATIONS.iter().filter(|migration| migration.version > applied) {
        tx.execute_batch(migration.sql).map_err(|source| Error::StoreMigration {
            path: path.clone(),
            version: migration.version,
            name: migration.name,
            source: Box::new(source),
        })?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|source| Error::Store { path: path.clone(), source: Box::new(source) })?;
    tx.commit().map_err(|source| Error::Store { path, source: Box::new(source) })
}

/// The schema version a database is currently at; zero for an empty file.
fn version(conn: &rusqlite::Connection, path: &std::path::Path) -> Result<u32> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|source| Error::Store { path: path.to_path_buf(), source: Box::new(source) })
}

#[cfg(test)]
mod tests {
    use super::{MIGRATIONS, SCHEMA_VERSION};

    #[test]
    fn versions_are_consecutive_from_one_and_end_at_the_declared_version() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                u32::try_from(index + 1).ok(),
                Some(migration.version),
                "{} is out of sequence",
                migration.name
            );
        }
        assert_eq!(MIGRATIONS.last().map(|last| last.version), Some(SCHEMA_VERSION));
    }
}
