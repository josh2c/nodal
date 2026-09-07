//! Rows of the `lease` table: a resource only one environment may hold at a time.
//!
//! A lease is taken by writing the row, never by reading first and then writing: two
//! environments racing for the same fixed port are separated by the primary key, and
//! the loser is told the resource is held rather than quietly overwriting the winner.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{EnvId, Lease, ResourceKey, Timestamp};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "lease";

/// Every column [`decode`] reads.
const COLUMNS: &str = "resource, environment_id, expires_at";

/// Take a lease, or take over one that has lapsed. `false` when another environment
/// holds it and the claim has not expired.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn acquire(conn: &Connection, lease: &Lease, now: Timestamp) -> Result<bool> {
    let taken = row::write(
        conn,
        "INSERT INTO lease (resource, environment_id, expires_at) VALUES (?, ?, ?) \
         ON CONFLICT (resource) DO UPDATE SET environment_id = excluded.environment_id, \
         expires_at = excluded.expires_at \
         WHERE lease.expires_at <= ? OR lease.environment_id = excluded.environment_id",
        params![
            lease.resource.as_str(),
            lease.environment_id.to_string(),
            lease.expires_at.unix_seconds(),
            now.unix_seconds(),
        ],
    )?;
    Ok(taken == 1)
}

/// Who holds a resource, `None` when nobody does. An expired row is still a row; the
/// caller decides whether a lapsed claim matters.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, resource: &ResourceKey) -> Result<Option<Lease>> {
    let sql = format!("SELECT {COLUMNS} FROM lease WHERE resource = ?");
    row::one(conn, &sql, params![resource.as_str()], decode)
}

/// Every lease an environment holds.
///
/// # Errors
/// As [`get`].
pub fn list_for_environment(conn: &Connection, environment_id: EnvId) -> Result<Vec<Lease>> {
    let sql = format!("SELECT {COLUMNS} FROM lease WHERE environment_id = ? ORDER BY resource");
    row::many(conn, &sql, params![environment_id.to_string()], decode)
}

/// Every lease that has lapsed: what a garbage collection pass reclaims.
///
/// # Errors
/// As [`get`].
pub fn list_expired(conn: &Connection, now: Timestamp) -> Result<Vec<Lease>> {
    let sql = format!("SELECT {COLUMNS} FROM lease WHERE expires_at <= ? ORDER BY resource");
    row::many(conn, &sql, params![now.unix_seconds()], decode)
}

/// Give a resource back. Only the holder releases it, so a stale process cannot free a
/// port another environment has since taken. `false` when it held nothing.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn release(conn: &Connection, resource: &ResourceKey, holder: EnvId) -> Result<bool> {
    let released = row::write(
        conn,
        "DELETE FROM lease WHERE resource = ? AND environment_id = ?",
        params![resource.as_str(), holder.to_string()],
    )?;
    Ok(released == 1)
}

/// Turn a row into a lease.
fn decode(row: &Row<'_>) -> Result<Lease> {
    Ok(Lease {
        resource: row::scalar::<ResourceKey>(row, TABLE, "resource")?,
        environment_id: row::scalar::<EnvId>(row, TABLE, "environment_id")?,
        expires_at: row::stamp(row, TABLE, "expires_at")?,
    })
}
