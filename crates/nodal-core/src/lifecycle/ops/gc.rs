//! `nodal gc`: remove what reclaim only moved aside, once its retention has run out.
//!
//! Reclaiming a unit does not delete anything ([`super::reclaim`]). It moves the home
//! to the project's trash directory and writes down the first instant it may go. This
//! is the operation that acts on that instant, and it is the only thing in Nodal that
//! deletes a directory a person worked in.
//!
//! Three things happen, in this order, and the order is the point.
//!
//! **Idle runtime is stopped first.** A tether whose unit was reclaimed, a process still
//! standing in a home that was reclaimed last week, a container still labelled for a
//! unit that no longer exists — these are what "idle" means here, and they are stopped
//! before the directory under them is removed rather than after, so that nothing is
//! deleting a tree a running process is reading. Runtime belonging to a unit that is
//! *still live* is never touched: a person's development server is not garbage, whatever
//! the clock says about it, and never removing what was not asked for is worth more than
//! the disk.
//!
//! The tethers come first, for the reason they come first in a reclaim
//! ([`super::reclaim`]): the registry recorded each group when `nodal run --tether`
//! started it, so a group is a record rather than an inference, and one signal reaches
//! every process in it. A group is read from the *environments* that have been
//! reclaimed, not from the units, because an environment in [`EnvState::Absent`] is one
//! whose home has been taken away and nothing of it should still be running.
//!
//! A tether's row is closed once its group is empty. That is what keeps the sweep from
//! signalling a group identifier the system has since given to something else: a row is
//! acted on only while it is open, and it stays open only while the group is there.
//!
//! A process that only stands in the directory is reported and never signalled. The
//! sweep signals a recorded tether and a process that carries a gone unit's `NODAL_ID`.
//! A working directory is not a statement of ownership: a tmux pane, an editor server
//! over SSH and a teammate's shell all match it, and the sweep cannot tell one of those
//! from a build somebody forgot. So it names them and leaves them running.
//!
//! **The retention is read once.** Two of the steps below measure a window the project
//! asked for, and the recipe is the only thing that states it. It is read once for the
//! whole sweep, before anything acts, because running the whole of recipe inference for
//! one number is the reading, not the number. A project whose recipe will not load is a
//! line of the report and is swept for nothing: a window nobody can read is not a window
//! to guess at, and guessing it would take a home away early.
//!
//! **A merged unit's home is given back before any of that.** A unit the list found
//! merged keeps its home, because the day after a merge is exactly when somebody wants
//! to look at what they did. It keeps it for the retention the project asked for
//! (`reclaim.trash_retention`), measured from the moment the merge was recorded, and
//! then this sweep reclaims it — by the ordinary path, so the uniqueness check applies
//! in full. A merged unit somebody has since put new work in is refused and named in
//! the report rather than removed, which is the whole reason the reclaim is reused
//! instead of the directory being taken directly.
//!
//! Reclaiming is not removing. The home goes to the trash with a retention of its own,
//! and a later sweep is what finally takes it.
//!
//! **Then the expired homes go**, one at a time: the directory first, then the row.
//! That order is the same one `nodal base gc` uses and for the same reason. A process
//! killed between them leaves a row pointing at nothing, which the next sweep clears; the
//! other order would leave a directory nothing knows about, which nothing would ever
//! clean up.
//!
//! Each of them is read again first, and that reading is the reason this step is not a
//! timer. A reclaim let the home go because its commits existed somewhere else, and that
//! somewhere else is a repository on this disk. The copy can go while the home sits in
//! the trash — somebody deletes a branch in a clone, a host drops a merged branch — and
//! until this sweep nothing looked again. The retention running out then removed the last
//! copy of a commit, with no refusal and no line.
//!
//! So every expired home is read with the reading a reclaim makes
//! ([`crate::lifecycle::assess`]), over the two refs that reclaim proved: `HEAD`, and the
//! `refs/nodal/<unit>/wip` a forced reclaim wrote the working tree onto, which no branch
//! reaches ([`work_tips`]). A commit no ref outside the directory reaches keeps the home,
//! keeps the row, and prints one line naming the commit and the copy the reclaim rested
//! on ([`crate::model::Rested`]). The row stays expired, so the next sweep reads it again
//! and a copy somebody restores is all it takes.
//!
//! A home the reclaim forced past a finding is not read again. The loss was named,
//! printed and accepted before the home was moved, and a sweep that refused to act on it
//! would keep every forced reclaim's home for ever and make `--force` mean nothing.
//!
//! Nothing is removed on a reading that could not be made. A home Git will not open, and
//! a project the registry has lost, are each one line of the report and a directory that
//! stays.
//!
//! **Then the records of runs that are over go.** The runner writes one ref per run
//! before it takes its first step ([`crate::git::snapshot`]), and until this sweep
//! nothing ever removed one. A record is kept for the same window a trashed home is
//! kept, for the same reason: the retention a project asked for is how long a person has
//! to read work back. The clock runs from the commit, because a snapshot commit is
//! written once and never moved.
//!
//! Three records are never removed. One whose run is still open is what that run's own
//! rollback reads. One whose run failed is the record most worth keeping. One whose
//! journal row is gone cannot be shown to be over, so it stays.
//!
//! **Then the lapsed leases are given back.** A lease outlives the environment that
//! took it only when something went wrong, so this is a repair rather than a routine.
//!
//! A sweep is not a [`crate::lifecycle::Plan`], because removing a directory has no
//! undo and a step is required to have one. What it has instead is an order in which
//! being interrupted is safe at every point.
//!
//! **Idle live units are reported, on request, and never touched.** `nodal gc --idle`
//! adds one section to the report: the units nobody has been in for longer than the
//! threshold, read from the session rows ([`crate::lifecycle::idle`]). It stops nothing
//! of theirs and asks nothing about them. A person's development server is not garbage
//! whatever the clock says, so the answer to "this has been quiet for a month" is a
//! line in a report and a person's own decision.
//!
//! The two signals it reads to find that idle runtime — the process table and the
//! container daemon — are not available on every host. A reading that cannot be made is
//! a [`Note`] in the answer and never a failure, which is the contract every attribution
//! signal already has, and it keeps "nothing was running" apart from "I could not look".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::git::{Git, Oid, refs, snapshot, union};
use crate::lifecycle::assess;
use crate::lifecycle::idle;
use crate::lifecycle::journal;
use crate::lifecycle::kernel::{self, Evidence, LossSet};
use crate::lifecycle::uniqueness::Finding;
use crate::lifecycle::witness::Checkout;
use crate::model::{
    EnvState, OperationId, OperationState, Outside, Project, ProjectId, SessionId, Timestamp,
    Trashed, Unit, UnitId, UnitStatus, trash as retention,
};
use crate::output::view::{HeldBack, Idle, Leftover, Retired, Swept};
use crate::runtime::attribute::{Note, Source, Standing};
use crate::runtime::processes::{Processes, Running};
use crate::runtime::stop::{self, Signals as _, Stopped, Target};
use crate::services::docker;
use crate::store::{Store, environments, leases, projects, sessions, trash, units};
use crate::workspace::remove::tree as remove_tree;

use super::reclaim;
use crate::{Error, Result};

/// The label a unit's containers carry.
const UNIT_LABEL: &str = "nodal.unit";

/// What one sweep was asked to do beyond its ordinary work.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Report the live units nothing has touched for this many days. `None` asks for no
    /// such report, which is not the same as a report that found none.
    pub idle: Option<u32>,
    /// Whether the project's own reclaim hooks run for a merged unit whose home is
    /// given back. `false` is `--no-hooks`.
    pub hooks: bool,
}

/// Reclaim the merged units whose retention has run out, remove every expired home,
/// stop the runtime of units that are gone, and give back lapsed leases.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read or written. A directory that
/// will not go, and a merged unit the uniqueness check refuses, are [`Leftover`]s in
/// the answer rather than errors: one home nobody can remove must not stop the rest of
/// the sweep.
pub fn collect(store: &mut Store, now: Timestamp, options: &Options) -> Result<Swept> {
    let mut leftovers = Vec::new();
    // Before anything is read, so that every reading after it is of rows that are still
    // true. A recorded group whose processes have gone is not runtime and not an
    // attachment, and a sweep that counted it would report a unit as held by somebody
    // and never as idle.
    let host = crate::model::HostName::current();
    let gone = crate::runtime::sessions::close_dead_groups(store.conn(), &host, now)?;
    tracing::debug!(gone, "recorded groups that had already ended");
    let registered = projects::list(store.conn())?;
    let retentions = retentions(&registered, &mut leftovers);
    let retired = retire(store, now, options.hooks, &retentions, &mut leftovers)?;
    let expired = trash::list_expired(store.conn(), now)?;
    let stopped = stop_absent(store.conn())?;
    let mut expiry = sweep(store, &expired, &registered)?;
    leftovers.append(&mut expiry.leftovers);
    let records = forget(store.conn(), now, &retentions, &mut leftovers)?;
    let released = release_lapsed(store.conn(), now, &mut leftovers)?;
    for process in &stopped.standing {
        leftovers.push(Leftover::new("standing", process.describe()));
    }
    Ok(Swept {
        now,
        removed: expiry.removed,
        kept: kept(store.conn(), now)?,
        held: expiry.held,
        freed_bytes: Some(expiry.freed),
        stopped: stopped.stopped,
        containers: stopped.containers,
        leases: released,
        records,
        retired,
        idle: match options.idle {
            Some(days) => quiet(store.conn(), now, days)?,
            None => Vec::new(),
        },
        idle_asked: options.idle.is_some(),
        notes: stopped.notes,
        leftovers,
    })
}

// ---------------------------------------------------------------------------
// What each project asked for, read once.
// ---------------------------------------------------------------------------

/// One project and the window it keeps a home and a record for.
#[derive(Debug, Clone, Copy)]
struct Retention<'a> {
    /// The project itself, which the reclaim of one of its units is given. Named rather
    /// than copied: the rows the sweep reads this from outlive every step that uses one.
    project: &'a Project,
    /// Days, from `reclaim.trash_retention`.
    days: u32,
}

/// What every project asked for, read once for the whole sweep.
///
/// Reading it is running the whole of recipe inference, so the two steps that measure
/// the window share one reading rather than taking one each.
///
/// A recipe that will not load is a line of the report, and its project is left out, so
/// the sweep removes nothing of it and touches nothing of it. The alternative is the
/// default window, which for a project that asked for a longer one takes a home away
/// early; a number nobody could read is not a number to act on.
///
/// The projects arrive read, because the expiry step reads them too: a trashed home is
/// read again against the checkout of the project it belonged to, and that project is
/// one of these rows whether or not its recipe loads.
fn retentions<'a>(registered: &'a [Project], leftovers: &mut Vec<Leftover>) -> Vec<Retention<'a>> {
    let mut read = Vec::new();
    for project in registered {
        match crate::recipe::load(&project.root) {
            Ok(effective) => {
                read.push(Retention { days: effective.recipe.trash_retention_days(), project });
            }
            Err(why) => {
                leftovers.push(Leftover::new("project", format!("{}: {why}", project.name)));
            }
        }
    }
    read
}

// ---------------------------------------------------------------------------
// The merged units whose retention has run out.
// ---------------------------------------------------------------------------

/// Give back the home of every merged unit that has kept one long enough.
///
/// Through [`super::reclaim`], not around it. That is the point of this whole path: the
/// uniqueness check, the teardown, the hooks and the trash entry are the ones a person
/// gets when they type `nodal reclaim`, so a merged unit somebody has since put work in
/// is refused here exactly as it would be there.
///
/// A refusal is a line of the report. So is any other failure of one unit, because a
/// sweep that stopped at the first unit it could not reclaim would leave the rest of the
/// machine untouched for a reason that has nothing to do with them. A registry that
/// cannot be read is not one of those and is raised.
fn retire(
    store: &mut Store,
    now: Timestamp,
    hooks: bool,
    retentions: &[Retention<'_>],
    leftovers: &mut Vec<Leftover>,
) -> Result<Vec<Retired>> {
    let mut retired = Vec::new();
    for (project, unit) in due(store.conn(), now, retentions)? {
        let request = reclaim::Request {
            target: Some(unit.slug.to_string()),
            force: false,
            hooks,
            // Never. An idle unit is retired without a person present, and the build
            // output of a checkout somebody adopted is theirs to give up.
            prune: false,
            cwd: project.root.clone(),
        };
        match reclaim::reclaim(store, &request) {
            Ok(report) => retired.push(Retired {
                slug: unit.slug.clone(),
                trashed: report.trashed.map(|entry| entry.path),
            }),
            Err(Error::Store { path, source }) => return Err(Error::Store { path, source }),
            Err(refused) => leftovers.push(refusal(&unit, &refused)),
        }
    }
    Ok(retired)
}

/// The merged units whose home has been kept for as long as the project asked.
///
/// The clock runs from the unit's own `updated_at`, which for a merged unit is the
/// instant the merge was recorded ([`crate::lifecycle::states`]). A unit whose home has
/// already gone is not one of these: there is nothing left to give back.
fn due<'a>(
    conn: &Connection,
    now: Timestamp,
    retentions: &[Retention<'a>],
) -> Result<Vec<(&'a Project, Unit)>> {
    let mut found = Vec::new();
    for Retention { project, days } in retentions {
        for unit in units::list_by_status(conn, project.id, UnitStatus::Merged)? {
            let expires = retention::expiry(unit.updated_at, *days);
            if expires.unix_seconds() <= now.unix_seconds() && live_home(conn, &unit)?.is_some() {
                found.push((*project, unit));
            }
        }
    }
    Ok(found)
}

/// The home the unit still has, `None` when its materialisation has been reclaimed.
///
/// One reading for the two questions asked of it: whether a reclaim has anything to act
/// on, and where this machine keeps the refs of the unit.
fn live_home(conn: &Connection, unit: &Unit) -> Result<Option<PathBuf>> {
    let Some(environment) = environments::latest_for_unit(conn, unit.id)? else { return Ok(None) };
    Ok((environment.state != EnvState::Absent).then_some(environment.home))
}

/// The line a refused unit gets, naming what was found rather than a policy.
fn refusal(unit: &Unit, why: &Error) -> Leftover {
    let detail = match why {
        Error::NotUnique { findings, .. } => {
            format!("{}: {}", unit.slug, Finding::summarise(findings))
        }
        other => format!("{}: {other}", unit.slug),
    };
    Leftover::new("unit", detail)
}

// ---------------------------------------------------------------------------
// The snapshot records of runs that are over.
// ---------------------------------------------------------------------------

/// Remove the pre-operation record of every run that is over and has been kept long
/// enough, and answer with the refs that went.
///
/// The window is the one the trash keeps a home for, read once with the rest
/// ([`retentions`]), so a project states it in one place. A ref that will not go is a
/// line of the report: one record nobody can remove must not stop the rest of the sweep.
fn forget(
    conn: &Connection,
    now: Timestamp,
    retentions: &[Retention<'_>],
    leftovers: &mut Vec<Leftover>,
) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for Retention { project, days } in retentions {
        for unit in units::list(conn, project.id)? {
            let Some(home) = live_home(conn, &unit)? else { continue };
            for record in collectable(conn, &home, &unit, *days, now) {
                match Git::at(&home).delete_ref(&record) {
                    Ok(()) => removed.push(record),
                    Err(why) => {
                        leftovers.push(Leftover::new("record", format!("{record}: {why}")));
                    }
                }
            }
        }
    }
    Ok(removed)
}

/// The records of this home that this sweep may remove, oldest first.
///
/// The same listing `nodal show` prints ([`snapshot::list`]), so what the report offers
/// to read back and what the sweep removes are one reading. A home Git cannot answer for
/// has no records to remove and is not a failure.
fn collectable(
    conn: &Connection,
    home: &Path,
    unit: &Unit,
    days: u32,
    now: Timestamp,
) -> Vec<String> {
    let Ok(taken) = snapshot::list(home, &unit.id.to_string()) else { return Vec::new() };
    taken
        .into_iter()
        .filter(|record| {
            retention::expiry(record.taken_at, days).unix_seconds() <= now.unix_seconds()
                && records_a_run_that_is_over(conn, &record.reference, unit)
        })
        .map(|record| record.reference)
        .collect()
}

/// Whether this ref is a pre-operation record whose run has finished either way.
///
/// The name says which run wrote it ([`refs::operation_in`], which is also what the
/// report reads it by) and the journal row says how that run ended. Every other ref of
/// the namespace answers `false`: the work-in-progress ref, the branch before a squash,
/// the copies a home took of the checkout, and a record whose run is still open, failed,
/// or whose row is no longer there.
fn records_a_run_that_is_over(conn: &Connection, reference: &str, unit: &Unit) -> bool {
    let Some(operation) = refs::operation_in(reference, &unit.id.to_string()) else {
        return false;
    };
    let Ok(id) = OperationId::parse(operation) else { return false };
    let Ok(Some(run)) = journal::get(conn, id) else { return false };
    matches!(run.state, OperationState::Committed | OperationState::RolledBack)
}

// ---------------------------------------------------------------------------
// The live units that have gone quiet.
// ---------------------------------------------------------------------------

/// The live units nothing has touched for `days`, longest quiet first.
///
/// Reported and nothing else. Every environment here is one a person can still open,
/// and this function reads timestamps ([`idle`]) rather than the machine: no process is
/// signalled, no container is asked about, and nothing is written.
fn quiet(conn: &Connection, now: Timestamp, days: u32) -> Result<Vec<Idle>> {
    let mut found = Vec::new();
    for environment in environments::list_all(conn)? {
        if environment.state == EnvState::Absent {
            continue;
        }
        let Some(unit) = units::get(conn, environment.unit_id)? else { continue };
        if !matches!(unit.status, UnitStatus::Open | UnitStatus::Review) {
            continue;
        }
        let sessions = sessions::list_for_environment(conn, environment.id)?;
        if idle::attached(&sessions) {
            continue;
        }
        let since = idle::last_seen(&sessions, environment.last_active, now);
        if idle::is_idle(since, now, days) {
            found.push(Idle { slug: unit.slug, home: environment.home, since });
        }
    }
    found.sort_by_key(|unit| (unit.since.unix_seconds(), unit.slug.to_string()));
    Ok(found)
}

/// What the sweep did about runtime that outlived its unit.
///
/// Named for the units it is about — the ones whose homes are gone — rather than for
/// the word "idle", which in this command means the live units that are only reported.
#[derive(Debug, Default)]
struct Outlived {
    /// What became of the processes that carry a gone unit's identifier.
    stopped: Stopped,
    /// The containers that went.
    containers: Vec<String>,
    /// The processes standing in a home that is gone. Reported, never signalled.
    standing: Vec<Standing>,
    /// The signals that could not be read.
    notes: Vec<Note>,
}

/// The entries whose retention has not run out yet.
fn kept(conn: &Connection, now: Timestamp) -> Result<Vec<Trashed>> {
    Ok(trash::list(conn)?.into_iter().filter(|entry| !entry.has_expired(now)).collect())
}

/// What the expiry step did with the homes whose retention had run out.
#[derive(Debug, Default)]
struct Expiry {
    /// The homes that went, oldest first.
    removed: Vec<Trashed>,
    /// What those directories occupied.
    freed: u64,
    /// The homes that stayed although their retention had run out, and why.
    held: Vec<HeldBack>,
    /// The directories that would not go.
    leftovers: Vec<Leftover>,
}

/// Read each expired home again, remove the ones that hold no last copy, and forget them.
///
/// The reading comes first, and a home it keeps keeps its row as well. The row is still
/// expired afterwards, so the next sweep reads the same home again: a copy somebody
/// restores is all it takes for the directory to go then.
///
/// A project is read once, however many of its homes expired on one sweep. What a
/// reading asks of the project is the same of every home of it — its checkout, and the
/// repositories beside that checkout — and asking per row paid six `git` invocations
/// and a directory walk to learn one answer over and over ([`Readings`]).
fn sweep(store: &Store, expired: &[Trashed], registered: &[Project]) -> Result<Expiry> {
    let mut swept = Expiry::default();
    let mut readings = Readings::of(registered);
    for entry in expired {
        if read_again(entry) {
            let Some(reading) = readings.of_project(entry.project_id) else {
                swept.leftovers.push(unread(entry, "the registry holds no project it belonged to"));
                continue;
            };
            match held_back(entry, reading) {
                Err(why) => {
                    swept.leftovers.push(unread(entry, &why.to_string()));
                    continue;
                }
                Ok(Some(held)) => {
                    swept.held.push(held);
                    continue;
                }
                Ok(None) => {}
            }
        }
        let size = size_of(&entry.path);
        match remove(&entry.path) {
            Ok(()) => {
                trash::remove(store.conn(), entry.environment_id)?;
                swept.freed += size;
                swept.removed.push(entry.clone());
            }
            Err(why) => swept.leftovers.push(Leftover::new("directory", why.to_string())),
        }
    }
    Ok(swept)
}

/// What every project of this sweep can say about where a commit also lives, read once.
///
/// A [`Checkout`] is six `git` invocations and the sibling walk is a bounded directory
/// walk, and both answer about the project rather than about the home. A sweep of one
/// project's four expired homes therefore pays for them once.
///
/// A project the registry no longer holds is not in here, and a home of one is refused
/// the reading rather than given an empty one: nothing to ask is not the same fact as
/// asked and found nothing.
struct Readings<'a> {
    /// The projects this sweep may read, by identifier.
    registered: BTreeMap<ProjectId, &'a Project>,
    /// What each of them answered, read on the first home that needed it.
    read: BTreeMap<ProjectId, Reading>,
}

/// One project's checkout and the repositories beside it.
struct Reading {
    /// The project's own checkout, read once.
    checkout: Checkout,
    /// The other repositories on this machine that may hold a copy of a commit.
    siblings: Vec<PathBuf>,
}

impl<'a> Readings<'a> {
    /// The projects a sweep may ask, indexed so a home finds its own without a scan.
    fn of(registered: &'a [Project]) -> Self {
        let registered = registered.iter().map(|project| (project.id, project)).collect();
        Self { registered, read: BTreeMap::new() }
    }

    /// This project's reading, taken now if this is the first home to ask for it.
    ///
    /// `None` where the registry holds no such project, which the caller turns into a
    /// refusal to remove anything of it.
    fn of_project(&mut self, project: ProjectId) -> Option<&Reading> {
        let root = &self.registered.get(&project)?.root;
        Some(self.read.entry(project).or_insert_with(|| Reading {
            checkout: Checkout::read(root),
            siblings: crate::doctor::scan::siblings(root),
        }))
    }
}

/// Whether this expired home is one the sweep reads again before it removes it.
///
/// Two homes are not. A reclaim that was forced past a finding named the loss, printed it
/// and moved the home anyway, and re-asking would keep every forced reclaim's home for
/// ever. A home that is not on the disk any more cannot lose anything, and the sweep
/// before this one may have removed the tree and been killed before it wrote the row;
/// removing the row is how that is finished.
fn read_again(entry: &Trashed) -> bool {
    entry.rested.re_asks() && entry.path.exists()
}

/// The line a home that could not be read leaves in the report.
fn unread(entry: &Trashed, why: &str) -> Leftover {
    Leftover::new("trashed home", format!("{}: {why}", entry.path.display()))
}

/// What keeps this expired home, and nothing when nothing does.
///
/// # Errors
/// [`Error::Git`] and [`Error::NotARepository`] when the home could not be read. That is
/// not a reading that found nothing, and nothing is removed on one.
fn held_back(entry: &Trashed, reading: &Reading) -> Result<Option<HeldBack>> {
    let Some(finding) = only_here(entry, reading)? else { return Ok(None) };
    let gone = gone_copies(entry, finding.commits());
    Ok(Some(HeldBack { entry: entry.clone(), finding, gone }))
}

/// The commits of a trashed home that no ref outside the directory reaches, and what the
/// reading could say about the remote.
///
/// The reading a reclaim makes, and the verdict the one judge makes of it
/// ([`kernel::judge`]) — deliberately not a second implementation of either. **The row the
/// reclaim wrote is not read here at all.** A record of what a removal rested on is not
/// permission to finish it: the sweep asks again, and what it may remove is what its own
/// reading proves. The row is read afterwards, to say which of the copies it named has
/// since gone ([`gone_copies`]).
///
/// Two things differ from the reclaim's reading, and each is a fact about a home nobody is
/// working in.
///
/// The work is read from the refs the reclaim proved rather than from every ref the home
/// holds ([`work_tips`]).
///
/// The paths of the working tree decide nothing, because a reclaim already settled them: a
/// home with none was the condition of an ordinary reclaim, and a forced one put what it
/// found on a ref. An untracked file the trash still holds would otherwise keep a home for
/// ever over a file nobody committed. So the loss set handed to the judge carries the
/// commits and no path, which is that rule stated as a value rather than as a filter over
/// the answer.
///
/// `None` is a home the sweep may remove. The [`Finding`] a held-back home answers with is
/// the words the report prints over it, read only where the verdict already refused.
///
/// # Errors
/// [`Error::Git`] and [`Error::NotARepository`] when the trashed home could not be read.
fn only_here(entry: &Trashed, reading: &Reading) -> Result<Option<Finding>> {
    let git = Git::open(&entry.path)?;
    let tips = work_tips(&git, entry)?;
    let input = assess::Input {
        work: assess::Work::Tips(&tips),
        ..assess::Input::refusal(&entry.path, Some(&reading.checkout), &reading.siblings)
    };
    let assessment = assess::assess(&input)?;
    let commits = LossSet {
        home: entry.path.clone(),
        paths: Vec::new(),
        commits: assessment.commits.clone(),
    };
    if kernel::judge(&commits, &Evidence::of_work(Timestamp::now())).safe() {
        return Ok(None);
    }
    Ok(assessment
        .findings()
        .into_iter()
        .find(|finding| matches!(finding, Finding::Unpushed { .. })))
}

/// The copies the reclaim rested on that no longer reach any of these commits.
///
/// The row names what made the removal safe; this says which of those names is the one
/// that has since gone, so the line a person reads is about the copy that actually went
/// rather than about every copy the reclaim ever counted. A repository that still holds
/// one of these commits is not named, and neither is one that will not answer: a copy
/// nobody could read was not found gone, and the line says only what this reading
/// proved.
///
/// One `rev-list` per recorded copy, and only for a home this sweep is keeping.
fn gone_copies(entry: &Trashed, sample: &[Oid]) -> Vec<Outside> {
    entry
        .rested
        .copies()
        .iter()
        .filter(|copy| {
            Git::open(&copy.repository)
                .and_then(|git| git.held(sample))
                .is_ok_and(|held| held.is_empty())
        })
        .cloned()
        .collect()
}

/// The refs a trashed home's own work is on: `HEAD`, and the work-in-progress snapshot a
/// forced reclaim wrote.
///
/// Two names and not every ref, and the rule behind the pair is one sentence: gc reads
/// exactly what the reclaim proved. A reclaim reads the working tree and `HEAD`
/// ([`crate::lifecycle::assess::Work::Checkout`]), so a commit no other reading of this
/// home ever looked at cannot by itself keep the directory; and a ref that exists only
/// because Nodal wrote it is not the person's work. `wip` is the one exception both
/// halves agree on: a forced reclaim put the work it found there itself.
///
/// Everything left out is left out under that rule. `refs/nodal/origin/*` and
/// `refs/nodal/checkout/*` are readings Nodal fetched in from the person's own checkout,
/// and `refs/nodal/<unit>/target` is a copy of the branch the unit was to merge into;
/// reading one of those as work would keep the home over a branch the person deleted in
/// their own checkout.
///
/// **Every `refs/heads/*` is left out, the unit's own branch among them.** A home is a
/// byte copy of a base, so it carries the base's `refs/heads/main` frozen at the moment
/// the base was built. A rewrite of `main` past that commit in the person's checkout
/// would leave the home holding the only copy of a commit the reclaim never looked at,
/// and pin the directory for ever. The unit's own branch is no different in a detached
/// home: a commit on it that `HEAD` does not reach is a commit the reclaim was never
/// refused over, and a sweep that read it would keep that home on every sweep from then
/// on with nothing a person could do to release it. Reading the branch on both sides is
/// the wider reading, and it is one change rather than two halves.
///
/// **A pre-operation record and a pre-merge record are left out as well**, and these are
/// the two that have to be argued because both hold real commits.
///
/// Every reclaim writes a pre-operation record before its first step
/// ([`crate::git::snapshot`]), so every trashed home holds one; and `nodal merge` writes
/// `refs/nodal/<unit>/premerge` before it squashes, so every merged unit holds commits
/// that by construction exist nowhere else. Reading either as work keeps that home for
/// ever, which is a leak and not a safety property: the merge that wrote the premerge ref
/// is the operation that put the work on the target branch, and the reclaim that wrote
/// the record is the one being carried out.
///
/// Neither holds content the rest of this list does not. A record's tree is the home's
/// working tree and its parent is the home's own branch; the homes read here are the ones
/// whose reclaim found nothing only there, so their working trees held no uncommitted and
/// no untracked work. Where a reclaim did find something, `--force` put it on `wip`,
/// which is named above, and [`crate::model::Rested::re_asks`] keeps that home out of
/// this reading altogether.
fn work_tips(git: &Git, entry: &Trashed) -> Result<Vec<Oid>> {
    let named = [String::from("HEAD"), refs::wip(&entry.unit_id.to_string())];
    let mut tips = Vec::new();
    for name in named {
        tips.extend(git.rev_parse_opt(&name)?);
    }
    Ok(union(&tips, &[]))
}

/// Stop what is still running for a unit whose home is not there any more.
///
/// The scope is deliberately narrow. An environment in [`EnvState::Absent`] has been
/// reclaimed: nothing of it should be running, and anything that is, is left over from
/// before the reclaim rather than work somebody is doing. Every other environment is
/// left alone, however long it has been since anybody touched it.
///
/// Narrow in the other direction too. A signal goes to a recorded tether and to a
/// process that carries a gone unit's identifier, and to nothing else. A process that
/// only stands in the directory a reclaimed home was, or is now, is reported as
/// [`Standing`] and left running: the sweep cannot tell a build somebody forgot from a
/// tmux pane, an editor server over SSH, or a teammate's shell.
fn stop_absent(conn: &Connection) -> Result<Outlived> {
    let tethers = absent_tethers(conn)?;
    let gone = absent_units(conn)?;
    let vacated = vacated(conn)?;
    if gone.is_empty() && tethers.is_empty() {
        return Ok(Outlived::default());
    }
    let mut outlived = Outlived::default();
    let (pids, standing, containers) = seen(&gone, &vacated, &mut outlived.notes);
    outlived.standing = standing;
    let mut targets: Vec<Target> = tethers.iter().map(|(_, pgid)| Target::Group(*pgid)).collect();
    targets.extend(pids.into_iter().map(Target::Process));
    outlived.stopped = stop::processes(&stop::Live, &targets, stop::GRACE);
    close_empty(conn, &tethers)?;
    match docker::remove(&docker::Cli, &containers) {
        Ok(removed) => {
            outlived.notes.extend(removed.why.map(|why| Note::new(Source::Docker, why)));
            outlived.containers = removed.containers;
        }
        Err(error) => outlived.notes.push(Note::new(Source::Docker, error.to_string())),
    }
    Ok(outlived)
}

/// The tethers of every environment that has been reclaimed, as row and group.
///
/// Only open rows, and only reclaimed environments. A live unit's tether is its
/// person's development server, and this sweep never touches one.
fn absent_tethers(conn: &Connection) -> Result<Vec<(SessionId, u32)>> {
    let mut found = Vec::new();
    for environment in environments::list_by_state(conn, EnvState::Absent)? {
        for session in sessions::list_open_tethers(conn, environment.id)? {
            if let Some(pgid) = session.pgid {
                found.push((session.id, pgid));
            }
        }
    }
    Ok(found)
}

/// Close the row of every tether whose group is now empty, and leave the rest open.
///
/// A group that would not stop keeps its row, so the next sweep tries again. A group
/// that has gone gives its row up, so no later sweep signals an identifier the system
/// has handed to something else.
fn close_empty(conn: &Connection, tethers: &[(SessionId, u32)]) -> Result<()> {
    let now = Timestamp::now();
    for (session, pgid) in tethers {
        if stop::Live.alive(Target::Group(*pgid)) {
            continue;
        }
        sessions::end(conn, *session, now)?;
    }
    Ok(())
}

/// What this machine can see of the units that are gone, noting what it cannot look at.
///
/// Two lists come back from the one scan, and they are the two levels attribution has
/// ([`crate::runtime::attribute::Confidence`]). The first names a gone unit outright and
/// is signalled. The second only stands in a directory a gone home used, and is
/// reported.
fn seen(
    gone: &[UnitId],
    vacated: &[PathBuf],
    notes: &mut Vec<Note>,
) -> (Vec<u32>, Vec<Standing>, Vec<String>) {
    let mut pids = Vec::new();
    let mut standing = Vec::new();
    match Processes::scan(&crate::runtime::processes::Live) {
        Ok(running) => {
            notes.extend(crate::runtime::attribute::withheld(&running));
            let spared = stop::spared();
            for process in running {
                if names_a_gone_unit(&process, gone) {
                    pids.push(process.pid);
                } else if stands_in(&process, vacated) && !spared.contains(&process.pid) {
                    standing.push(Standing::new(process.pid, process.command.clone()));
                }
            }
        }
        Err(error) => notes.push(Note::new(Source::Environment, error.to_string())),
    }
    let containers = match docker::survey(&docker::Cli) {
        Ok(docker::Survey::Ran(containers)) => labelled(containers, gone),
        Ok(docker::Survey::Unavailable { why }) => {
            notes.push(Note::new(Source::Docker, why));
            Vec::new()
        }
        Err(error) => {
            notes.push(Note::new(Source::Docker, error.to_string()));
            Vec::new()
        }
    };
    (pids, standing, containers)
}

/// Every directory a reclaimed home was in, and every directory one is in now.
///
/// Both names, because a home is moved rather than deleted. The kernel reports a
/// working directory by following the inode, so a process that stood in the home before
/// the reclaim now stands in the trash path, and a process that entered the directory
/// after the reclaim stands in the old one. Watching one name and not the other would
/// report half of them.
fn vacated(conn: &Connection) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for environment in environments::list_by_state(conn, EnvState::Absent)? {
        paths.push(crate::paths::resolve(&environment.home));
    }
    for entry in trash::list(conn)? {
        paths.push(crate::paths::resolve(&entry.path));
    }
    Ok(paths)
}

/// Whether a process stands in one of these directories.
fn stands_in(process: &Running, vacated: &[PathBuf]) -> bool {
    process.cwd.as_deref().is_some_and(|cwd| vacated.iter().any(|vacated| cwd.starts_with(vacated)))
}

/// The units every one of whose materialisations has been reclaimed.
fn absent_units(conn: &Connection) -> Result<Vec<UnitId>> {
    let mut gone = Vec::new();
    for unit in environments::list_by_state(conn, EnvState::Absent)? {
        let live = environments::list_for_unit(conn, unit.unit_id)?
            .iter()
            .any(|row| row.state != EnvState::Absent);
        if !live && units::get(conn, unit.unit_id)?.is_some() && !gone.contains(&unit.unit_id) {
            gone.push(unit.unit_id);
        }
    }
    Ok(gone)
}

/// Whether a process says it belongs to one of these units.
fn names_a_gone_unit(process: &Running, gone: &[UnitId]) -> bool {
    process
        .var(crate::env::vars::ID)
        .and_then(|id| UnitId::parse(id).ok())
        .is_some_and(|id| gone.contains(&id))
}

/// The containers among these that are labelled for a unit that is gone.
fn labelled(containers: Vec<docker::Container>, gone: &[UnitId]) -> Vec<String> {
    containers
        .into_iter()
        .filter(|container| {
            container
                .label(UNIT_LABEL)
                .and_then(|id| UnitId::parse(id).ok())
                .is_some_and(|id| gone.contains(&id))
        })
        .map(|container| container.name)
        .collect()
}

/// Give back every lease whose claim has run out.
fn release_lapsed(
    conn: &Connection,
    now: Timestamp,
    leftovers: &mut Vec<Leftover>,
) -> Result<Vec<String>> {
    let mut released = Vec::new();
    for lease in leases::list_expired(conn, now)? {
        if leases::release(conn, &lease.resource, lease.environment_id)? {
            released.push(lease.resource.to_string());
        } else {
            leftovers.push(Leftover::new("lease", lease.resource.to_string()));
        }
    }
    Ok(released)
}

/// Remove a directory and everything under it. One that is not there is already gone.
///
/// A trashed home is the home a unit had, so it holds the read-only content its base
/// held, and `gc` is the last thing that will ever look at it. It uses the removal that
/// opens what it must, because a directory `gc` walks past is a directory nobody
/// collects.
fn remove(path: &Path) -> Result<()> {
    remove_tree(path)
}

/// What a directory occupies, as the sum of the sizes of the files under it.
///
/// An approximation, and named as one: it counts a file once whatever the filesystem
/// did about sharing its blocks, so a home cloned copy-on-write reads as the size of
/// its content rather than the space it actually took. Reporting the larger of the two
/// is the safe direction for a number a person reads after a deletion. Anything that
/// cannot be read contributes nothing rather than failing the sweep.
fn size_of(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            total += size_of(&entry.path());
        } else if kind.is_file() {
            total += entry.metadata().map_or(0, |data| data.len());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{remove, size_of};

    #[test]
    fn a_directory_reads_as_the_sum_of_what_is_under_it() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a"), [0_u8; 100]).unwrap();
        std::fs::create_dir(root.path().join("d")).unwrap();
        std::fs::write(root.path().join("d").join("b"), [0_u8; 23]).unwrap();
        assert_eq!(size_of(root.path()), 123);
    }

    #[test]
    fn what_is_not_there_reads_as_nothing_and_removes_without_complaint() {
        let root = tempfile::TempDir::new().unwrap();
        let missing = root.path().join("never");
        assert_eq!(size_of(&missing), 0);
        remove(&missing).unwrap();
    }
}
