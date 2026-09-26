//! Every value a fixture holds, addressed the way a row is addressed.
//!
//! The per-subject tests in `upgrade.rs` read one subject each back through today's
//! readers, which is what says a unit is still a unit and a verdict is still a verdict.
//! They cannot say that *nothing else* was lost: each one names the columns it names, a
//! column nobody named is a column nobody checks, and a migration that drops one goes
//! green. That is the shape of the defect this module answers. A table rebuilt with a
//! column left out of its `SELECT` list — which is the shape migration 16 already uses —
//! erases a column of every row in the table, and no assertion about `slug` or `kind`
//! notices.
//!
//! So the whole of a fixture is read before the upgrade, cell by cell, and held to what
//! it held afterwards. The claim is total rather than chosen: every row of every table,
//! addressed by its primary key, keeps every value it had. Nothing has to be added here
//! when a migration adds a column, because the columns are read out of the database
//! rather than listed.
//!
//! The expectation is the frozen file itself, not a second database to compare against.
//! Comparing one migrated fixture with another would be blind to a migration that
//! destroys the same thing in both, which is precisely what a bad migration does.
//!
//! What this forbids is a migration that changes a value a person's registry already
//! held. No migration in one to seventeen does: a migration adds a column, and a column
//! a fixture predates holds no cell here to compare. A migration that really has to
//! rewrite an existing value — a back-fill over rows that are already there — fails this
//! check and has to say so in the test that expects it. That is the right way round for
//! a launch blocker: silence is the thing being removed.

use std::collections::BTreeSet;

use rusqlite::types::Value;
use rusqlite::{Connection, Statement, params_from_iter};

/// One row of one table: the key that finds it again, and every value it holds.
#[derive(Debug, Clone)]
pub struct Row {
    /// The table the row is in.
    pub table: String,
    /// Its primary key, column by column, in the order the key is declared.
    pub key: Vec<(String, Value)>,
    /// Every column of the row, the key's own columns included.
    pub cells: Vec<(String, Value)>,
}

impl Row {
    /// The row as a reader names it: `unit[id=01J8Z6H…2]`.
    fn address(&self) -> String {
        let key: Vec<String> =
            self.key.iter().map(|(column, value)| format!("{column}={}", shown(value))).collect();
        format!("{}[{}]", self.table, key.join(", "))
    }
}

/// Every row of every table, read as data rather than through a store.
///
/// # Panics
/// When the catalogue will not answer, or a table carries no primary key: a row with no
/// key cannot be found again after the upgrade, so it cannot be held to anything.
pub fn of(conn: &Connection) -> Vec<Row> {
    let mut rows = Vec::new();
    for table in tables(conn) {
        let key = key_of(conn, &table);
        assert!(!key.is_empty(), "{table} has no primary key, so its rows cannot be followed");
        rows.extend(rows_of(conn, &table, &key));
    }
    rows
}

/// What a database no longer holds of `before`: one line per loss, and none for none.
///
/// The lines are what the failure prints, so each one says where the loss is and what it
/// was: the table, the row, the column, the value the fixture held and the value there
/// now.
///
/// # Panics
/// When the catalogue or a row will not answer.
pub fn lost(before: &[Row], conn: &Connection) -> Vec<String> {
    let present = tables(conn);
    let mut losses = dropped_tables(before, &present);
    for row in before.iter().filter(|row| present.contains(&row.table)) {
        losses.extend(losses_of(conn, row));
    }
    losses
}

/// What one row lost, which is nothing when every cell of it is still there.
fn losses_of(conn: &Connection, row: &Row) -> Vec<String> {
    let columns: Vec<String> = row.key.iter().map(|(column, _)| column.clone()).collect();
    let sql = format!(
        "SELECT * FROM \"{}\" WHERE {}",
        row.table,
        columns.iter().map(|column| format!("\"{column}\" IS ?")).collect::<Vec<_>>().join(" AND ")
    );
    let mut statement = conn.prepare(&sql).expect("a table is readable by its key");
    let keys = row.key.iter().map(|(_, value)| value.clone());
    let mut found = statement.query(params_from_iter(keys)).expect("a row is readable");
    let Some(now) = found.next().expect("a row is readable") else {
        return vec![format!("the row {} is gone", row.address())];
    };
    let held: Vec<String> = now.as_ref().column_names().iter().map(|&it| it.to_owned()).collect();
    row.cells
        .iter()
        .filter_map(|(column, was)| {
            if !held.contains(column) {
                return Some(format!("the column {}.{column} is gone", row.table));
            }
            let is: Value = now.get(column.as_str()).expect("a value is readable");
            (is != *was).then(|| {
                format!(
                    "{} lost {column}: it held {}, it holds {}",
                    row.address(),
                    shown(was),
                    shown(&is)
                )
            })
        })
        .collect()
}

/// Each table the fixture held rows in that the migrated database no longer has at all.
///
/// One line per table rather than one per row: a log of a thousand events that lost its
/// table has one thing wrong with it, not a thousand.
fn dropped_tables(before: &[Row], present: &BTreeSet<String>) -> Vec<String> {
    before
        .iter()
        .map(|row| row.table.as_str())
        .filter(|table| !present.contains(*table))
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .map(|table| format!("the table {table} is gone"))
        .collect()
}

/// Every table of a database, the ones SQLite keeps for itself left out.
fn tables(conn: &Connection) -> BTreeSet<String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .expect("the catalogue is readable");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("the catalogue is readable")
        .collect::<Result<BTreeSet<String>, _>>()
        .expect("the catalogue is readable")
}

/// The primary key of a table, in the order the key declares its columns.
fn key_of(conn: &Connection, table: &str) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT name, pk FROM pragma_table_info(?) WHERE pk > 0 ORDER BY pk")
        .expect("the catalogue is readable");
    statement
        .query_map([table], |row| row.get::<_, String>(0))
        .expect("the catalogue is readable")
        .collect::<Result<Vec<String>, _>>()
        .expect("the catalogue is readable")
}

/// Every row of one table, keyed by the columns its primary key names.
fn rows_of(conn: &Connection, table: &str, key: &[String]) -> Vec<Row> {
    let mut statement =
        conn.prepare(&format!("SELECT * FROM \"{table}\"")).expect("a table is readable");
    let columns = names(&statement);
    let width = columns.len();
    let mut found = statement.query([]).expect("a table is readable");
    let mut rows = Vec::new();
    while let Some(row) = found.next().expect("a row is readable") {
        let cells: Vec<(String, Value)> = (0..width)
            .map(|index| {
                (columns[index].clone(), row.get::<_, Value>(index).expect("a value is readable"))
            })
            .collect();
        rows.push(Row {
            table: table.to_owned(),
            key: cells.iter().filter(|(column, _)| key.contains(column)).cloned().collect(),
            cells,
        });
    }
    rows
}

/// The column names of a prepared statement, owned.
fn names(statement: &Statement<'_>) -> Vec<String> {
    statement.column_names().iter().map(|&name| name.to_owned()).collect()
}

/// One value as a failure line shows it: readable, and never truncated into a lie.
fn shown(value: &Value) -> String {
    match value {
        Value::Null => "nothing".to_owned(),
        Value::Integer(number) => number.to_string(),
        Value::Real(number) => format!("{number:?}"),
        Value::Text(text) => format!("'{text}'"),
        Value::Blob(bytes) => format!("{} bytes", bytes.len()),
    }
}
