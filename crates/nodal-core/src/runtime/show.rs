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
//! it adds is the log.

use rusqlite::Connection;

use crate::model::Unit;
use crate::output::view::{UnitDetail, UnitList};
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
    let row = listed
        .units
        .into_iter()
        .find(|row| row.slug == unit.slug)
        .ok_or_else(|| Error::UnitNotFound { slug: unit.slug.to_string() })?;
    let mut history = events::list_recent(conn, unit.id, HISTORY)?;
    history.reverse();
    Ok(UnitDetail { now, unit: row, history })
}
