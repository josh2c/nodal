//! The frozen registry fixtures: one committed file per schema version.
//!
//! A fixture is an artifact of the version it represents, not a description of one. It
//! is checked in as SQL — the schema that version's migrations produced, the rows a
//! Nodal of that version held, and the `user_version` it stamped — and nothing
//! regenerates it afterwards. That is the whole point: a fixture rebuilt from today's
//! migration files moves whenever they move, so it agrees with the code by
//! construction and proves nothing about the upgrade a person's registry crosses.
//!
//! Text rather than a binary `.db`, because a fixture is reviewed. A committed `.sql`
//! shows in a diff what a version's registry held, which is what a reader has to be
//! able to check; a `.db` hides both the schema and the rows behind a page format, and
//! a wrong byte in one is invisible until a test fails. What is frozen is the content,
//! not SQLite's file layout, and the content is what every claim here is about.
//!
//! Two guards keep a fixture from drifting:
//!
//! 1. a fixture's own `PRAGMA user_version` must be the version its name says, so the
//!    file cannot start from a migration other than the one it claims ([`registry`]);
//! 2. a fixture's schema must be the schema its version's migrations produce, so an
//!    edit to a migration that has already shipped is caught rather than followed
//!    (`a_fixture_holds_the_schema_its_version_produced` in `upgrade.rs`).
//!
//! [`make::freeze`] writes a fixture for a version that has none. It is how a new
//! migration gets its fixture, and it refuses to touch a file that is already there.

pub mod make;
pub mod rows;

use std::path::{Path, PathBuf};

use nodal_core::store::migrations::MIGRATIONS;
use rusqlite::Connection;

/// Where the committed fixtures live.
pub fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("registry")
}

/// The file version `version`'s fixture is in.
pub fn file_of(version: u32) -> PathBuf {
    directory().join(format!("v{version:02}.sql"))
}

/// Every version a fixture is committed for, in order.
pub fn committed() -> Vec<u32> {
    let mut found: Vec<u32> = std::fs::read_dir(directory())
        .expect("the fixture directory is committed")
        .map(|entry| entry.expect("a readable directory entry").file_name())
        .filter_map(|name| version_in(&name.to_string_lossy()))
        .collect();
    found.sort_unstable();
    found
}

/// The version a fixture's file name states, `None` for a file that is not one.
fn version_in(name: &str) -> Option<u32> {
    name.strip_prefix('v')?.strip_suffix(".sql")?.parse().ok()
}

/// Lay the frozen fixture for `version` down in `into` as a registry file, unopened.
///
/// This is where the first guard lives. The version a file carries is read back after
/// it is laid down and held to the version the name promised, so a fixture that was
/// stamped at the wrong migration fails every test that reads it rather than quietly
/// starting the upgrade somewhere else.
///
/// # Panics
/// When the fixture is missing, will not load, or does not carry its own version.
pub fn registry(into: &Path, version: u32) -> PathBuf {
    let file = file_of(version);
    let sql = std::fs::read_to_string(&file)
        .unwrap_or_else(|why| panic!("schema version {version} has no frozen fixture: {why}"));
    let path = into.join("registry.db");
    let conn = Connection::open(&path).expect("a new registry file");
    conn.execute_batch(&sql).unwrap_or_else(|why| panic!("{} did not load: {why}", file.display()));
    conn.close().expect("the fixture connection closes");
    assert_eq!(
        version_of(&path),
        version,
        "{} records a version other than the migration it starts from",
        file.display()
    );
    path
}

/// The schema version a file carries, read without opening it as a store.
///
/// # Panics
/// When the file will not open or will not answer.
pub fn version_of(path: &Path) -> u32 {
    let conn = Connection::open(path).expect("the file opens");
    conn.query_row("PRAGMA user_version", [], |row| row.get(0)).expect("a version is readable")
}

/// Every object of a database's schema, as the kind, the name and the statement SQLite
/// kept for it, in one order whatever order the objects were made in.
///
/// What the second guard compares. `sqlite_master` holds the text of each object as
/// SQLite recorded it, a column added by `ALTER TABLE` included, so two databases the
/// same statements produced answer the same list and one an edited migration produced
/// does not. The order is by kind and name rather than by creation, because a fixture
/// creates its indexes after its tables and a replay creates each one where its
/// migration put it.
///
/// # Panics
/// When the catalogue cannot be read.
pub fn schema_of(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare(
            "SELECT type || ' ' || name || ': ' || sql FROM sqlite_master \
             WHERE sql IS NOT NULL ORDER BY type, name",
        )
        .expect("the catalogue is readable");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("the catalogue is readable")
        .collect::<Result<Vec<String>, _>>()
        .expect("the catalogue is readable")
}

/// A database with migrations one to `version` applied and no rows, for the guard that
/// compares a frozen schema with the one today's migrations produce.
///
/// # Panics
/// When a migration will not apply.
pub fn replayed(version: u32) -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    for migration in MIGRATIONS.iter().filter(|migration| migration.version <= version) {
        conn.execute_batch(migration.sql)
            .unwrap_or_else(|why| panic!("migration {} did not apply: {why}", migration.version));
    }
    conn
}
