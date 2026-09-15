//! The list of a project's units: what `nodal ls`, and a bare `nodal`, answer with.
//!
//! The list is the command a person types most, so it is the one with a startup budget
//! and the one that must never write. Nothing here opens a transaction, records an
//! event or reconciles a session row.
//!
//! **Each home is asked each question once.** The reading is
//! [`crate::context::survey`] and this module turns its answer into rows. The survey is
//! taken anyway, for the memories the command writes afterwards, and it already reads
//! everything a row shows; taking a second reading here made every home answer
//! `status`, `rev-list`, `merge-tree` and `rev-parse` twice for one `nodal ls`. One
//! pass is the rule the survey's own module doc states, for the same reason: a fact
//! read twice is a fact that can be read two ways.
//!
//! A home that Git cannot answer for is a note under the table, not a failure. A list
//! that refuses to print because one directory was removed is worth less than a list
//! that prints nine rows and says which one it could not read. The survey makes those
//! notes; this module carries them to the table.
//!
//! A reclaimed materialisation is not one of those. It is `absent` because a reclaim
//! moved it away on purpose, so there is no directory to ask Git about and no note to
//! make; the unit is listed with no home, the way it is before it is materialised. This
//! is the rule [`crate::runtime::ps::scope`] already applies, for the same reason.
//!
//! What is read here rather than surveyed is the process table, once, for who is
//! attached to each home. It is not a question about a repository and no home is asked
//! it.
//!
//! **NEEDS is decided from what has already been read.** The column says why a unit
//! wants a person, ranked, in the words `nodal reclaim --check` uses
//! ([`crate::lifecycle::assess::Needs`]), so that one word means one thing in both
//! places. What it must not do is start a survey of its own: the list is the command a
//! person types most and it is held to a git-process budget per row (`ci/measure.sh`).
//!
//! So every input is one the list already holds. The counts, the verdict and the
//! divergence come from the survey. The bystander comes from the one process scan below,
//! which is read for WHO anyway. The staleness of the remote evidence is the one new
//! reading, and it is `stat` and never `git`: the checkout's own newest reading of the
//! remote is taken once for the whole list, and each home is compared against it
//! ([`crate::lifecycle::witness::read_since`]).
//!
//! That reading is the cheap necessary half of the witness rule and not the rule. A row
//! it marks `unknown` is a row whose remote evidence **cannot** be current; whether a
//! reading that is current actually reaches the commits is a question with a cost, and
//! `nodal reclaim --check` is where it is paid. The column points at the unit; the
//! preflight answers about it.
//!
//! **WHO is two readings, in this order.** The lock rows first, then the process table.
//! The order is not a preference: the process table is `/proc`, which does not cross
//! Linux accounts, so on a host two engineers share it cannot see the other person at
//! all. The lock row is written down and therefore can. A lapsed hold is not a holder
//! and never reaches a row, which is why the caller passes the live ones rather than
//! every row the table holds.
//!
//! **What the list writes** (`docs/contracts.md`, The list): "The list's reading is
//! pure. After reading, the command layer records at most two things it learned or
//! derived: a unit's flip to merged, and each touched unit's recomputed `WORKUNIT.md`.
//! It records no event, reconciles no session, and never contacts the network."
//!
//! Neither of the two is here. Both belong to the command, after this answer is in
//! hand: the flip to merged is recorded by [`crate::lifecycle::states::settle`], which
//! the `ls` and `show` commands call, and the memories are compiled by
//! [`crate::context::compile`]. This module writes nothing at all, which is what makes
//! that sentence true of module boundaries and not only of what `nodal ls` does.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::Connection;

use crate::Result;
use crate::context::survey::{self, Snapshot, Work};
use crate::doctor::unique;
use crate::git::Integration;
use crate::lifecycle::{assess, witness};
use crate::model::{ActorName, EnvId, HostName, Lock, Needs, Project, Session, Timestamp, UnitId};
use crate::output::notice::{self, Notice};
use crate::output::view::{
    EnvLine, Holder, HolderState, ToolSessions, UnitList, UnitRow, WorkTree,
};
use crate::paths;
use crate::runtime::processes::{Processes, Running};
use crate::runtime::{sessions, stop};
use crate::store::sessions as session_rows;

/// Every unit of a project, with what Git and the process table say about each.
///
/// One survey of the project, turned into rows. A caller that needs the survey itself —
/// the `ls` command, which compiles the memories from it — takes it with
/// [`survey::project`] and calls [`rows`] instead, so every home is read once for the
/// whole command rather than once for each reader of the answer.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read.
pub fn list(
    conn: &Connection,
    processes: &dyn Processes,
    project: &Project,
    now: Timestamp,
) -> Result<UnitList> {
    let surveyed = survey::project(conn, project)?;
    let held = crate::runtime::lock::live(conn, &project.root, now)?;
    let idle_hours = crate::runtime::lock::idle_hours(&project.root);
    let open = session_rows::list_open_all(conn)?;
    let held = Held::of(&held, idle_hours, processes).recording(&open);
    Ok(rows(&surveyed, processes, project, &held, now))
}

/// The writers of a project's units, by unit, with the project's idle window applied.
///
/// The rows are read once for the whole list rather than once per unit, for the reason
/// the survey gives about Git: a list of eight units must not become eight statements to
/// answer one column.
///
/// The process table is asked whether each recorded holder is still running
/// ([`crate::runtime::lock::liveness`]), so that a row never says "holds" about a session
/// that ended. That is a reading of `/proc` and never a signal.
#[derive(Debug, Default)]
pub struct Held {
    /// Who holds each unit.
    holders: BTreeMap<UnitId, Holder>,
    /// The processes the registry recorded for each materialisation, from its open
    /// session rows.
    ///
    /// The list reads these for the same reason `nodal reclaim --check` does: Nodal's
    /// own tether wrapper carries no identifier in its own environment, so the record is
    /// the only thing that says the process standing in a home is Nodal's own
    /// ([`assess::Own`]). The list and the preflight print the same word over the same
    /// process, so they read the same inputs.
    recorded: BTreeMap<EnvId, Vec<u32>>,
}

impl Held {
    /// The holders of these locks, as a report shows them.
    #[must_use]
    pub fn of(locks: &[Lock], idle_hours: u32, processes: &dyn Processes) -> Self {
        let here = HostName::current();
        // One reading of the process table for every lock, rather than one for each.
        let pids: Vec<u32> = locks.iter().filter_map(|lock| lock.pid).collect();
        let seen = crate::runtime::lock::Seen::read(processes, &pids);
        Self {
            holders: locks
                .iter()
                .filter_map(|lock| {
                    let state = crate::runtime::lock::liveness(lock, &here, &seen);
                    Some((lock.unit_id, Holder::from_lock(lock, idle_hours, state)?))
                })
                .collect(),
            recorded: BTreeMap::new(),
        }
    }

    /// The same, with the processes the registry recorded for each materialisation.
    ///
    /// One query for the whole list, because a list of eight units must not put eight
    /// statements to the registry to answer one column.
    #[must_use]
    pub fn recording(mut self, sessions: &[Session]) -> Self {
        for session in sessions {
            if let Some(pid) = session.pid {
                self.recorded.entry(session.environment_id).or_default().push(pid);
            }
        }
        self
    }

    /// Who holds one unit, `None` when nobody does.
    #[must_use]
    pub fn of_unit(&self, unit: UnitId) -> Option<Holder> {
        self.holders.get(&unit).cloned()
    }

    /// The processes the registry recorded for one materialisation.
    fn of_environment(&self, environment: EnvId) -> &[u32] {
        self.recorded.get(&environment).map_or(&[], Vec::as_slice)
    }
}

/// The same list, built from a survey the caller has already taken.
#[must_use]
pub fn rows(
    surveyed: &[Snapshot],
    processes: &dyn Processes,
    project: &Project,
    held: &Held,
    now: Timestamp,
) -> UnitList {
    let mut notices = Vec::new();
    // Each home with the unit it belongs to, because a process carrying another unit's
    // identifier is a bystander here and the predicate has to be asked with this one's
    // ([`assess::bystander`]).
    let homes: Vec<Placed> = surveyed
        .iter()
        .filter_map(|subject| {
            let environment = subject.home.as_ref()?;
            Some(Placed::new(subject.unit.id, &environment.home, held.of_environment(environment.id)))
        })
        .collect();
    let seen = scan(processes, &homes, &mut notices);
    // One reading of the checkout for the whole list, and two `stat` calls per home
    // against it. Asking `git` per row is what the budget in `ci/measure.sh` forbids, and
    // whether the project has a remote is a column of its registry row rather than a
    // question for Git at all.
    let remote =
        Reading { exists: project.remote_url.is_some(), heard: unique::heard(&project.root) };
    let mut units = Vec::new();
    for subject in surveyed {
        notices.extend(
            subject.notes.iter().map(|cause| Notice::about(subject.unit.slug.to_string(), cause)),
        );
        units.push(row(subject, &seen, held, remote));
    }
    order(&mut units);
    // One line per cause, however many units reported it. A list of eight units whose
    // project tracks its own `CLAUDE.md` is eight units with one thing wrong, not eight
    // things (`crate::output::notice`).
    let notes = notice::collapse(&notices, "units");
    UnitList { project: project.name.clone(), now, units, worktrees: Vec::new(), notes }
}

/// Put the units a person should look at first at the top.
///
/// Two rules, in this order. A unit whose work is already on the base is finished, so it
/// sinks. Everything else is sorted by how far the base has moved under it, most behind
/// first, because that is the unit whose next command is a sync. Units that tie are
/// ordered by slug, so one list of one registry is always the same list.
pub fn order(rows: &mut [UnitRow]) {
    rows.sort_by(|left, right| {
        let key = |row: &UnitRow| {
            let work = row.work.as_ref();
            (
                work.is_some_and(|work| work.integration.is_integrated()),
                std::cmp::Reverse(work.map_or(0, |work| work.main.behind)),
            )
        };
        key(left).cmp(&key(right)).then_with(|| left.slug.as_str().cmp(right.slug.as_str()))
    });
}

/// One row: the registry's facts about a unit, and the survey's.
fn row(subject: &Snapshot, seen: &Seen, held: &Held, remote: Reading) -> UnitRow {
    let mut row = UnitRow::from_unit(&subject.unit);
    // The writer is a fact about the unit and not about its home, so it is set before
    // the row gives up on a unit that has none. A unit whose home was reclaimed holds
    // nothing, because the reclaim released it.
    row.holder = held.of_unit(subject.unit.id);
    let Some(environment) = subject.home.as_ref() else { return row };
    row.sessions = seen.attached.of(&environment.home);
    if let Some(holder) = row.holder.as_mut() {
        still_there(holder, &row.sessions);
    }
    row.last_active = Some(environment.last_active);
    row.environment = Some(EnvLine::from_environment(environment));
    row.work = subject.work.as_ref().map(work_tree);
    // Resolved, because `read_since` reads `.git` under the path it is given and a
    // state directory reached through a symbolic link is the ordinary shape on macOS.
    let reading = Remote {
        exists: remote.exists,
        current: witness::read_since(&paths::resolve(&environment.home), remote.heard),
    };
    row.needs = needs(subject.work.as_ref(), seen.blocked(&environment.home), reading);
    row
}

/// A hold belongs to an actor, and an actor outlives any one process of theirs.
///
/// The identifier a lock row carries is the command that entered the home, and that
/// command has usually ended long before anybody reads the list: `nodal new` writes its
/// own identifier and exits. Reading that alone would report every unit as held by
/// somebody who is gone, which is a different untruth from the one this replaced.
///
/// So a hold whose recorded process is gone is read once more, against the same scan the
/// WHO column is built from: where a process of that actor stands in the home, the actor
/// is there and the hold is live. A hold with neither is an agent that was killed with
/// nothing of it left in the home, and it is the only one reported as gone.
fn still_there(holder: &mut Holder, sessions: &[ToolSessions]) {
    if holder.state == HolderState::Gone && sessions.iter().any(|seen| seen.tool == holder.actor) {
        holder.state = HolderState::Live;
    }
}

/// What this machine knows about the project's remote, for one row.
///
/// Read once for the whole list and asked of each home: whether the project has a remote
/// at all, and when anything here last heard from it. Both are the registry and `stat`;
/// neither costs a `git`.
#[derive(Debug, Clone, Copy)]
struct Reading {
    /// Whether the project row names a remote.
    exists: bool,
    /// When the project's checkout last heard from it, `None` when nothing says.
    heard: Option<SystemTime>,
}

/// The same, once it has been compared with one home.
#[derive(Debug, Clone, Copy)]
struct Remote {
    /// Whether the project has a remote for this machine to be out of date about.
    exists: bool,
    /// Whether anything here read it after this home wrote its own record of it.
    current: bool,
}

/// Why this unit needs a person, most actionable first.
///
/// The order of [`Needs`] is the ranking and the first match wins, so a unit with
/// uncommitted work and a conflict is reported as the first of the two. Every branch
/// here is decided from a reading the list already took.
fn needs(work: Option<&Work>, blocked: bool, remote: Remote) -> Option<Needs> {
    // A home Git could not answer for has no ranking, and `Nothing` would be the wrong
    // answer rather than a cautious one. The top of the ranking is the working tree, so
    // a reading that could not see the working tree cannot say that the top is empty.
    // The note under the table is where the reason goes ([`crate::context::survey`]).
    let work = work?;
    if work.dirty + work.staged + work.untracked > 0 {
        return Some(Needs::UniqueLoss);
    }
    if blocked {
        return Some(Needs::BlockingRuntime);
    }
    if stale(work, remote.exists, remote.current) {
        return Some(Needs::UnknownEvidence);
    }
    if work.integration == Integration::Conflict || work.divergence.is_behind() {
        return Some(Needs::Diverged);
    }
    if work.integration.is_integrated() || work.divergence.ahead > 0 {
        return Some(Needs::Review);
    }
    Some(Needs::Nothing)
}

/// Whether this unit has work whose remote evidence cannot be current.
///
/// Three things have to hold. The unit is ahead of the base, so it has something to
/// lose. The **project** has a remote, so there is a remote for this machine to be out of
/// date about. And nothing here has read that remote since the home last wrote its own
/// record of it — which is the reading a home makes when it pushes and never corrects
/// afterwards ([`witness`]).
///
/// The middle one is about the project and not about the unit, and the difference is a
/// whole class of unit. A branch nobody has pushed has no upstream at all, and reading
/// that as "no remote question" would call it `review` while `nodal reclaim` refuses it:
/// its commits are on no remote and nothing here proved otherwise. A unit with no
/// upstream is the case with the most to lose, not the least.
///
/// A unit with nothing of its own is not marked, however old the reading is. There is
/// nothing about it a stale ref could get wrong.
const fn stale(work: &Work, has_remote: bool, current: bool) -> bool {
    work.divergence.ahead > 0 && has_remote && !current
}

/// What the survey read of one home, as the list shows it.
///
/// The counts and the upstream are the survey's own reading of the one `git status` it
/// runs; the logs it read beside them are the memory's, and a row has no room for them.
fn work_tree(work: &Work) -> WorkTree {
    WorkTree {
        dirty: work.dirty,
        staged: work.staged,
        untracked: work.untracked,
        detached: work.detached,
        base: work.base.clone(),
        main: work.divergence,
        integration: work.integration,
        remote: work.remote.clone(),
    }
}

/// Who is attached to each home, counted by tool.
#[derive(Debug, Default)]
struct Attached(BTreeMap<PathBuf, BTreeMap<ActorName, u32>>);

impl Attached {
    /// The tools attached to one home, by name.
    fn of(&self, home: &Path) -> Vec<ToolSessions> {
        self.0
            .get(home)
            .map(|counts| {
                counts
                    .iter()
                    .map(|(tool, count)| ToolSessions { tool: tool.clone(), count: *count })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// What one reading of the process table said about the project's homes.
///
/// Two answers out of one scan, because they are two questions about one table and
/// reading it twice would double the cost of the column that is cheapest to get wrong.
#[derive(Debug, Default)]
struct Seen {
    /// Who is attached to each home, counted by tool. This is WHO.
    attached: Attached,
    /// The homes something this unit may not signal is standing in.
    ///
    /// A tmux pane, an editor server over SSH, a teammate's shell — and a process of
    /// another unit, which Nodal did start and which this unit still may not touch. What
    /// they have in common is the whole of the definition: a reclaim here would signal
    /// none of them and would move the home out from under all of them
    /// ([`assess::bystander`], which decides it).
    bystanders: BTreeSet<PathBuf>,
}

impl Seen {
    /// Whether something Nodal did not start is standing in this home.
    fn blocked(&self, home: &Path) -> bool {
        self.bystanders.contains(home)
    }

}

/// One home the scan asks about, with the unit it belongs to and what the registry says
/// is that unit's own.
///
/// The home is carried twice on purpose: resolved, which is the form the predicate asks
/// for because the kernel's reading of a working directory has every link taken out, and
/// as the registry names it, which is the key every other reading of the list uses.
struct Placed {
    /// The unit whose home this is, and the processes recorded for it.
    unit: UnitId,
    /// Those processes.
    recorded: Vec<u32>,
    /// The home, with every link on the way to it followed.
    resolved: PathBuf,
    /// The home, as the registry names it.
    home: PathBuf,
}

impl Placed {
    /// One home, resolved once for the whole list.
    fn new(unit: UnitId, home: &Path, recorded: &[u32]) -> Self {
        Self {
            unit,
            recorded: recorded.to_vec(),
            resolved: paths::resolve(home),
            home: home.to_path_buf(),
        }
    }

    /// What the registry says is this unit's own.
    fn own(&self) -> assess::Own<'_> {
        assess::Own::of(self.unit, &self.recorded)
    }
}

/// Read the process table once, for who is attached to each home and for what is
/// standing in one.
///
/// A host whose process table Nodal cannot read gets a note and an empty answer, for the
/// reason `docs/contracts.md` gives: a note is the difference between "nothing is
/// attached" and "I could not see". A row's NEEDS is then decided without the runtime
/// half, which understates and never overstates.
///
/// What counts as standing in a home is [`assess::bystander`] and not a rule of this
/// module's own. The list and the preflight print the same word over the same process,
/// so they have to be reading the same predicate — including over a process that carries
/// **another** unit's identifier, which is Nodal's own and is still nothing this unit may
/// signal or move a home out from under.
fn scan(processes: &dyn Processes, homes: &[Placed], notices: &mut Vec<Notice>) -> Seen {
    let running = match processes.scan() {
        Ok(running) => running,
        Err(error) => {
            // About the run, not about a unit: the table is read once for the whole
            // list, so the reason it could not be read is one line whatever the list
            // holds.
            notices.push(Notice::general(format!("who: {error}")));
            return Seen::default();
        }
    };
    let mut seen = Seen { attached: attached(&running, notices), ..Seen::default() };
    let spared = stop::spared();
    for process in &running {
        for placed in homes {
            let placement = std::slice::from_ref(&placed.resolved);
            if assess::bystander(process, placed.own(), placement, &spared) {
                seen.bystanders.insert(placed.home.clone());
            }
        }
    }
    seen
}

/// Who is attached to each home, counted by tool, from the one scan.
fn attached(running: &[Running], notices: &mut Vec<Notice>) -> Attached {
    let derived = match sessions::derive(running) {
        Ok(derived) => derived,
        Err(error) => {
            notices.push(Notice::general(format!("who: {error}")));
            return Attached::default();
        }
    };
    let mut homes: BTreeMap<PathBuf, BTreeMap<ActorName, u32>> = BTreeMap::new();
    for process in derived {
        *homes.entry(process.root).or_default().entry(process.actor.name).or_default() += 1;
    }
    Attached(homes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{Needs, Remote, needs};
    use crate::context::survey::Work;
    use crate::git::{Divergence, Integration};
    use crate::output::view::Remote as Upstream;

    /// A project with a remote that this machine has read since the home last wrote its
    /// own record of it: the state in which the remote evidence is worth something.
    const CURRENT: Remote = Remote { exists: true, current: true };

    /// The same project, where nothing here has read the remote since.
    const STALE: Remote = Remote { exists: true, current: false };

    /// A unit that has committed one thing, pushed it, and has a clean tree.
    fn clean() -> Work {
        Work {
            base: String::from("refs/remotes/origin/main"),
            base_commit: None,
            forked_at: None,
            divergence: Divergence { ahead: 1, behind: 0 },
            integration: Integration::Open,
            dirty: 0,
            staged: 0,
            untracked: 0,
            detached: false,
            remote: Some(Upstream {
                upstream: String::from("origin/nodal/worker-import"),
                divergence: Divergence { ahead: 0, behind: 0 },
            }),
            uncommitted: Vec::new(),
            touched: Vec::new(),
            commits: Vec::new(),
            gained: Vec::new(),
        }
    }

    /// The ranking, asserted where it decides: a unit that is several of these at once
    /// is reported as the most actionable of them, and nothing else.
    #[test]
    fn the_most_actionable_reason_is_the_one_reported() {
        let mut work = clean();
        work.dirty = 3;
        work.integration = Integration::Conflict;
        work.divergence.behind = 4;
        assert_eq!(needs(Some(&work), true, STALE), Some(Needs::UniqueLoss));

        work.dirty = 0;
        assert_eq!(needs(Some(&work), true, STALE), Some(Needs::BlockingRuntime));
        assert_eq!(needs(Some(&work), false, STALE), Some(Needs::UnknownEvidence));
        assert_eq!(needs(Some(&work), false, CURRENT), Some(Needs::Diverged));
    }

    /// Staged and untracked paths are work no commit holds, exactly as changed ones are.
    #[test]
    fn every_kind_of_uncommitted_path_is_possible_unique_loss() {
        for set in [
            |w: &mut Work| w.dirty = 1,
            |w: &mut Work| w.staged = 1,
            |w: &mut Work| {
                w.untracked = 1;
            },
        ] {
            let mut work = clean();
            set(&mut work);
            assert_eq!(needs(Some(&work), false, CURRENT), Some(Needs::UniqueLoss));
        }
    }

    /// A unit whose remote evidence cannot be current is `unknown` and never `nothing`.
    /// A unit with nothing of its own is not marked, however old the reading is: there
    /// is nothing about it a stale ref could get wrong.
    #[test]
    fn an_unreadable_remote_is_unknown_only_where_the_unit_has_something_to_lose() {
        let work = clean();
        assert_eq!(needs(Some(&work), false, STALE), Some(Needs::UnknownEvidence));
        assert_eq!(needs(Some(&work), false, CURRENT), Some(Needs::Review));

        let mut nothing_of_its_own = clean();
        nothing_of_its_own.divergence.ahead = 0;
        assert_eq!(needs(Some(&nothing_of_its_own), false, STALE), Some(Needs::Nothing));
    }

    /// A branch nobody has pushed has no upstream, and that is the case with the most to
    /// lose rather than the least. What decides the question is whether the **project**
    /// has a remote, because that is what this machine can be out of date about.
    ///
    /// Reading it the other way put a never-pushed unit under `review` while `nodal
    /// reclaim` refuses it, which is the one direction this column must not be wrong in.
    #[test]
    fn a_unit_that_was_never_pushed_is_unknown_and_not_review() {
        let mut never_pushed = clean();
        never_pushed.remote = None;
        assert_eq!(needs(Some(&never_pushed), false, STALE), Some(Needs::UnknownEvidence));
    }

    /// A project with no remote at all has no remote reading to be stale, so the age of
    /// one says nothing about it.
    #[test]
    fn a_project_with_no_remote_is_never_marked_unknown() {
        let mut work = clean();
        work.remote = None;
        let no_remote = Remote { exists: false, current: false };
        assert_eq!(needs(Some(&work), false, no_remote), Some(Needs::Review));
    }

    /// Work the base already carries is a unit somebody should end, and it is the lowest
    /// rank that says anything at all.
    #[test]
    fn integrated_work_reads_as_review_and_a_quiet_unit_as_nothing() {
        let mut integrated = clean();
        integrated.integration = Integration::Integrated(crate::git::integration::Reason::Ancestor);
        integrated.divergence.ahead = 0;
        assert_eq!(needs(Some(&integrated), false, CURRENT), Some(Needs::Review));

        let mut quiet = clean();
        quiet.divergence.ahead = 0;
        assert_eq!(needs(Some(&quiet), false, CURRENT), Some(Needs::Nothing));
    }

    /// A home Git could not be asked about has no ranking at all, and the column prints
    /// the placeholder rather than `nothing`.
    ///
    /// The top of the ranking is the working tree, so a reading that could not see the
    /// working tree cannot say the top of it is empty. The two words are different
    /// answers and the contract forbids printing one for the other.
    #[test]
    fn a_home_that_could_not_be_read_has_no_ranking_and_is_not_nothing() {
        assert_eq!(needs(None, true, STALE), None);
        assert_ne!(Needs::Nothing.label(), crate::output::human::NONE);
    }
}
