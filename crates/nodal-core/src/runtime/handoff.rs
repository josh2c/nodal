//! Stating a handoff: what one actor leaves for whoever continues a unit.
//!
//! Every other event Nodal keeps is one it watched: a command that ran, a commit that
//! was made, a session that attached. This is the one a person or an agent says, and it
//! is the one the next session reads first, because the unit's memory keeps stated notes
//! apart from what Nodal observed (`docs/contracts.md`, The unit's memory).
//!
//! It records and nothing else. No lock is taken, no home is entered and no file in the
//! home is written: the memory is compiled from the registry by the commands that read
//! a unit, so a handoff is in the next `WORKUNIT.md` without this writing one.
//!
//! The actor is read the way every other event reads it ([`crate::runtime::actor`]) —
//! an agent by its own variable, a person by the account the process runs as — so a
//! handoff an agent states is attributed to the agent and not to whoever started it.

use rusqlite::Connection;

use crate::model::{Environment, Epistemic, Event, EventId, EventKind, Timestamp, Unit};
use crate::output::view::EventLog;
use crate::runtime::actor;
use crate::store::{environments, events};
use crate::{Error, Result};

/// Record a handoff on a unit and answer with the event that was written.
///
/// The answer is the event log of one event, which is the value `nodal handoff` prints
/// and the value its `--json` carries: a caller that states a handoff gets back what was
/// recorded rather than a word saying it was.
///
/// # Errors
/// [`Error::Empty`] when the body says nothing, and [`Error::Store`] when the event
/// could not be written.
pub fn state(conn: &Connection, unit: &Unit, body: &str) -> Result<EventLog> {
    let body = body.trim();
    if body.is_empty() {
        return Err(Error::EmptyHandoff { slug: unit.slug.to_string() });
    }
    let now = Timestamp::now();
    let event = Event {
        id: EventId::from_ulid(ulid::Ulid::new()),
        unit: unit.id,
        environment: newest(conn, unit)?.map(|environment| environment.id),
        ts: now,
        actor: actor::current()?,
        kind: EventKind::Handoff,
        epistemic: Epistemic::Stated,
        body: body.to_owned(),
        refs: std::collections::BTreeMap::new(),
        raw_ref: None,
    };
    events::append(conn, &event)?;
    Ok(EventLog { now, unit: Some(unit.slug.clone()), events: vec![event] })
}

/// The unit's newest materialisation, when it has one.
///
/// A handoff belongs to the unit rather than to a home, so a unit whose home was
/// reclaimed still takes one; the home is recorded where there is one, because that is
/// what every other event of the unit carries.
fn newest(conn: &Connection, unit: &Unit) -> Result<Option<Environment>> {
    Ok(environments::list_for_unit(conn, unit.id)?.pop())
}
