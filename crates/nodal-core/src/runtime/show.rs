//! One unit in full: what `nodal show` answers with.
//!
//! The list answers "which unit needs me next"; this answers "what is this one". It is
//! the same reading as a row of the list, with the unit's log under it, because a unit
//! on its own is a thing with a history and the list has no room for one.
//!
//! The reading is the list's, not a second one. A row of `nodal ls` already joins the
//! registry with what Git says about the home and with who is attached to it, and a
//! second way of computing the same row is a second row that can disagree with the
//! first. So the list is what this is given, already read and already settled, and all
//! it adds is the log and one measurement.
//!
//! **The measurement is the one thing a detail pays for and a list does not.** What a
//! home occupies is a walk of it, and the list is the command with a startup budget. A
//! detail is about one unit and a person asking about one unit is asking what it costs,
//! so the walk happens here ([`crate::doctor::size`]) and the figure is the same
//! [`Bytes`](crate::doctor::size::Bytes) a reclaim preflight reports. A row a list
//! printed says it was not measured and why; it never prints an empty column that reads
//! as nothing.

use rusqlite::Connection;

use crate::doctor::size;
use crate::model::{EnvState, Unit};
use crate::output::view::{Disk, UnitDetail, UnitList};
use crate::store::events;
use crate::{Error, Result};

/// How many events of the unit's log the answer carries, newest last.
const HISTORY: u32 = 20;

/// Everything known about one unit: its row of `listed`, and its log under it.
///
/// # Errors
/// [`Error::UnitNotFound`] when the list has no such unit, and [`Error::Store`] when the
/// log could not be read.
pub fn detail(conn: &Connection, listed: UnitList, unit: &Unit) -> Result<UnitDetail> {
    let now = listed.now;
    let mut row = listed
        .units
        .into_iter()
        .find(|row| row.slug == unit.slug)
        .ok_or_else(|| Error::UnitNotFound { slug: unit.slug.to_string() })?;
    measure(&mut row);
    let mut history = events::list_recent(conn, unit.id, HISTORY)?;
    history.reverse();
    Ok(UnitDetail { now, unit: row, history })
}

/// Walk the unit's home and record what it holds.
///
/// A home that is not on the disk is not walked: a reclaimed materialisation has no
/// directory to ask, and the row already says so. Nothing is opened and nothing is
/// written ([`size`]).
fn measure(row: &mut crate::output::view::UnitRow) {
    let Some(environment) = row.environment.as_mut() else { return };
    if environment.state == EnvState::Absent {
        return;
    }
    environment.disk = Disk::measured(size::measure(&environment.home).reading());
}
