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
use crate::git::{refs, snapshot};
use crate::lifecycle::journal;
use crate::model::{EnvState, OperationId, Unit};
use crate::output::view::{Disk, Snapshot, Taker, UnitDetail, UnitList, UnitRow};
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
    let snapshots = snapshots(conn, &row, unit);
    Ok(UnitDetail { now, unit: row, snapshots, history })
}

/// What Nodal recorded of the home before it changed it, oldest first.
///
/// One `for-each-ref` over the unit's own namespace in its own home. A home that is not
/// there, or that Git cannot answer for, has no snapshots to list and is not a failure:
/// this is one section of a report about a unit, and a unit whose home was reclaimed is
/// still a unit to show.
///
/// The kind of each operation comes from the journal row the ref is named after, so the
/// report says "before merge" rather than an identifier. A row that is no longer there
/// leaves the kind unsaid rather than guessed.
fn snapshots(conn: &Connection, row: &UnitRow, unit: &Unit) -> Vec<Snapshot> {
    let Some(environment) = row.environment.as_ref() else { return Vec::new() };
    let Ok(taken) = snapshot::list(&environment.home, &unit.id.to_string()) else {
        return Vec::new();
    };
    taken
        .into_iter()
        .map(|one| Snapshot {
            taken_by: taker(conn, &one.reference, unit),
            reference: one.reference,
            commit: one.commit.as_str().to_owned(),
            taken_at: one.taken_at,
        })
        .collect()
}

/// What wrote one ref of a unit's namespace.
fn taker(conn: &Connection, reference: &str, unit: &Unit) -> Taker {
    let namespace = format!("{}{}/", refs::NAMESPACE, unit.id);
    let Some(rest) = reference.strip_prefix(&namespace) else { return Taker::Other };
    if rest == "wip" {
        return Taker::WorkInProgress;
    }
    if rest == "premerge" {
        return Taker::PreMerge;
    }
    let Some(operation) = rest.strip_prefix(refs::PRE) else { return Taker::Other };
    let run =
        OperationId::parse(operation).ok().and_then(|id| journal::get(conn, id).ok().flatten());
    let outcome = run
        .as_ref()
        .and_then(|run| serde_json::to_value(run.state).ok())
        .and_then(|state| state.as_str().map(ToOwned::to_owned));
    Taker::Operation { operation: operation.to_owned(), op: run.map(|run| run.kind), outcome }
}

/// Walk the unit's home and record what it holds.
///
/// A home that is not on the disk is not walked: a reclaimed materialisation has no
/// directory to ask, and the row already says so. Nothing is opened and nothing is
/// written ([`size`]).
fn measure(row: &mut UnitRow) {
    let Some(environment) = row.environment.as_mut() else { return };
    if environment.state == EnvState::Absent {
        return;
    }
    environment.disk = Disk::measured(size::measure(&environment.home).reading());
}
