//! One writer per home: taking the write on a unit, refreshing it, and handing it over.
//!
//! The registry knows who holds a unit; this module is the policy over that row. It
//! answers one question for every verb that enters a home — may this actor write here,
//! and what does the person get told if not.
//!
//! **Why a row and not the process table.** Attribution reads `/proc`, and `/proc` does
//! not cross Linux accounts: two engineers logged in to one box each see their own
//! processes and none of the other's. A host that two people share is exactly the host
//! where "is anybody else in this unit" has to be answered, so the answer is written
//! down rather than observed. A lock row is that record.
//!
//! **The hold is advisory.** Nodal refuses its own write verbs to a second actor and
//! stops nothing else. An editor opens in the home, `git` runs in it, and a process
//! starts there, exactly as before. What the lock prevents is two writers driving one
//! home through Nodal's own operations at once, which is the case that loses work.
//!
//! **The read verbs never take it.** `ls`, `show`, `ps`, `explain` and `env --export`
//! answer in a locked home and take nothing. The prompt hook runs `env --export`, so a
//! second actor's shell still carries the unit's variables and ports: refusing those
//! would take the environment away from somebody the lock is not there to stop.
//!
//! **Two clocks.** A hold lapses when its absolute expiry passes, which is what a
//! transfer bundle from another host carries, or when nobody has entered the home for
//! the project's idle window (`[lock] idle_hours`, eight by default). The idle window is
//! measured from the last entry, so a person working all day keeps the hold and a home
//! nobody has touched since yesterday belongs to whoever asks next.
//!
//! **The pid is a record, not a signal.** The process that took the hold is written down
//! so a person can look it up. Nothing here signals it and no hold is released because
//! the process is gone: a lock names an actor, and an actor outlives any one shell.

use std::path::Path;

use rusqlite::Connection;

use crate::model::{Actor, EventKind, HostName, Lock, Project, Recipe, Timestamp, Unit, UnitId};
use crate::store::{events, locks};
use crate::{Error, Result};

/// How far ahead a fresh hold sets its absolute expiry, as a multiple of the idle
/// window.
///
/// The absolute clock is the transfer bundle's, not this host's, so on this host it has
/// to stay out of the idle window's way: a hold that expired absolutely while somebody
/// was still working would make the idle window a lie. Two windows is far enough to do
/// that and near enough that a registry copied to another machine does not carry a claim
/// good for a year.
const ABSOLUTE_WINDOWS: u32 = 2;

/// What entering a home did to its lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entered {
    /// The lock was free, or had lapsed, and this actor now holds it.
    Took,
    /// This actor already held it, and the idle window starts again.
    Refreshed,
    /// Another actor held it and `--take` moved it here.
    TakenOver,
}

/// Take or refresh the write on `unit` for the actor running now.
///
/// `take` is `--take`: it moves a hold another actor still has, and records the hand-off
/// on both units' log. Without it, a home another actor holds is refused.
///
/// The recipe is read from the project's own root, so the idle window is the project's
/// setting. A project whose recipe cannot be read gets the default window rather than a
/// failure: a lock is not the place a broken `nodal.toml` first shows up.
///
/// # Errors
/// [`Error::UnitLocked`] when another actor holds the unit and `take` is false,
/// [`Error::Store`] when the registry could not be read or written, and
/// [`Error::Actor`] when this process cannot say who it is.
pub fn enter(
    conn: &Connection,
    unit: &Unit,
    project: &Project,
    take: bool,
    now: Timestamp,
) -> Result<Entered> {
    let actor = crate::runtime::actor::current()?;
    let host = crate::lifecycle::owner::current_host();
    let idle_hours = idle_hours(&project.root);
    let held = locks::get(conn, unit.id)?;
    let mine = Lock {
        unit_id: unit.id,
        host: host.clone(),
        actor: Some(actor.clone()),
        pid: Some(std::process::id()),
        taken_at: now,
        refreshed_at: now,
        expires_at: absolute_expiry(now, idle_hours),
    };

    let holder = held.filter(|held| held.holds_anyone(now, idle_hours));
    let Some(holder) = holder else {
        // Free, or lapsed. The statement takes it over in one write, so two processes
        // racing for a lapsed hold make one holder rather than two.
        let idle_deadline = mine.idle_deadline(idle_hours);
        if locks::take(conn, &mine, now, idle_deadline)? {
            return Ok(Entered::Took);
        }
        // Somebody won the race between the read and the write. Read again and refuse
        // with the holder they actually are, rather than reporting the row that lapsed.
        let won = locks::get(conn, unit.id)?;
        return match won {
            Some(won) if won.is_held_by(&actor, &host) => Ok(Entered::Refreshed),
            Some(won) => Err(refusal(unit, &won, now)),
            None => Err(Error::StoreMissingRow { table: "lock", id: unit.id.to_string() }),
        };
    };

    if holder.is_held_by(&actor, &host) {
        locks::take(conn, &mine, now, holder.idle_deadline(idle_hours))?;
        return Ok(Entered::Refreshed);
    }
    if !take {
        return Err(refusal(unit, &holder, now));
    }
    locks::hand_over(conn, &mine)?;
    record_hand_off(conn, unit.id, &holder, &actor, &host)?;
    Ok(Entered::TakenOver)
}

/// Record this actor as the writer of a unit whose row is being written right now.
///
/// `new` and `adopt` call this from inside their own registry write, so a unit is held
/// by whoever made it from the instant it exists. There is no prior row to lose a race
/// with: the unit identifier is drawn by the operation and nothing else has seen it.
///
/// The idle window comes from the recipe the operation already loaded, rather than being
/// read again from disk, so the hold and the home are made from one reading.
///
/// # Errors
/// [`Error::Store`] when the row could not be written, and [`Error::Actor`] when this
/// process cannot say who it is.
pub fn open(conn: &Connection, unit: UnitId, idle_hours: u32, now: Timestamp) -> Result<()> {
    let mine = Lock {
        unit_id: unit,
        host: crate::lifecycle::owner::current_host(),
        actor: Some(crate::runtime::actor::current()?),
        pid: Some(std::process::id()),
        taken_at: now,
        refreshed_at: now,
        expires_at: absolute_expiry(now, idle_hours),
    };
    locks::hand_over(conn, &mine)
}

/// Take or refresh the write on the home at `home`, refusing a second actor.
///
/// This is what the write verbs call: `cd`, `shell`, `run`, `new` and `adopt`. A
/// directory the registry holds no unit for answers `None` and is entered as before,
/// because there is no unit for a hold to be about.
///
/// # Errors
/// As [`enter`].
pub fn claim(
    conn: &Connection,
    home: &Path,
    take: bool,
    now: Timestamp,
) -> Result<Option<Entered>> {
    let Some((unit, project)) = crate::runtime::entry::registered(conn, home)? else {
        return Ok(None);
    };
    enter(conn, &unit, &project, take, now).map(Some)
}

/// Refresh the write on the home at `home`, and refuse nobody.
///
/// This is what the prompt hook calls, through `nodal env --export`. It keeps a holder's
/// idle window open on every entry into the home and takes a free lock, but a home
/// another actor holds is not an error here: the hook exports the unit's variables and
/// ports for whoever is standing in the directory, and taking those away from a second
/// actor would stop somebody the lock exists to inform rather than to block.
///
/// Answers what it did, and `None` when the home is not a registered unit or is held by
/// somebody else.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read or written, and [`Error::Actor`]
/// when this process cannot say who it is.
pub fn touch(conn: &Connection, home: &Path, now: Timestamp) -> Result<Option<Entered>> {
    match claim(conn, home, false, now) {
        Ok(entered) => Ok(entered),
        Err(Error::UnitLocked { .. }) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Give up the write on `unit`, whoever on this host holds it.
///
/// A reclaim calls this: the home has gone, so a row naming a writer for it would name
/// a writer of nothing. It is not an error for there to be no row.
///
/// # Errors
/// [`Error::Store`] on a failed statement.
pub fn release(conn: &Connection, unit: UnitId) -> Result<bool> {
    locks::release(conn, unit, &crate::lifecycle::owner::current_host())
}

/// Who holds `unit`, once the clock has been applied: `None` when the hold has lapsed.
///
/// This is what a report asks. A lapsed row is still in the table until somebody enters
/// the home, and printing it as a holder would name somebody who has gone.
///
/// # Errors
/// [`Error::Store`] on a failed statement.
pub fn holder(conn: &Connection, unit: &Unit, root: &Path, now: Timestamp) -> Result<Option<Lock>> {
    let idle_hours = idle_hours(root);
    Ok(locks::get(conn, unit.id)?.filter(|held| held.holds_anyone(now, idle_hours)))
}

/// Every lock the registry holds that has not lapsed, for a project whose root is
/// `root`.
///
/// The list reads this once for the whole project rather than a query per unit, because
/// a list of eight units would otherwise be eight statements to answer one column.
///
/// # Errors
/// [`Error::Store`] on a failed statement.
pub fn live(conn: &Connection, root: &Path, now: Timestamp) -> Result<Vec<Lock>> {
    let idle_hours = idle_hours(root);
    Ok(locks::list_all(conn)?
        .into_iter()
        .filter(|held| held.holds_anyone(now, idle_hours))
        .collect())
}

/// The idle window a project's recipe asks for, or the default when it does not say.
///
/// A recipe that cannot be read yields the default. The window is a convenience and a
/// missing `nodal.toml` is reported by the commands whose work needs one.
///
/// # Panics
/// Never.
#[must_use]
pub fn idle_hours(root: &Path) -> u32 {
    crate::recipe::load(root)
        .map_or_else(|_| Recipe::default(), |effective| effective.recipe)
        .lock_idle_hours()
}

/// How far ahead a fresh hold's absolute expiry sits.
fn absolute_expiry(now: Timestamp, idle_hours: u32) -> Timestamp {
    let ahead = i64::from(idle_hours.saturating_mul(ABSOLUTE_WINDOWS)).saturating_mul(3_600);
    Timestamp::from_unix_seconds(now.unix_seconds().saturating_add(ahead)).unwrap_or(now)
}

/// The refusal a second actor gets, naming who holds the unit and how to take it.
///
/// It says a name, an instant and a way out, because a refusal without its reason is a
/// refusal a person cannot act on. The holder always has a name here: a row that records
/// no actor holds nobody ([`Lock::holds_anyone`]) and never reaches this.
fn refusal(unit: &Unit, held: &Lock, now: Timestamp) -> Error {
    Error::UnitLocked {
        slug: unit.slug.to_string(),
        actor: held.actor.as_ref().map_or_else(String::new, |actor| actor.name.to_string()),
        host: held.host.to_string(),
        since: crate::output::human::span(now, held.taken_at),
    }
}

/// Write the hand-off on the unit's log, so that the move is in the record a person
/// reads rather than only in the row it changed.
///
/// Only a hold somebody has reaches here, so the name is always there: a row with no
/// actor is entered without `--take` and hands nothing off.
fn record_hand_off(
    conn: &Connection,
    unit: UnitId,
    from: &Lock,
    to: &Actor,
    host: &HostName,
) -> Result<()> {
    let was = from.actor.as_ref().map_or_else(String::new, |actor| actor.name.to_string());
    events::note(
        conn,
        (unit, None),
        EventKind::Handoff,
        format!("write lock taken from {was} by {}", to.name),
        &[("from", was.clone()), ("to", to.name.to_string()), ("host", host.to_string())],
    )
}
