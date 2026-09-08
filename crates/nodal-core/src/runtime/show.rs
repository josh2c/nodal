//! One unit in full: what `nodal show` answers with.
//!
//! The list answers "which unit needs me next"; this answers "what is this one". It is
//! the same reading as a row of the list, with the unit's log under it, because a unit
//! on its own is a thing with a history and the list has no room for one.
//!
//! The reading is the list's, not a second one. A row of `nodal ls` already joins the
//! registry with what Git says about the home and with who is attached to it, and a
//! second way of computing the same row is a second row that can disagree with the
//! first.

use rusqlite::Connection;

use crate::model::{Project, Timestamp, Unit};
use crate::output::view::UnitDetail;
use crate::runtime::ls;
use crate::runtime::processes::Processes;
use crate::store::events;
use crate::{Error, Result};

/// How many events of the unit's log the answer carries, newest last.
const HISTORY: u32 = 20;

/// Everything known about one unit.
///
/// # Errors
/// [`Error::UnitNotFound`] when the project has no such unit, and whatever the registry
/// or Git reported.
pub fn detail(
    conn: &Connection,
    processes: &dyn Processes,
    project: &Project,
    unit: &Unit,
    now: Timestamp,
) -> Result<UnitDetail> {
    let listed = ls::list(conn, processes, project, now)?;
    let row = listed
        .units
        .into_iter()
        .find(|row| row.slug == unit.slug)
        .ok_or_else(|| Error::UnitNotFound { slug: unit.slug.to_string() })?;
    let mut history = events::list_recent(conn, unit.id, HISTORY)?;
    history.reverse();
    Ok(UnitDetail { now, unit: row, history })
}
