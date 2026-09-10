//! Rows of the `trash` table: the homes reclaim moved aside and `gc` will remove.
//!
//! One row per environment. The row is written inside the transaction that finishes a
//! reclaim, so a trashed directory and the record of it cannot come apart: there is no
//! moment at which a home sits in the trash directory with nothing that says whose it
//! was.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{EnvId, ProjectId, Slug, Timestamp, Trashed, UnitId};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "trash";

/// Every column [`decode`] reads.
const COLUMNS: &str = "environment_id, unit_id, project_id, slug, home, path, snapshot, \
                       pruned_bytes, trashed_at, expires_at";

/// Record a reclaimed home.
///
/// Writing the same environment twice leaves the row the second call describes, so the
/// step that writes it can be repeated by a resumed operation.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::InvalidValue`] when a
/// path is not UTF-8.
pub fn insert(conn: &Connection, entry: &Trashed) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO trash (environment_id, unit_id, project_id, slug, home, path, snapshot, \
         pruned_bytes, trashed_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (environment_id) DO UPDATE SET path = excluded.path, \
         snapshot = excluded.snapshot, pruned_bytes = excluded.pruned_bytes, \
         trashed_at = excluded.trashed_at, expires_at = excluded.expires_at",
        params![
            entry.environment_id.to_string(),
            entry.unit_id.to_string(),
            entry.project_id.to_string(),
            entry.slug.as_str(),
            row::path_of(&entry.home)?,
            row::path_of(&entry.path)?,
            entry.snapshot.as_deref(),
            stored(entry.pruned_bytes),
            entry.trashed_at.unix_seconds(),
            entry.expires_at.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// One entry by the environment it was the home of, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, environment_id: EnvId) -> Result<Option<Trashed>> {
    let sql = format!("SELECT {COLUMNS} FROM trash WHERE environment_id = ?");
    row::one(conn, &sql, params![environment_id.to_string()], decode)
}

/// Everything in the trash, oldest first.
///
/// # Errors
/// As [`get`].
pub fn list(conn: &Connection) -> Result<Vec<Trashed>> {
    let sql = format!("SELECT {COLUMNS} FROM trash ORDER BY trashed_at, environment_id");
    row::many(conn, &sql, [], decode)
}

/// Everything in one project's trash, oldest first.
///
/// # Errors
/// As [`get`].
pub fn list_for_project(conn: &Connection, project_id: ProjectId) -> Result<Vec<Trashed>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM trash WHERE project_id = ? ORDER BY trashed_at, environment_id"
    );
    row::many(conn, &sql, params![project_id.to_string()], decode)
}

/// Everything whose retention has run out, oldest first. The read `nodal gc` makes.
///
/// # Errors
/// As [`get`].
pub fn list_expired(conn: &Connection, now: Timestamp) -> Result<Vec<Trashed>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM trash WHERE expires_at <= ? ORDER BY trashed_at, environment_id"
    );
    row::many(conn, &sql, params![now.unix_seconds()], decode)
}

/// Forget one entry, once its directory is gone. `false` when there was no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn remove(conn: &Connection, environment_id: EnvId) -> Result<bool> {
    let removed = row::write(
        conn,
        "DELETE FROM trash WHERE environment_id = ?",
        params![environment_id.to_string()],
    )?;
    Ok(removed == 1)
}

/// A byte count as SQLite holds whole numbers, which is a signed sixty-four bit
/// integer.
///
/// A figure past that is eight exabytes of build output, which no home has and no
/// filesystem Nodal runs on would report. It is written as the largest number the
/// column holds rather than refused, because the size of what a prune dropped is not a
/// reason to fail the transaction that records where a person's home went.
fn stored(bytes: u64) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

/// Turn a row into an entry.
fn decode(row: &Row<'_>) -> Result<Trashed> {
    Ok(Trashed {
        environment_id: row::scalar::<EnvId>(row, TABLE, "environment_id")?,
        unit_id: row::scalar::<UnitId>(row, TABLE, "unit_id")?,
        project_id: row::scalar::<ProjectId>(row, TABLE, "project_id")?,
        slug: row::scalar::<Slug>(row, TABLE, "slug")?,
        home: row::path(row, TABLE, "home")?,
        path: row::path(row, TABLE, "path")?,
        snapshot: row::plain::<Option<String>>(row, TABLE, "snapshot")?,
        pruned_bytes: row::number::<u64>(row, TABLE, "pruned_bytes")?,
        trashed_at: row::stamp(row, TABLE, "trashed_at")?,
        expires_at: row::stamp(row, TABLE, "expires_at")?,
    })
}
