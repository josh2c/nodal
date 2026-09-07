//! Rows of the `session` table: who is attached to an environment right now.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{Actor, ActorKind, ActorName, EnvId, Session, SessionId, Timestamp};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "session";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, environment_id, actor_kind, actor_name, pid, started_at, ended_at";

/// Record an actor attaching to an environment.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when the session is already recorded,
/// [`crate::Error::Store`] on any other failure.
pub fn insert(conn: &Connection, session: &Session) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO session (id, environment_id, actor_kind, actor_name, pid, started_at, \
         ended_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            session.id.to_string(),
            session.environment_id.to_string(),
            row::name_of(&session.actor.kind, "actor kind")?,
            session.actor.name.as_str(),
            session.pid,
            session.started_at.unix_seconds(),
            session.ended_at.map(Timestamp::unix_seconds),
        ],
    )?;
    Ok(())
}

/// One session by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: SessionId) -> Result<Option<Session>> {
    let sql = format!("SELECT {COLUMNS} FROM session WHERE id = ?");
    row::one(conn, &sql, params![id.to_string()], decode)
}

/// Every session of an environment, oldest first.
///
/// # Errors
/// As [`get`].
pub fn list_for_environment(conn: &Connection, environment_id: EnvId) -> Result<Vec<Session>> {
    let sql = format!("SELECT {COLUMNS} FROM session WHERE environment_id = ? ORDER BY id");
    row::many(conn, &sql, params![environment_id.to_string()], decode)
}

/// The sessions still attached to an environment: "who is in this unit right now".
///
/// # Errors
/// As [`get`].
pub fn list_open(conn: &Connection, environment_id: EnvId) -> Result<Vec<Session>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM session WHERE environment_id = ? AND ended_at IS NULL ORDER BY id"
    );
    row::many(conn, &sql, params![environment_id.to_string()], decode)
}

/// Every session still open, over every environment: what a scan of the process table
/// is reconciled against.
///
/// # Errors
/// As [`get`].
pub fn list_open_all(conn: &Connection) -> Result<Vec<Session>> {
    let sql = format!("SELECT {COLUMNS} FROM session WHERE ended_at IS NULL ORDER BY id");
    row::many(conn, &sql, params![], decode)
}

/// Record an actor detaching; `false` when the session is unknown or already ended.
///
/// Only an open session is closed, so a second detach — a shell hook that fires twice —
/// leaves the first end time standing rather than moving it.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn end(conn: &Connection, id: SessionId, at: Timestamp) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE session SET ended_at = ? WHERE id = ? AND ended_at IS NULL",
        params![at.unix_seconds(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Turn a row into a session.
fn decode(row: &Row<'_>) -> Result<Session> {
    Ok(Session {
        id: row::scalar::<SessionId>(row, TABLE, "id")?,
        environment_id: row::scalar::<EnvId>(row, TABLE, "environment_id")?,
        actor: Actor {
            kind: row::name::<ActorKind>(row, TABLE, "actor_kind")?,
            name: row::scalar::<ActorName>(row, TABLE, "actor_name")?,
        },
        pid: row::number_opt::<u32>(row, TABLE, "pid")?,
        started_at: row::stamp(row, TABLE, "started_at")?,
        ended_at: row::stamp_opt(row, TABLE, "ended_at")?,
    })
}
