//! Rows of the `trash` table: the homes reclaim moved aside and `gc` will remove.
//!
//! One row per environment. The row is written inside the transaction that finishes a
//! reclaim, so a trashed directory and the record of it cannot come apart: there is no
//! moment at which a home sits in the trash directory with nothing that says whose it
//! was.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{EnvId, ProjectId, Rested, Slug, Timestamp, Trashed, UnitId};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "trash";

/// Every column [`decode`] reads.
const COLUMNS: &str = "environment_id, unit_id, project_id, slug, home, path, snapshot, \
                       pruned_bytes, rested, trashed_at, expires_at";

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
         pruned_bytes, rested, trashed_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (environment_id) DO UPDATE SET path = excluded.path, \
         snapshot = excluded.snapshot, pruned_bytes = excluded.pruned_bytes, \
         rested = excluded.rested, trashed_at = excluded.trashed_at, \
         expires_at = excluded.expires_at",
        params![
            entry.environment_id.to_string(),
            entry.unit_id.to_string(),
            entry.project_id.to_string(),
            entry.slug.as_str(),
            row::path_of(&entry.home)?,
            row::path_of(&entry.path)?,
            entry.snapshot.as_deref(),
            stored(entry.pruned_bytes),
            written(&entry.rested)?,
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

/// What a verdict rested on, as the one text column holds it.
///
/// [`Rested::Unrecorded`] is written as the empty string, which is what every row
/// before this column already holds, so a row Nodal writes and a row the migration made
/// read back as the same fact.
fn written(rested: &Rested) -> Result<String> {
    if matches!(rested, Rested::Unrecorded) {
        return Ok(String::new());
    }
    serde_json::to_string(rested).map_err(|why| crate::Error::InvalidValue {
        kind: "what the reclaim's check rested on",
        value: why.to_string(),
    })
}

/// The same, read back. Text that will not parse is read as [`Rested::Unrecorded`],
/// which is the strict answer: `gc` then reads the home again and believes only what
/// that reading shows it.
fn read(text: &str) -> Rested {
    if text.is_empty() {
        return Rested::Unrecorded;
    }
    serde_json::from_str(text).unwrap_or_default()
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
        rested: read(&row::plain::<String>(row, TABLE, "rested")?),
        trashed_at: row::stamp(row, TABLE, "trashed_at")?,
        expires_at: row::stamp(row, TABLE, "expires_at")?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{read, written};
    use crate::model::{Outside, Rested};

    /// A verdict that rested on a copy travels to the column and back unchanged. The
    /// sweep that decides whether a directory may go reads exactly what the reclaim
    /// wrote.
    #[test]
    fn what_a_verdict_rested_on_survives_the_column() {
        let rested = Rested::Safe {
            copies: vec![Outside {
                repository: PathBuf::from("/w/project"),
                references: vec![String::from("refs/remotes/origin/topic")],
                commits: 3,
            }],
        };
        assert_eq!(read(&written(&rested).unwrap()), rested);
    }

    /// A forced reclaim named its loss before it moved anything, and the row says so.
    #[test]
    fn a_forced_reclaim_is_written_down_as_forced() {
        assert_eq!(read(&written(&Rested::Forced).unwrap()), Rested::Forced);
    }

    /// The empty string is what the migration left in every row an older Nodal wrote,
    /// and what this module writes for a verdict it has nothing to say about. Both read
    /// as the strict answer rather than as a claim the home was safe.
    #[test]
    fn a_row_that_says_nothing_reads_as_unrecorded() {
        assert_eq!(written(&Rested::Unrecorded).unwrap(), "");
        assert_eq!(read(""), Rested::Unrecorded);
    }

    /// Text no version of this model wrote is not a reason to fail a sweep, and it is
    /// not evidence either. It reads as the answer that makes `gc` ask again.
    #[test]
    fn text_that_will_not_parse_reads_as_unrecorded() {
        assert_eq!(read("{\"kind\":\"from a later nodal\"}"), Rested::Unrecorded);
    }
}
