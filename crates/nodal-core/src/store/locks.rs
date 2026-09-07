//! Rows of the `lock` table: which host may write a unit.
//!
//! V0 runs on one host, so nothing takes a lock yet. The table and these functions
//! exist because a unit that moves to another machine must not be writable from both,
//! and because a lock carried in a transfer bundle needs somewhere to land.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{HostName, Lock, Timestamp, UnitId};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "lock";

/// Every column [`decode`] reads.
const COLUMNS: &str = "unit_id, host, expires_at";

/// Claim the write on a unit, or take over a claim that has lapsed. `false` when
/// another host holds it and the claim has not expired.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn acquire(conn: &Connection, lock: &Lock, now: Timestamp) -> Result<bool> {
    let taken = row::write(
        conn,
        "INSERT INTO lock (unit_id, host, expires_at) VALUES (?, ?, ?) \
         ON CONFLICT (unit_id) DO UPDATE SET host = excluded.host, \
         expires_at = excluded.expires_at \
         WHERE lock.expires_at <= ? OR lock.host = excluded.host",
        params![
            lock.unit_id.to_string(),
            lock.host.as_str(),
            lock.expires_at.unix_seconds(),
            now.unix_seconds(),
        ],
    )?;
    Ok(taken == 1)
}

/// Who holds the write on a unit, `None` when nobody does.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, unit_id: UnitId) -> Result<Option<Lock>> {
    let sql = format!("SELECT {COLUMNS} FROM lock WHERE unit_id = ?");
    row::one(conn, &sql, params![unit_id.to_string()], decode)
}

/// Every lock a host holds.
///
/// # Errors
/// As [`get`].
pub fn list_for_host(conn: &Connection, host: &HostName) -> Result<Vec<Lock>> {
    let sql = format!("SELECT {COLUMNS} FROM lock WHERE host = ? ORDER BY unit_id");
    row::many(conn, &sql, params![host.as_str()], decode)
}

/// Give up the write on a unit. Only the holder releases it. `false` when it held none.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn release(conn: &Connection, unit_id: UnitId, holder: &HostName) -> Result<bool> {
    let released = row::write(
        conn,
        "DELETE FROM lock WHERE unit_id = ? AND host = ?",
        params![unit_id.to_string(), holder.as_str()],
    )?;
    Ok(released == 1)
}

/// Turn a row into a lock.
fn decode(row: &Row<'_>) -> Result<Lock> {
    Ok(Lock {
        unit_id: row::scalar::<UnitId>(row, TABLE, "unit_id")?,
        host: row::scalar::<HostName>(row, TABLE, "host")?,
        expires_at: row::stamp(row, TABLE, "expires_at")?,
    })
}
