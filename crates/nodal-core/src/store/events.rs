//! Rows of the `event` table: what happened in a unit.
//!
//! Appending is the hot path and the one several processes do at once — a shell hook, an
//! agent's note, a command wrapper — so it is a single insert with no read before it.
//! Nothing here writes the unit's `events.jsonl`; dual-writing the portable record is
//! the capture step's job, and keeping it out of the store is what lets an append be one
//! statement.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{
    Actor, ActorKind, ActorName, EnvId, Epistemic, Event, EventId, EventKind, RawRef, RefName,
    UnitId,
};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "event";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, \
     body, refs, raw_ref";

/// Append one event.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when the identifier is already in the log,
/// [`crate::Error::Store`] on any other failure.
pub fn append(conn: &Connection, event: &Event) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, \
         epistemic, body, refs, raw_ref) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            event.id.to_string(),
            event.unit.to_string(),
            event.environment.map(|environment| environment.to_string()),
            event.ts.unix_seconds(),
            row::name_of(&event.actor.kind, "actor kind")?,
            event.actor.name.as_str(),
            row::name_of(&event.kind, "event kind")?,
            row::name_of(&event.epistemic, "epistemic tier")?,
            event.body.as_str(),
            row::json_of(&event.refs, "event references")?,
            event.raw_ref.as_ref().map(RawRef::as_str),
        ],
    )?;
    Ok(())
}

/// One event by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: EventId) -> Result<Option<Event>> {
    let sql = format!("SELECT {COLUMNS} FROM event WHERE id = ?");
    row::one(conn, &sql, params![id.to_string()], decode)
}

/// A unit's log in order. Identifiers are ULIDs, so ordering by them is ordering by
/// creation, which is what makes the log readable without trusting clocks.
///
/// # Errors
/// As [`get`].
pub fn list_for_unit(conn: &Connection, unit: UnitId) -> Result<Vec<Event>> {
    let sql = format!("SELECT {COLUMNS} FROM event WHERE unit_id = ? ORDER BY id");
    row::many(conn, &sql, params![unit.to_string()], decode)
}

/// The events of a unit after `after`, in order: what a context compile reads to catch
/// up without re-reading the whole log.
///
/// # Errors
/// As [`get`].
pub fn list_since(conn: &Connection, unit: UnitId, after: EventId) -> Result<Vec<Event>> {
    let sql = format!("SELECT {COLUMNS} FROM event WHERE unit_id = ? AND id > ? ORDER BY id");
    row::many(conn, &sql, params![unit.to_string(), after.to_string()], decode)
}

/// The newest events of a unit, newest first.
///
/// # Errors
/// As [`get`].
pub fn list_recent(conn: &Connection, unit: UnitId, limit: u32) -> Result<Vec<Event>> {
    let sql = format!("SELECT {COLUMNS} FROM event WHERE unit_id = ? ORDER BY id DESC LIMIT ?");
    row::many(conn, &sql, params![unit.to_string(), limit], decode)
}

/// How many events a unit has.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn count_for_unit(conn: &Connection, unit: UnitId) -> Result<u64> {
    let counted = row::one(
        conn,
        "SELECT COUNT(*) AS n FROM event WHERE unit_id = ?",
        params![unit.to_string()],
        |row| row::number::<u64>(row, TABLE, "n"),
    )?;
    Ok(counted.unwrap_or_default())
}

/// Turn a row into an event.
fn decode(row: &Row<'_>) -> Result<Event> {
    Ok(Event {
        id: row::scalar::<EventId>(row, TABLE, "id")?,
        unit: row::scalar::<UnitId>(row, TABLE, "unit_id")?,
        environment: row::scalar_opt::<EnvId>(row, TABLE, "environment_id")?,
        ts: row::stamp(row, TABLE, "ts")?,
        actor: Actor {
            kind: row::name::<ActorKind>(row, TABLE, "actor_kind")?,
            name: row::scalar::<ActorName>(row, TABLE, "actor_name")?,
        },
        kind: row::name::<EventKind>(row, TABLE, "kind")?,
        epistemic: row::name::<Epistemic>(row, TABLE, "epistemic")?,
        body: row::plain::<String>(row, TABLE, "body")?,
        refs: row::json::<std::collections::BTreeMap<RefName, String>>(row, TABLE, "refs")?,
        raw_ref: row::scalar_opt::<RawRef>(row, TABLE, "raw_ref")?,
    })
}
