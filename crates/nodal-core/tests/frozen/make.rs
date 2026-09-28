//! Writing the frozen fixture for a schema version that has none.
//!
//! One function, and it is not part of any assertion. A fixture is written once, by the
//! person who appends a migration: the version is lived through — each migration applied
//! in order, and the rows a Nodal of that version wrote applied at the version that could
//! hold them — and what the database then holds is written out as SQL and committed.
//! [`write`] answers `None` for a version whose file is already there, so running it
//! again cannot rewrite a fixture that has shipped.
//!
//! The file carries the schema SQLite itself recorded, statement for statement, rather
//! than the migration text that produced it. A column a migration added with `ALTER
//! TABLE` is inside the `CREATE TABLE` SQLite keeps, and a table migration 16 made again
//! under a new name carries the name it was renamed to, so the whole schema of a version
//! reaches the file as one list of statements a reader can check.
//!
//! What the first seventeen fixtures prove, and what they do not, is in
//! `tests/fixtures/registry/README.md`.

use std::fmt::Write as _;
use std::path::PathBuf;

use nodal_core::store::migrations::MIGRATIONS;
use rusqlite::Connection;
use rusqlite::types::ValueRef;

use super::file_of;
use super::rows::ROWS;

/// Write the frozen fixture for `version`, or answer `None` when it is already frozen.
///
/// # Panics
/// When the database cannot be built or read, or the file cannot be written.
pub fn write(version: u32) -> Option<PathBuf> {
    let file = file_of(version);
    if file.exists() {
        return None;
    }
    let conn = lived(version);
    std::fs::write(&file, dump(&conn, version)).expect("the fixture directory is writable");
    Some(file)
}

/// A database that has lived through every version up to `version`.
///
/// The rows for a version are applied straight after that version's migration, so a
/// back-fill a later migration performs runs over rows that were already there. That is
/// the order a person's registry reached its present state in, and rows written after
/// every migration had run would stand in front of the migrations that were meant to
/// touch them.
fn lived(version: u32) -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    for migration in MIGRATIONS.iter().filter(|migration| migration.version <= version) {
        let at = migration.version;
        conn.execute_batch(migration.sql)
            .unwrap_or_else(|why| panic!("migration {at} did not apply: {why}"));
        if let Some((_, rows)) = ROWS.iter().find(|(wrote, _)| *wrote == at) {
            conn.execute_batch(rows)
                .unwrap_or_else(|why| panic!("the version {at} rows did not apply: {why}"));
        }
    }
    conn
}

/// The whole of a database as the statements that build it again.
fn dump(conn: &Connection, version: u32) -> String {
    let mut out = header(version);
    writeln!(out, "PRAGMA user_version = {version};\n").expect("a string takes a write");
    for statement in schema(conn) {
        writeln!(out, "{statement};\n").expect("a string takes a write");
    }
    for table in tables(conn) {
        out.push_str(&inserts(conn, &table));
    }
    out
}

/// What the file says about itself, so a reader knows it is data and not output.
fn header(version: u32) -> String {
    format!(
        "-- A frozen registry fixture: the schema and rows of version {version}.\n\
         --\n\
         -- Data, not a script. It was written once, by the Nodal that appended migration\n\
         -- {version}, and nothing regenerates it: a fixture rebuilt from today's migrations\n\
         -- moves whenever they move and proves nothing about the upgrade a person's\n\
         -- registry crosses. Edit it only to correct what version {version} really held.\n\
         --\n\
         -- `crates/nodal-core/tests/upgrade.rs` opens this with the current binary,\n\
         -- migrates it, and asserts every row below is still there with the meaning it\n\
         -- had here.\n\n"
    )
}

/// Every statement that made an object of the schema, in creation order.
///
/// Creation order is an order the statements may be run in again: an index follows the
/// table it is on, and a table migration 16 made again follows the rows it copied.
fn schema(conn: &Connection) -> Vec<String> {
    read(conn, "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid")
}

/// Every table's name, in creation order, which is an order they may be filled in.
fn tables(conn: &Connection) -> Vec<String> {
    read(conn, "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY rowid")
}

/// One column of text from the catalogue.
fn read(conn: &Connection, sql: &str) -> Vec<String> {
    let mut statement = conn.prepare(sql).expect("the catalogue is readable");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("the catalogue is readable")
        .collect::<Result<Vec<String>, _>>()
        .expect("the catalogue is readable")
}

/// One `INSERT` per row of `table`, in rowid order, and nothing for an empty table.
fn inserts(conn: &Connection, table: &str) -> String {
    let mut statement =
        conn.prepare(&format!("SELECT * FROM \"{table}\"")).expect("a table is readable");
    let columns = statement.column_names().join(", ");
    let width = statement.column_count();
    let mut rows = statement.query([]).expect("a table is readable");
    let mut out = String::new();
    while let Some(row) = rows.next().expect("a row is readable") {
        let values: Vec<String> = (0..width)
            .map(|index| literal(row.get_ref(index).expect("a value is readable")))
            .collect();
        writeln!(out, "INSERT INTO {table} ({columns})\nVALUES ({});", values.join(", "))
            .expect("a string takes a write");
    }
    out
}

/// One value as SQL text that reads back as the same value.
fn literal(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "NULL".to_owned(),
        ValueRef::Integer(number) => number.to_string(),
        ValueRef::Real(number) => format!("{number:?}"),
        ValueRef::Text(bytes) => {
            format!("'{}'", String::from_utf8_lossy(bytes).replace('\'', "''"))
        }
        ValueRef::Blob(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                write!(hex, "{byte:02X}").expect("a string takes a write");
            }
            format!("X'{hex}'")
        }
    }
}
