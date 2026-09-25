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
//! **A lineage proven gone has let the hold go, and nothing less does.** Where a reading
//! of the whole table holds no process in the recorded session, the hold has lapsed for
//! re-entry: the next actor takes it, the take is recorded as a hand-off, and the log
//! says the holder was proven gone. Where the table still holds one, a second lineage is
//! refused. `nodal run --tether` therefore keeps working: the tether stays live, and the
//! shell that started it is in the session that took the hold, so its next command is
//! the same holder. A hold is moved from a previous holder on three grounds and no
//! others — proven death, an expired lease, and `--take` — and each writes its own line
//! ([`Moved`]).
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
//!
//! **The process is named by a pinned identity.** A number is not a process, so the row
//! records the identifier and the instant it started ([`crate::model::Holding`]), and a
//! reading resolves the pair or resolves nothing. What the pair replaced was a
//! comparison against `taken_at`, which belongs to the hold and not to the process: a
//! refresh keeps `taken_at` and writes the refreshing process's own identifier, so every
//! refreshed hold read as a number that had come round again and was reported gone while
//! it was running. A false "the holder is gone" is what tells the next actor a held home
//! is free, and it is the same false-safe family as the one the occupancy reading had.

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
    // Borrowed, not copied. The row is read once and used twice — to decide whether
    // anybody still holds it, and to name who the clock released it from — and a hold
    // is on the path every write verb takes.
    let Some(holder) = held.as_ref().filter(|held| held.holds_anyone(now, entry.idle_hours)) else {
        // Free, or lapsed. The statement takes it over in one write, so two processes
        // racing for a lapsed hold make one holder rather than two. The lineage is part
        // of that statement's match, so two processes of one actor race like strangers.
        let mine = entry.claim(unit.id);
        if locks::take(conn, &mine, now, mine.idle_deadline(entry.idle_hours))? {
            // A row the clock released still names who it was released from, and a hold
            // that moved with nothing written down is a hold a person cannot account
            // for. The note says the lease ran out, which is the other ground a hold
            // moves on and reads nothing like the one a reading establishes.
            //
            // A hold that came back to the holder it already had moved nowhere, and
            // [`record_hand_off`] writes nothing for it. The lapse is real — the row was
            // anybody's for the asking — but "taken from ada by ada" is not.
            if let Some(lapsed) = held.as_ref().filter(|held| held.actor.is_some()) {
                record_hand_off(conn, unit.id, lapsed, &entry, Moved::LeaseExpired)?;
            }
            return Ok(Entered::Took);
        }
        // Somebody won the race between the read and the write. Read again and answer
        // for the holder they actually are, rather than reporting the row that lapsed.
        let Some(won) = locks::get(conn, unit.id)? else {
            return Err(Error::StoreMissingRow { table: "lock", id: unit.id.to_string() });
        };
        return settle(conn, unit, &won, &entry);
    };
    settle(conn, unit, holder, &entry)
}

/// Who is entering a home, and how: one reading of this process, taken once.
///
/// It is a value rather than five arguments because every one of them is read from this
/// process at the same instant and they are only ever used together. [`Entry::claim`] is
/// the lock this entry would write, which is the one place the fields are spelled out.
///
/// The pinned process is not among them. It is the one field that costs a reading of the
/// process table, and the only thing that wants it is a row being written, so
/// [`Entry::claim`] reads it and nothing else does. Most entries write no row: a refusal
/// reads the table for nobody, and `nodal env --export` — which the prompt hook runs on
/// every entry into a home — is a refresh that the lineage may already have. Reading it
/// later is the same answer, because a process's identifier and the instant it started
/// do not change while it is running.
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
    ///
    /// The process is pinned here rather than in the entry, because this is the one
    /// place a pin is wanted: it is written into a row or it is not read at all.
    fn claim(&self, unit: UnitId) -> Lock {
        Lock {
            unit_id: unit,
            host: self.host.clone(),
            actor: Some(self.actor.clone()),
            process: Some(crate::runtime::processes::current_process()),
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
pub enum Lineage {
    /// The hold was taken from this session. The same worker, entering again.
    Same,
    /// Another session of the same actor, and it is still there: a second holder.
    Second,
    /// The whole table was read and no process is in the recorded session. The holder is
    /// proven dead, and this is the only answer that lets a hold go.
    Gone,
    /// Nothing states a lineage, or the reading could not be taken. Never "it is gone".
    Unreadable,
}

/// Why a hold moved away from the actor who had it, which is what the log has to say.
///
/// A hold is taken from a previous holder on three grounds and no others, and each one
/// prints its own line. Two of them happen without anybody asking, so the line is the
/// whole of what a person has to go on afterwards: "the hold moved" with no reason is
/// the record that let a false reading pass unnoticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moved {
    /// The table was read all the way through and no process is in the lineage the hold
    /// was taken from. The holder is proven dead.
    ProvenDead,
    /// Neither clock kept it: the idle window ran out with nobody entering the home, or
    /// the absolute expiry a bundle carried has passed. Nothing was read about anybody.
    LeaseExpired,
    /// A person asked, with `--take`.
    Asked,
}

impl Moved {
    /// The clause the log records, which is also the word the `why` attribute keeps.
    ///
    /// Proven death and an expired lease read differently because they are not the same
    /// event and a person acts on them differently: one says a reading was taken and
    /// what it found, the other says a clock ran out and nothing was read.
    const fn why(self) -> &'static str {
        match self {
            Self::ProvenDead => "the previous holder was proven gone from this host",
            Self::LeaseExpired => "the previous holder's lease had expired",
            Self::Asked => "asked for",
        }
    }
}

/// Which of the four a holder, this process and a reading of the table make.
///
/// The reading is passed rather than taken here, so the decision is one value a test can
/// state on either host: this is the kernel, and [`reading_of`] is the attribution. The
/// three answers it can carry are the three a reading has — `Some(true)` the recorded
/// session still holds a process, `Some(false)` the whole table was read and it holds
/// none, and `None` the reading could not be taken or was not called for.
///
/// **`None` never frees the hold.** A row with no recorded session, a process that
/// cannot say which session it is in, a table this host could not list and a table
/// holding a record this account may not read are all `Unreadable`: the rule
/// [`crate::model::Lock::holds_anyone`] states for an actor holds for a lineage, and a
/// record that states nothing refuses nobody and releases nobody.
#[must_use]
pub fn lineage(holder: &Lock, here: Option<u32>, session_holds_a_process: Option<bool>) -> Lineage {
    if holder.was_taken_from(here) {
        return Lineage::Same;
    }
    match (holder.session, here, session_holds_a_process) {
        (Some(_), Some(_), Some(true)) => Lineage::Second,
        (Some(_), Some(_), Some(false)) => Lineage::Gone,
        _ => Lineage::Unreadable,
    }
}

/// The reading [`lineage`] is decided on: whether this host still holds a process in the
/// session the hold was taken from.
///
/// **The row is asked before the machine is.** A hold whose recorded lineage is the one
/// asking is answered `Same` by the row alone, and a reading taken for that entry is a
/// walk of the whole process table thrown away. The short-circuit lives here rather than
/// in [`lineage`] because an argument is evaluated before the function that ignores it:
/// moving the reading to the call site is what made the holder's own re-entry pay for a
/// walk it never looks at.
///
/// That entry is the hot one. `nodal env --export` runs it from the prompt hook on every
/// entry into a home, and `nodal run`, `nodal shell` and `nodal cd` by the holder run it
/// too — one walk of this host's table is about 2,700 reads against a 5 ms budget.
///
/// A reading is otherwise taken only where both sides state a lineage, because a number
/// nobody wrote down is not an identity to resolve. `None` where it was not taken and
/// `None` where it could not be, which are one answer to the decision: nothing was
/// proved, and `Second`, `Gone` and `Unreadable` each still rest on a real reading.
fn reading_of(holder: &Lock, here: Option<u32>) -> Option<bool> {
    if holder.was_taken_from(here) {
        return None;
    }
    let (Some(recorded), Some(_)) = (holder.session, here) else { return None };
    crate::runtime::processes::session_is_live(recorded)
}

/// Answer one entry against the holder the registry actually has.
///
/// Both questions are asked here and in this order: is this the actor who holds it, and
/// is this the lineage the hold was taken from. An actor who matches on the name alone
/// gets no further than a stranger does.
fn settle(conn: &Connection, unit: &Unit, holder: &Lock, entry: &Entry) -> Result<Entered> {
    if holder.is_held_by(&entry.actor, &entry.host) {
        match lineage(holder, entry.session, reading_of(holder, entry.session)) {
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
            Lineage::Gone => return take_over(conn, unit, holder, entry, Moved::ProvenDead),
            Lineage::Second => {}
        }
    }
    if !entry.take {
        return Err(refusal(unit, holder, entry));
    }
    take_over(conn, unit, holder, entry, Moved::Asked)
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
    take_over(conn, unit, &won, entry, Moved::Asked)
}

/// Move the hold here and write the move on the unit's log.
///
/// `why` is the ground [`settle`] already established, passed rather than established
/// again: the answer is what the note has to say, and reading `/proc` a second time to
/// say it would pay for the same walk twice and could answer differently from the
/// decision it describes.
fn take_over(
    conn: &Connection,
    unit: &Unit,
    from: &Lock,
    entry: &Entry,
    why: Moved,
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
        process: Some(crate::runtime::processes::current_process()),
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
/// **Gone is a proof and never a default.** The one reading that says it is a reading of
/// this host's table that nothing carries the holder's identifier. Everything else that
/// stops short of that is unknown, and a report says which, because "I cannot see" and
/// "nobody is there" are different answers and only one of them is news.
///
/// Four things make the answer unknown, and each is a reading that could not be taken or
/// an identity that could not be resolved: a hold from another machine, whose process
/// identifiers mean nothing here; a row that records no process; a host that publishes no
/// process table this account can read; and a process found at the identifier that
/// neither the row nor the table dates, which cannot be told from a later one wearing
/// the same number.
///
/// **The pin is the row's own, not the hold's.** The instant compared against is
/// [`Holding::started_at`], written beside the identifier when the hold was taken. It
/// used to be `taken_at`, which belongs to the hold: a refresh keeps `taken_at` and
/// writes the refreshing process's identifier, so every refreshed hold read as a process
/// that began after the hold and was reported gone while it was running. That false
/// "the holder is gone" is what tells a second actor the home is free.
#[must_use]
pub fn liveness(lock: &Lock, here: &HostName, seen: &Seen) -> HolderState {
    if &lock.host != here {
        return HolderState::Unknown { why: Unknowable::AnotherHost };
    }
    let Some(held) = lock.process.as_ref() else {
        return HolderState::Unknown { why: Unknowable::NoPid };
    };
    match seen.of(held.pid) {
        None => HolderState::Unknown { why: Unknowable::NoProcessTable },
        // The whole table was read and nothing carries the identifier. Proven dead.
        Some(Presence::Gone) => HolderState::Gone,
        Some(Presence::Running { started_at }) => match (started_at, held.started_at) {
            // The process wearing the number now is the one the hold was taken by.
            (Some(found), Some(recorded)) if is_the_same_start(found, recorded) => {
                HolderState::Live
            }
            // An identifier that came round again: a process began at this number at an
            // instant the hold does not name, so it is not the process that took it.
            // Reporting it as the holder would put a stranger's shell in the WHO column.
            (Some(_), Some(_)) => HolderState::Gone,
            // One side of the pair is undated, so the holder cannot be told from a later
            // process wearing its number. Nothing is proved, and an unproved identity
            // never takes a hold away from anybody.
            (None, _) | (_, None) => HolderState::Unknown { why: Unknowable::Undated },
        },
    }
}

/// How far apart two readings of one process's start may be and still be that process.
///
/// A start instant is derived and not stated. Linux publishes the boot instant and the
/// process's age in the kernel's own ticks, and the instant is the sum; macOS states the
/// instant itself. The sum is where the slack comes from: a kernel that recomputes the
/// boot instant as now minus uptime answers a value that can differ by a second between
/// two reads, so one unchanged process read twice gives two instants a second apart.
///
/// A second, and no wider. What the slack could let through is an identifier reused by a
/// process that started within a second of the hold being taken, which needs the whole
/// identifier space to come round inside that second; and the direction it errs in is
/// the hold standing, which is the direction this module errs in everywhere. Comparing
/// for exact equality erred the other way — it reported a running holder gone on a
/// second of arithmetic — which is the fault the pin was added to remove.
const PIN_SLACK_SECONDS: i64 = 1;

/// Whether two readings of a start instant are readings of one process.
fn is_the_same_start(found: Timestamp, recorded: Timestamp) -> bool {
    found.unix_seconds().saturating_sub(recorded.unix_seconds()).abs() <= PIN_SLACK_SECONDS
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
    let pids: Vec<u32> = held.pid().into_iter().collect();
    let seen = Seen::read(&crate::runtime::processes::Live, &pids);
    match liveness(held, &HostName::current(), &seen) {
        HolderState::Gone => {
            let named = held.process.as_ref().map_or_else(
                || String::from("the process"),
                |process| format!("pid {}", process.pid),
            );
            format!(
                " {named} that took it is gone from this host; nodal show says whether \
                 anything of that actor is still in the home."
            )
        }
        HolderState::Live | HolderState::Unknown { .. } => String::new(),
    }
}

/// Whether this entry takes the hold from somebody else, rather than being the holder
/// it already had entering again.
///
/// A holder is an actor, on a host, from a lineage. All three have to be the same for
/// this to be the same worker: the rule this module is built on is that a name is not a
/// writer, so one actor entering from a second session is a second holder and taking the
/// hold from itself is a real move.
///
/// **An unstated lineage moves nothing.** A row written before holds carried one, and a
/// host that will not say which session this process is in, state nothing about which of
/// the two this is. A hand-off written on that is a move nobody can show happened, and
/// the log is the record a person accounts for their home with.
fn moves_the_hold(from: &Lock, entry: &Entry) -> bool {
    if !from.is_held_by(&entry.actor, &entry.host) {
        return true;
    }
    matches!((from.session, entry.session), (Some(recorded), Some(here)) if recorded != here)
}

/// Write the hand-off on the unit's log, so that the move is in the record a person
/// reads rather than only in the row it changed.
///
/// Only a hold somebody had reaches here, so the name is always there. A row that holds
/// no actor names a host rather than a writer, and every caller filters it out before it
/// gets this far: there is nobody for a hand-off to be from.
///
/// **A recorded move is a real move.** A hold that came back to the holder it already
/// had is not a hand-off on any of the three grounds, and the one that can reach here
/// that way is an expired lease: the clock released the row, its own holder asked next,
/// and nothing changed hands. A line saying "taken from ada by ada" is a move a person
/// would go looking for and never find, which is what a log is for not doing. The other
/// two grounds cannot reach here without a move — a proven-dead lineage is a lineage
/// that differs, and `--take` is refused to the holder's own lineage before this — and
/// the guard is written once for all three rather than at each call, because a ground
/// added later must meet it too.
fn record_hand_off(
    conn: &Connection,
    unit: UnitId,
    from: &Lock,
    entry: &Entry,
    why: Moved,
) -> Result<()> {
    if !moves_the_hold(from, entry) {
        return Ok(());
    }
    let was = from.actor.as_ref().map_or_else(String::new, |actor| actor.name.to_string());
    let to = &entry.actor;
    // Why it moved, because the three grounds read the same in the log and are not the
    // same event: one person asked for the hold, one reading proved the holder gone, and
    // one clock ran out with nothing read about anybody.
    let why = why.why();
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
