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
//! Four guards keep a fixture honest:
//!
//! 1. a fixture's own `PRAGMA user_version` must be the version its name says, so the
//!    file cannot start from a migration other than the one it claims ([`registry`]);
//! 2. a fixture's schema must be the schema its version's migrations produce, so an
//!    edit to a migration that has already shipped is caught rather than followed
//!    (`a_fixture_holds_the_schema_its_version_produced` in `upgrade.rs`);
//! 3. every cell of a fixture must still be there after the upgrade, which is the claim
//!    the per-subject tests cannot make on their own ([`cells`]);
//! 4. every place a migration made must hold something in that migration's own fixture,
//!    so a version cannot be covered by a file that says nothing about it
//!    ([`places_made`]). A file existing was the coverage claim, and a file existing is
//!    not coverage.
//!
//! [`make::write`] writes a fixture for a version that has none. It is how a new
//! migration gets its fixture, and it refuses to touch a file that is already there.

pub mod cells;
pub mod make;
pub mod rows;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
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

/// Every object of a database's schema, by the kind and name that address it, as the
/// statement SQLite kept for it.
///
/// What the second guard compares, one object at a time: a schema held whole compares as
/// two walls of text and a reader has to find the difference by eye, while a map compares
/// per object and the failure names the table. `sqlite_master` holds the text of each
/// object as SQLite recorded it, a column added by `ALTER TABLE` included, so two
/// databases the same statements produced answer the same map and one an edited migration
/// produced does not. Creation order is not compared, because a fixture creates its
/// indexes after its tables and a replay creates each one where its migration put it.
///
/// # Panics
/// When the catalogue cannot be read.
pub fn schema_of(conn: &Connection) -> BTreeMap<String, String> {
    let mut statement = conn
        .prepare("SELECT type || ' ' || name, sql FROM sqlite_master WHERE sql IS NOT NULL")
        .expect("the catalogue is readable");
    statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .expect("the catalogue is readable")
        .collect::<Result<BTreeMap<String, String>, _>>()
        .expect("the catalogue is readable")
}

/// A place a migration made, which is somewhere a person's registry can hold a fact it
/// could not hold before: a table, or a column of a table that was already there.
///
/// What the fourth guard is over. A fixture for the version that made the place has to
/// hold something in it, or the version's own migration is the one thing its fixture says
/// nothing about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    /// The table.
    pub table: String,
    /// The column, or `None` for a table that is itself new.
    pub column: Option<String>,
}

impl fmt::Display for Place {
    fn fmt(&self, into: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.column {
            Some(column) => write!(into, "{}.{column}", self.table),
            None => write!(into, "the table {}", self.table),
        }
    }
}

/// Every place version `version`'s migration made that the version before it had not.
///
/// Read out of the schemas rather than listed by hand, so a migration that adds a column
/// is covered by this without anybody remembering to add it here. A migration that adds
/// no place — an index rule, a widened constraint, a back-fill — makes none, and asks
/// nothing of its fixture.
///
/// # Panics
/// When a migration will not apply or the catalogue cannot be read.
pub fn places_made(version: u32) -> Vec<Place> {
    let before = columns_of(&replayed(version.saturating_sub(1)));
    let after = columns_of(&replayed(version));
    let mut made = Vec::new();
    for (table, columns) in after {
        let Some(had) = before.get(&table) else {
            made.push(Place { table, column: None });
            continue;
        };
        made.extend(
            columns
                .difference(had)
                .map(|column| Place { table: table.clone(), column: Some(column.clone()) }),
        );
    }
    made.sort_unstable();
    made
}

/// Whether a fixture holds anything in `place`.
///
/// A table is filled when it holds a row. A column is filled when some row holds a value
/// in it that the migration did not put there: not null, and not the default the column
/// was added with. That second half is the difference between a guard and a formality —
/// `NOT NULL DEFAULT 0` leaves no row null, so a fixture that says nothing at all about
/// such a column would pass on the migration's own default.
///
/// # Panics
/// When the table cannot be counted or the catalogue cannot be read.
pub fn filled(conn: &Connection, place: &Place) -> bool {
    let Some(column) = &place.column else {
        return count(conn, &format!("SELECT count(*) FROM \"{}\"", place.table)) > 0;
    };
    let held = match default_of(conn, &place.table, column) {
        Some(default) => format!("\"{column}\" IS NOT NULL AND \"{column}\" IS NOT ({default})"),
        None => format!("\"{column}\" IS NOT NULL"),
    };
    count(conn, &format!("SELECT count(*) FROM \"{}\" WHERE {held}", place.table)) > 0
}

/// The default a column was declared with, as SQL, and `None` for a column with none.
fn default_of(conn: &Connection, table: &str, column: &str) -> Option<String> {
    conn.query_row(
        "SELECT dflt_value FROM pragma_table_info(?) WHERE name = ?",
        [table, column],
        |row| row.get::<_, Option<String>>(0),
    )
    .expect("the catalogue is readable")
}

/// One count, for a query that answers exactly one.
fn count(conn: &Connection, sql: &str) -> u32 {
    conn.query_row(sql, [], |row| row.get(0)).expect("a count is readable")
}

/// Every table of a database and the columns it holds.
///
/// # Panics
/// When the catalogue cannot be read.
pub fn columns_of(conn: &Connection) -> BTreeMap<String, BTreeSet<String>> {
    let mut statement = conn
        .prepare(
            "SELECT m.name, info.name FROM sqlite_master AS m \
             JOIN pragma_table_info(m.name) AS info \
             WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'",
        )
        .expect("the catalogue is readable");
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut rows = statement.query([]).expect("the catalogue is readable");
    while let Some(row) = rows.next().expect("the catalogue is readable") {
        let table: String = row.get(0).expect("a table has a name");
        let column: String = row.get(1).expect("a column has a name");
        found.entry(table).or_default().insert(column);
    }
    found
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
