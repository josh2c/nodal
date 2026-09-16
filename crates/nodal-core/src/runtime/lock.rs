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
//! **Nothing is signalled; the record is read.** The process that took the hold and the
//! session it was in are written down. Nothing here signals either of them, and no hold
//! is released by killing anything. What changed is what the record says: a hold is an
//! actor and a lineage, not an actor alone.
//!
//! **An actor is not a writer.** A fleet of agents all report as `claude-code`, so the
//! name matched itself and the lock refused nobody: a second agent entered a home the
//! first one held and said nothing. A second process of one actor is a second holder,
//! and it is refused the way another actor is, with `--take` as the way through.
//!
//! **The lineage is the session, not the process group.** Every write verb runs in a new
//! process — `run.rs`, `shell.rs` and `cd.rs` each call [`claim`] from one — so the
//! recorded pid cannot be what a re-entry matches: the first `nodal run` has exited
//! before the second one starts. A process group cannot be it either. A shell with job
//! control puts every foreground command in a group of its own, so two `nodal run`
//! commands typed one after the other are two groups, and a rule keyed on the group
//! would refuse the engineer their own next command. Both commands, and the shell that
//! started them, are in one POSIX session. That is what is recorded
//! ([`crate::model::Lock::session`]) and what a re-entry is measured against.
//!
//! **A lineage that has gone has let the hold go.** Where the recorded session holds no
//! process, the hold has lapsed for re-entry: the next actor takes it, the take is
//! recorded as a hand-off, and the log says the lineage had gone. Where it still holds
//! one, a second lineage is refused. `nodal run --tether` therefore keeps working: the
//! tether stays live, and the shell that started it is in the session that took the
//! hold, so its next command is the same holder.
//!
//! **A reading that could not be taken refuses nobody.** A host that publishes no
//! process table cannot say whether a recorded session is still there, and "I cannot
//! see" is not "it is gone". macOS publishes no `/proc`, so a same-actor re-entry there
//! falls back to the older behaviour — the name alone — and the contract says so. A row
//! that records no session, written before locks carried a lineage, is read the same
//! way.
//!
//! **A record is read before it is printed.** The row outlives the process, so a report
//! that printed the row alone said `claude-code holds 8 h` about a session that was
//! killed hours earlier, and a fresh session had nothing to tell it otherwise.
//! [`liveness`] asks this host's process table whether the recorded *process* is still
//! there, and that answer is still a word in the report and never a refusal: a process
//! identifier is reused, and a hold that let go on a reading of one would be a hold that
//! let go of the wrong home. The refusal reads the recorded *session* instead, which is
//! a different question with a different failure: a session identifier that came round
//! again names a session, and the worst it does is keep a lapsed hold held, which is the
//! conservative direction.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;

use crate::model::{Actor, EventKind, HostName, Lock, Project, Recipe, Timestamp, Unit, UnitId};
use crate::output::view::{HolderState, Unknowable};
use crate::runtime::processes::{Presence, Processes};
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
    let entry = Entry {
        actor: crate::runtime::actor::current()?,
        host: HostName::current(),
        session: crate::runtime::processes::current_session(),
        take,
        now,
        idle_hours: idle_hours(&project.root),
    };
    let held = locks::get(conn, unit.id)?;
    let holder = held.filter(|held| held.holds_anyone(now, entry.idle_hours));
    let Some(holder) = holder else {
        // Free, or lapsed. The statement takes it over in one write, so two processes
        // racing for a lapsed hold make one holder rather than two. The lineage is part
        // of that statement's match, so two processes of one actor race like strangers.
        let mine = entry.claim(unit.id);
        if locks::take(conn, &mine, now, mine.idle_deadline(entry.idle_hours))? {
            return Ok(Entered::Took);
        }
        // Somebody won the race between the read and the write. Read again and answer
        // for the holder they actually are, rather than reporting the row that lapsed.
        let Some(won) = locks::get(conn, unit.id)? else {
            return Err(Error::StoreMissingRow { table: "lock", id: unit.id.to_string() });
        };
        return settle(conn, unit, &won, &entry);
    };
    settle(conn, unit, &holder, &entry)
}

/// Who is entering a home, and how: one reading of this process, taken once.
///
/// It is a value rather than six arguments because every one of them is read from this
/// process at the same instant and they are only ever used together. [`Entry::claim`] is
/// the lock this entry would write, which is the one place the fields are spelled out.
struct Entry {
    /// Who is entering.
    actor: Actor,
    /// The host they are entering from.
    host: HostName,
    /// The POSIX session this process is in, `None` where the host will not say.
    session: Option<u32>,
    /// Whether `--take` was given.
    take: bool,
    /// The instant of the entry.
    now: Timestamp,
    /// The project's idle window.
    idle_hours: u32,
}

impl Entry {
    /// The hold this entry would write on `unit`.
    fn claim(&self, unit: UnitId) -> Lock {
        Lock {
            unit_id: unit,
            host: self.host.clone(),
            actor: Some(self.actor.clone()),
            pid: Some(std::process::id()),
            session: self.session,
            taken_at: self.now,
            refreshed_at: self.now,
            expires_at: absolute_expiry(self.now, self.idle_hours),
        }
    }
}

/// What the record says about the lineage a re-entry comes from.
///
/// Four answers and not two, because "the record does not say" and "the hold belongs to
/// somebody still at work" are different, and only one of them refuses anybody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lineage {
    /// The hold was taken from this session. The same worker, entering again.
    Same,
    /// Another session of the same actor, and it is still there: a second holder.
    Second,
    /// The recorded session holds no process. The hold has lapsed for re-entry.
    Gone,
    /// Nothing states a lineage, or this host publishes no process table.
    Unreadable,
}

/// Which of the four a holder and this process make.
///
/// A row with no recorded session, and a process that cannot say which session it is in,
/// are both `Unreadable`: the rule [`crate::model::Lock::holds_anyone`] states for an
/// actor holds for a lineage, and a record that states nothing refuses nobody.
fn lineage(holder: &Lock, here: Option<u32>) -> Lineage {
    if holder.was_taken_from(here) {
        return Lineage::Same;
    }
    let (Some(recorded), Some(_)) = (holder.session, here) else {
        return Lineage::Unreadable;
    };
    match crate::runtime::processes::session_is_live(recorded) {
        None => Lineage::Unreadable,
        Some(true) => Lineage::Second,
        Some(false) => Lineage::Gone,
    }
}

/// Answer one entry against the holder the registry actually has.
///
/// Both questions are asked here and in this order: is this the actor who holds it, and
/// is this the lineage the hold was taken from. An actor who matches on the name alone
/// gets no further than a stranger does.
fn settle(conn: &Connection, unit: &Unit, holder: &Lock, entry: &Entry) -> Result<Entered> {
    if holder.is_held_by(&entry.actor, &entry.host) {
        match lineage(holder, entry.session) {
            Lineage::Same | Lineage::Unreadable => {
                // The recorded lineage comes first, because a reading that could not be
                // taken states nothing and must not overwrite what the row already says.
                // Writing this process's own session here instead made the claim differ
                // from the row, which is what [`locks::take`] matches on: the statement
                // touched nothing and the entry reported a refresh that never happened.
                // A row that records no lineage is upgraded by the first entry that can
                // say what its own is.
                let mine = Lock {
                    session: holder.session.or(entry.session),
                    ..entry.claim(holder.unit_id)
                };
                if locks::take(conn, &mine, entry.now, holder.idle_deadline(entry.idle_hours))? {
                    return Ok(Entered::Refreshed);
                }
                // The statement matched nothing, so the row is no longer the one just
                // read. A refresh that wrote nothing is not a refresh, and reporting one
                // would tell a person their idle window moved when it did not.
                return contested(conn, unit, entry);
            }
            Lineage::Gone => return take_over(conn, unit, holder, entry, Lineage::Gone),
            Lineage::Second => {}
        }
    }
    if !entry.take {
        return Err(refusal(unit, holder, entry));
    }
    take_over(conn, unit, holder, entry, Lineage::Second)
}

/// Answer an entry whose write found the row already changed.
///
/// The row is read once more and this entry is refused, or moved by `--take`. It is not
/// settled again: the reading that would decide is the one that has just lost a race,
/// and a second attempt could lose the same race again. "Somebody else got there first"
/// is a refusal a person can act on, which is the right answer and a terminating one.
fn contested(conn: &Connection, unit: &Unit, entry: &Entry) -> Result<Entered> {
    let Some(won) = locks::get(conn, unit.id)? else {
        return Err(Error::StoreMissingRow { table: "lock", id: unit.id.to_string() });
    };
    if !entry.take {
        return Err(refusal(unit, &won, entry));
    }
    take_over(conn, unit, &won, entry, Lineage::Second)
}

/// Move the hold here and write the move on the unit's log.
///
/// `why` is the reading [`settle`] already took, passed rather than taken again: the
/// answer is what the note has to say, and reading `/proc` a second time to say it would
/// pay for the same walk twice and could answer differently from the decision it
/// describes.
fn take_over(
    conn: &Connection,
    unit: &Unit,
    from: &Lock,
    entry: &Entry,
    why: Lineage,
) -> Result<Entered> {
    locks::hand_over(conn, &entry.claim(unit.id))?;
    record_hand_off(conn, unit.id, from, entry, why)?;
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
        host: HostName::current(),
        actor: Some(crate::runtime::actor::current()?),
        pid: Some(std::process::id()),
        session: crate::runtime::processes::current_session(),
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
    locks::release(conn, unit, &HostName::current())
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

/// What became of the process that took a hold, read and never signalled.
///
/// Three things make the answer unknown rather than gone, and each of them is a reading
/// that could not be taken: a hold from another machine, whose process identifiers mean
/// nothing here; a row that records no process; and a host that publishes no process
/// table this account can read. A report says which, because "I cannot see" and "nobody
/// is there" are different answers and only one of them is news.
#[must_use]
pub fn liveness(lock: &Lock, here: &HostName, seen: &Seen) -> HolderState {
    if &lock.host != here {
        return HolderState::Unknown { why: Unknowable::AnotherHost };
    }
    let Some(pid) = lock.pid else { return HolderState::Unknown { why: Unknowable::NoPid } };
    match seen.of(pid) {
        None => HolderState::Unknown { why: Unknowable::NoProcessTable },
        Some(Presence::Gone) => HolderState::Gone,
        // An identifier that came round again. The process wearing it now started after
        // the hold was taken, so it is not the process that took it, and reporting it as
        // the holder would put a stranger's shell in the WHO column.
        Some(Presence::Running { started_at: Some(started) }) if started > lock.taken_at => {
            HolderState::Gone
        }
        Some(Presence::Running { .. }) => HolderState::Live,
    }
}

/// What one reading of the process table said about the identifiers a caller asked for.
///
/// Taken once for a whole list ([`read`]) and asked of each lock, because the reading is
/// of one machine and a list of eight units must not read it eight times. A host that
/// could not be read says so once, for every identifier, rather than answering `gone`.
#[derive(Debug, Default)]
pub struct Seen(Option<BTreeMap<u32, Presence>>);

impl Seen {
    /// Read this host's table for these identifiers.
    #[must_use]
    pub fn read(processes: &dyn Processes, pids: &[u32]) -> Self {
        Self(processes.presences(pids).ok())
    }

    /// What it said about one identifier, `None` where the table could not be read.
    #[must_use]
    fn of(&self, pid: u32) -> Option<Presence> {
        self.0.as_ref().map(|seen| seen.get(&pid).copied().unwrap_or(Presence::Gone))
    }
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
fn refusal(unit: &Unit, held: &Lock, entry: &Entry) -> Error {
    Error::UnitLocked {
        slug: unit.slug.to_string().into_boxed_str(),
        actor: held
            .actor
            .as_ref()
            .map_or_else(String::new, |actor| actor.name.to_string())
            .into_boxed_str(),
        host: held.host.to_string().into_boxed_str(),
        since: crate::output::human::span(entry.now, held.taken_at).into_boxed_str(),
        hold: hold_line(held, entry).into_boxed_str(),
    }
}

/// What the refusal adds about the process that took the hold, or nothing.
///
/// A refusal names the holder, and the holder may be a session that ended hours ago.
/// Saying so is the difference between a person who waits and a person who types
/// `--take` at once, so the reading [`liveness`] takes for a report is taken here too.
/// Only [`HolderState::Gone`] is said: a live hold needs no sentence, and a reading that
/// could not be taken contradicts nothing the row claims.
///
/// The sentence says what this reading proves and no more. Whether anything of that
/// actor is still in the home is the second reading a report takes
/// ([`crate::runtime::ls`]), and it costs a scan of the process table against every home
/// of the project, which is not what a refusal should pay for. So the refusal names the
/// reading it made and names the command that makes the other one.
fn hold_line(held: &Lock, entry: &Entry) -> String {
    if held.is_held_by(&entry.actor, &entry.host) {
        // The name on both sides is one name, so the refusal reads as "you hold it"
        // unless it says which of the two processes of that name the hold belongs to.
        let named = held.session.map_or_else(String::new, |sid| format!(" session {sid}"));
        return format!(
            " That is a second process of the same name: the hold belongs to{named}, \
             and this command is in another one."
        );
    }
    let seen =
        Seen::read(&crate::runtime::processes::Live, &held.pid.into_iter().collect::<Vec<_>>());
    match liveness(held, &HostName::current(), &seen) {
        HolderState::Gone => {
            let named =
                held.pid.map_or_else(|| String::from("the process"), |pid| format!("pid {pid}"));
            format!(
                " {named} that took it is gone from this host; nodal show says whether \
                 anything of that actor is still in the home."
            )
        }
        HolderState::Live | HolderState::Unknown { .. } => String::new(),
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
    entry: &Entry,
    why: Lineage,
) -> Result<()> {
    let was = from.actor.as_ref().map_or_else(String::new, |actor| actor.name.to_string());
    let to = &entry.actor;
    // Why it moved, because the two reasons read the same in the log and are not the
    // same event: one person asked for the hold, and the other found it let go.
    let why = match why {
        Lineage::Gone => "the previous holder was gone",
        Lineage::Same | Lineage::Second | Lineage::Unreadable => "asked for",
    };
    events::note(
        conn,
        (unit, None),
        EventKind::Handoff,
        format!("write lock taken from {was} by {}: {why}", to.name),
        &[
            ("from", was.clone()),
            ("to", to.name.to_string()),
            ("host", entry.host.to_string()),
            ("why", why.to_string()),
        ],
    )
}
