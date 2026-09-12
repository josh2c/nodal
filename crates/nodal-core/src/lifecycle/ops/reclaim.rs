//! `nodal reclaim`: end a unit, and be able to say what is left.
//!
//! Reclaiming is the operation a person has to trust most, because it is the one that
//! takes things away. Three rules shape it, and each one is a refusal to do the
//! convenient thing.
//!
//! **Nothing is removed until the uniqueness check has been made.**
//! [`crate::lifecycle::uniqueness::check`] is the single answer to "does this home hold
//! work that exists nowhere else", and a hit refuses the reclaim naming what it found.
//! `--force` does not skip the check; it takes a work-in-progress snapshot of the whole
//! home first ([`crate::git::snapshot`]) and then goes on, so a forced reclaim loses a
//! directory rather than the work in it.
//!
//! **A home is moved, never deleted.** It goes to `<state>/<project>/trash/<id>`, and a
//! row records where it went and when [`super::gc`] may remove it. That move is the
//! point of no return for the person: after it, the path they were working in is not
//! there, and every later `nodal` reports the unit as reclaimed. It is not the point of
//! no return for the *operation* — the step's undo moves the directory back, which is
//! what makes a killed reclaim recoverable — and the report says which of the two it
//! means.
//!
//! **Reclaim stops what carries the unit's id; it reports what only stands in the
//! home.** The teardown signals two things: the process groups the registry recorded
//! when `nodal run --tether` started them, and the processes carrying the home's own
//! `NODAL_ID`. Both are records. A process matched by its working directory alone is
//! not: a tmux pane, an editor server over SSH and a teammate's shell all stand in the
//! home and none of them says which unit it is working on
//! ([`crate::runtime::attribute`]). So a reclaim names those by command and process and
//! leaves them running, and it refuses to move the home out from under one of them
//! unless `--force` is given.
//!
//! **The end of the operation is a verification, not an assertion.** Everything the
//! unit had is read back by identifier — its ports, its leases, its sessions, its
//! processes, its containers, its directory — and whatever answers is reported as a
//! leftover. An operation that could only succeed or fail would have to choose between
//! claiming a machine is clean and refusing to finish; a list of what is left is the
//! honest third answer, and it is the one field of the report worth reading first.
//!
//! # What is inside the plan and what is around it
//!
//! The plan holds the work that can be taken back. Stopping a process cannot be taken
//! back, and neither can somebody else's hook command, so those are not steps: a step
//! whose undo silently does nothing would have the runner report a rolled-back reclaim
//! that had in fact killed a server. Runtime is torn down inside the plan because it is
//! ordered work the journal should record; its undo says, in as many words, that it
//! restores nothing.
//!
//! # The tether is the certain target
//!
//! Everything else the teardown stops was inferred: a process is this unit's because it
//! carries the unit's identifier, or because it stands in the unit's home. A tether was
//! *recorded*. `nodal run --tether` put a command in a process group of its own and
//! wrote the group into the registry, so the group is a fact the registry holds rather
//! than a reading of the machine.
//!
//! So tethered groups are the teardown's first target, and they are signalled as
//! groups: one signal reaches the development server, the compiler it started and the
//! watcher that compiler started. The processes a scan attributed are signalled after
//! them, and whatever the group already took with it is simply no longer there.
//!
//! The groups are read once, before the plan runs, and carried in [`Params`]. That is
//! what puts them in the journal, so a reclaim rebuilt after a kill stops the same
//! groups the first attempt was going to.
//!
//! A tether is also the one runtime signal a host with no process table can read. `kill`
//! answers for a process group everywhere, so a tethered server is stopped, and its
//! survival reported, on a machine where attribution can see nothing.
//!
//! # A signal that cannot be read is a note, never silence
//!
//! Stopping a unit's runtime, and the verification that reads it back, both ask the two
//! signals `nodal ps` asks: the process table and the container daemon. Neither is
//! available everywhere — the process table is read from `/proc` and macOS does not
//! publish one ([`crate::runtime::processes`]), and a machine may have no Docker. A
//! reading that cannot be made is reported as a [`Note`] and the reclaim carries on,
//! which is the same contract every attribution signal already has.
//!
//! The verification is where that matters. "Nothing left by id" and "I could not look"
//! are different answers, and a verification that printed the first when it meant the
//! second would be the one lie this operation must not tell. So a note suppresses that
//! line, and the report says which signal went unread.
//!
//! # What a reclaim does on the remote
//!
//! A unit that was sent for review left two kinds of ref on the remote: the branch,
//! which is the person's work and other people's to read, and whatever Nodal wrote
//! under `refs/nodal/<id>/`, which is Nodal's own bookkeeping about a unit that is
//! about to stop existing. Reclaiming deletes the second kind and never the first.
//!
//! It is the one thing besides `nodal done` that reaches a network, and it is gated
//! twice so that it reaches one no more often than it must. A unit no `done` ever
//! pushed for is never asked about — the registry answers that, offline — and the
//! remote is then asked which of those refs it actually holds, so a reclaim deletes
//! names it has read rather than names it has guessed. Every failure out there is a
//! note in the report: a remote that cannot be reached is not a reason to refuse to
//! end a unit on this machine.
//!
//! # Bases and roots
//!
//! A base is never reclaimed by this operation at all: it is not a unit's home, it is
//! what homes are cloned from, and `nodal base gc` is where it goes. A unit adopted in
//! place — a person's own checkout, `managed = false` — is unregistered and never
//! trashed: its rows are closed, its ports are given back, and the directory is left
//! exactly as it is, because Nodal did not create it and it is not Nodal's to move.
//! When that checkout is a linked worktree, the uniqueness check found nothing unique,
//! and the integration verdict is done, the report carries `git worktree remove <path>`.
//! Reclaim asks once and runs it only when the person confirms. Unique work refuses the
//! reclaim and prints nothing runnable. A home Nodal made still goes to the trash; the
//! offer never appears for one.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::context::survey::Bases;
use crate::git::{Git, refs};
use crate::lifecycle::hooks::{
    self, Approvals, Context, Ownership, Phase, Ran, Registered, Runner,
};
use crate::lifecycle::journal::Operation;
use crate::lifecycle::step::{Commit, Output, Outputs, Plan, Step, nothing};
use crate::lifecycle::uniqueness::{self, Finding};
use crate::lifecycle::{Done, Rebuild, guard, marker, run};
use crate::model::{
    EnvId, EnvState, Environment, EventKind, Project, Recipe, Timestamp, Trashed, Unit, UnitId,
    UnitStatus, expiry,
};
use crate::output::view::{Leftover, Pruned, Reclaimed};
use crate::runtime::attribute::{Note, Source, Standing};
use crate::runtime::processes;
use crate::runtime::stop::{self, Signals as _, Stopped, Target};
use crate::services::docker;
use crate::services::ports::{self, Released};
use crate::store::{Store, environments, events, sessions, trash, units};
use crate::workspace::{home, prune};
use crate::{Error, Result};

/// What this operation is called in the journal.
pub const KIND: &str = "reclaim";

/// The key of the step whose answer the registry write reads. Which process groups a
/// teardown could not stop decides which session rows stay open, so the step that finds
/// out and the write that acts on it name the same string.
const TEARDOWN: &str = "runtime.stop";

/// The key of the step that takes the build output out of the trashed copy. The
/// registry write reads it: what the prune dropped is recorded on the trash row.
const PRUNE: &str = "home.prune";

/// The message a forced reclaim's snapshot commit carries.
const SNAPSHOT_MESSAGE: &str = "nodal: work in progress at reclaim";

/// The label a unit's containers carry, which is how they are found again.
const UNIT_LABEL: &str = "nodal.unit";

/// What a person asked `nodal reclaim` for.
#[derive(Debug, Clone)]
pub struct Request {
    /// The unit's handle, or nothing to mean the unit the working directory is in.
    pub target: Option<String>,
    /// Whether to go on past a uniqueness check that found something, after taking a
    /// snapshot of the home.
    pub force: bool,
    /// Whether the project's own hooks run. `false` is `--no-hooks`.
    pub hooks: bool,
    /// Where the command was run, which decides the unit when no target was given.
    pub cwd: PathBuf,
}

/// Everything the plan is built from, and the whole of what the journal keeps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    /// The project the unit belongs to.
    pub project: Project,
    /// The unit being reclaimed.
    pub unit: Unit,
    /// Its materialisation.
    pub environment: Environment,
    /// Where the home is going and when it may be removed, for a home Nodal made.
    /// `None` for a checkout adopted in place, which is unregistered and left alone.
    pub entry: Option<Trashed>,
    /// The process groups the unit's open recorded sessions hold — a tether, or a group
    /// a recipe hook left behind — read after `pre_reclaim` and before anything moves.
    /// These are the teardown's first and most certain targets.
    ///
    /// Defaulted on the way in, because a reclaim journalled by an older Nodal has no
    /// such field and still has to be rebuilt and finished rather than refused.
    #[serde(default)]
    pub tethers: Vec<u32>,
    /// Whether the home is moved although something Nodal did not start is standing in
    /// it. Journalled, because the step that refuses the move is the one a rebuilt plan
    /// runs again, and it has to refuse the same way.
    #[serde(default)]
    pub force: bool,
}

/// Reclaim a unit: check, tear down, trash the home, and verify by identifier.
///
/// # Errors
/// [`Error::NotUnique`] when the home holds work that exists nowhere else and `--force`
/// was not given, [`Error::AlreadyReclaimed`] when the unit has been reclaimed already,
/// [`Error::HomeMarkedFor`] when the directory the registry names belongs to another
/// unit, [`Error::HookNotApproved`] when a declared hook is not the approved one, and
/// whatever Git, the filesystem or the registry reported.
pub fn reclaim(store: &mut Store, request: &Request) -> Result<Reclaimed> {
    let mut prepared = prepare(store, request)?;
    let mut hooks_ran = Vec::new();
    hooks_ran.extend(prepared.hook(
        Phase::PreReclaim,
        prepared.params.environment.home.clone(),
        &Registered { conn: store.conn() },
    )?);
    // The groups are read after the hook and not before it. A `pre_reclaim` that
    // backgrounds work leaves a group this same command has just recorded, and a list
    // taken earlier would journal a teardown that does not stop it — after which the
    // registry write would close the row and the group would be exactly the invisible
    // process the recording exists to prevent.
    prepared.params.tethers = tethers(store.conn(), prepared.params.environment.id)?;
    let params = &prepared.params;
    // Before the home moves, because the remote is reached through the repository in
    // it, and after the hook, because a hook that refuses stops the reclaim and nothing
    // should have left this machine by then.
    let pruned = prune_refs(store.conn(), params)?;
    let done = run(store, &plan(params)?)?;
    hooks_ran.extend(prepared.hook(
        Phase::PostReclaim,
        prepared.after(),
        &Registered { conn: store.conn() },
    )?);
    report(&prepared, &done, hooks_ran, pruned, verify(store, params)?)
}

/// Delete Nodal's own refs for this unit on the remote, and never the branch.
///
/// `None` is "no remote was reached", and it is the answer for every unit no `done` has
/// pushed for: the registry is asked first, and a unit with no push in its history is
/// one this machine has nothing to clean up out there for. That gate is what keeps a
/// reclaim offline for the units that never left.
///
/// Everything after the gate is reported rather than raised. The branch is never named
/// here — the prefix is the unit's own namespace, and [`crate::git::push::delete`]
/// drops anything outside it a second time — so the worst a failure out here can cost
/// is a ref of Nodal's left on a remote, which is not worth refusing a reclaim over.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read.
fn prune_refs(conn: &Connection, params: &Params) -> Result<Option<Pruned>> {
    if !was_pushed(conn, params.unit.id)? {
        return Ok(None);
    }
    let home = params.environment.home.clone();
    if !home.is_dir() {
        return Ok(Some(Pruned::nothing(format!(
            "{} is not there, so nodal's own refs on the remote were left",
            home.display()
        ))));
    }
    let git = match Git::open(&home) {
        Ok(git) => git,
        Err(why) => return Ok(Some(Pruned::nothing(why.to_string()))),
    };
    let remote = match super::done::chosen(&git, None) {
        Ok(remote) => remote,
        Err(why) => return Ok(Some(Pruned::nothing(why.to_string()))),
    };
    let prefix = format!("{}{}/", refs::NAMESPACE, params.unit.id);
    Ok(Some(pruned_on(&git, &remote, &prefix)))
}

/// Ask the remote what it holds under the unit's namespace, and delete that.
fn pruned_on(git: &Git, remote: &str, prefix: &str) -> Pruned {
    let there = match git.remote_refs(remote, prefix) {
        Ok(names) => names,
        Err(why) => return Pruned::on(remote, Vec::new(), vec![why.to_string()]),
    };
    if there.is_empty() {
        return Pruned::on(remote, Vec::new(), Vec::new());
    }
    match git.delete_remote_refs(remote, &there) {
        Ok(deleted) => Pruned::on(remote, deleted, Vec::new()),
        Err(why) => Pruned::on(remote, Vec::new(), vec![why.to_string()]),
    }
}

/// Whether a `done` ever pushed for this unit.
///
/// The `done` is the only thing that writes a [`EventKind::Sync`] for a unit, so one in
/// the history is the registry's record that this unit's branch reached a remote. A
/// unit with none is one nothing of Nodal's is out there for.
fn was_pushed(conn: &Connection, unit: UnitId) -> Result<bool> {
    Ok(!events::list_recent_of_kinds(conn, unit, &[EventKind::Sync], 1)?.is_empty())
}

/// The plan: tear the runtime down, then move the home. One registry write at the end.
///
/// The move is last on purpose. Every earlier step is something the unit's home does
/// not need to exist for, and a step that failed before the move leaves a home a person
/// can still open.
///
/// # Errors
/// [`Error::Render`] when the parameters cannot be written to the journal.
pub fn plan(params: &Params) -> Result<Plan> {
    let value = serde_json::to_value(params)
        .map_err(|source| Error::Render { kind: "operation parameters", source })?;
    let commit = commit_of(params);
    let plan = Plan::new(KIND, params.unit.slug.to_string(), value, commit).then(StopRuntime {
        unit: params.unit.id,
        environment: params.environment.clone(),
        tethers: params.tethers.clone(),
    });
    let Some(entry) = &params.entry else {
        if params.environment.managed {
            return Ok(plan);
        }
        return Ok(plan.then(Unadopt { home: params.environment.home.clone() }));
    };
    Ok(plan
        .then(TrashHome {
            slug: params.unit.slug.clone(),
            unit: params.unit.id,
            home: entry.home.clone(),
            path: entry.path.clone(),
            force: params.force,
        })
        .then(TrashPrune { path: entry.path.clone() }))
}

/// The registry write that finishes a reclaim.
///
/// Ports, leases and sessions are given up here rather than in a step, and that is the
/// difference between a port being free and a port being free *at the same moment* the
/// unit stops existing. A step that released them would leave a window in which the
/// registry says a reclaimed unit still holds a port, and a rolled-back reclaim would
/// have to take back a port another unit may already have been granted.
fn commit_of(params: &Params) -> Commit {
    let (unit, environment) = (params.unit.clone(), params.environment.clone());
    let entry = params.entry.clone();
    Box::new(move |tx: &Transaction<'_>, outputs: &Outputs| -> Result<Output> {
        let now = Timestamp::now();
        let given = ports::release(tx, environment.id)?;
        let pruned: prune::Report = outputs.read(PRUNE)?.unwrap_or_default();
        let standing = still_standing(outputs.read::<Teardown>(TEARDOWN)?.as_ref());
        for session in sessions::list_open(tx, environment.id)? {
            if session.pgid.is_some_and(|pgid| standing.contains(&pgid)) {
                continue;
            }
            sessions::end(tx, session.id, now)?;
        }
        environments::update_state(tx, environment.id, EnvState::Absent, now)?;
        units::update_status(tx, unit.id, UnitStatus::Archived, now)?;
        // The home has gone, so a row naming its writer would name the writer of
        // nothing. Only this host's hold is given up: a claim another machine took is
        // that machine's to release.
        crate::runtime::lock::release(tx, unit.id)?;
        let entry = entry.clone().map(|entry| Trashed { pruned_bytes: pruned.bytes, ..entry });
        if let Some(entry) = &entry {
            trash::insert(tx, entry)?;
        }
        record(tx, &unit, &environment, entry.as_ref(), &given)?;
        // The ports are the one thing in this operation's report that only the write
        // itself knows: they are given back inside this transaction, and what came back
        // is what it returned. It is not journalled, because a resumed reclaim's report
        // is `resolve`'s, not this one's.
        serde_json::to_value(given)
            .map_err(|source| Error::Render { kind: "released ports", source })
    })
}

/// The process groups the teardown signalled and could not stop.
///
/// A tether's row is closed with the rest of the unit's sessions, except this one case.
/// An open row is the claim "this group is still the unit's to stop", and it is what
/// `nodal gc` acts on later. Closing the row of a group that is still running would
/// throw away the only record of it, so a group that survived every signal keeps its
/// row and appears in the report as a leftover as well.
/// A reclaim with no teardown in the journal is one whose plan never had that step, so
/// there is nothing that survived it and every open row is the unit's to close.
fn still_standing(teardown: Option<&Teardown>) -> Vec<u32> {
    let Some(torn) = teardown else { return Vec::new() };
    torn.stopped
        .left
        .iter()
        .filter_map(|target| match target {
            Target::Group(pgid) => Some(*pgid),
            Target::Process(_) => None,
        })
        .collect()
}

/// Write the line a later `nodal explain` reads: what was reclaimed and where it went.
fn record(
    tx: &Transaction<'_>,
    unit: &Unit,
    environment: &Environment,
    entry: Option<&Trashed>,
    released: &Released,
) -> Result<()> {
    let mut refs = vec![("ports", released.allocated.len().to_string())];
    let body = match entry {
        Some(entry) => format!("reclaimed; home moved to {}", entry.path.display()),
        None => format!("unregistered; {} left in place", environment.home.display()),
    };
    if let Some(entry) = entry {
        refs.push(("trash", entry.path.display().to_string()));
        refs.extend(entry.snapshot.clone().map(|snapshot| ("snapshot", snapshot)));
    }
    events::note(tx, (unit.id, Some(environment.id)), EventKind::Note, body, &refs)
}

/// Finding an interrupted reclaim again, from what the journal kept.
pub struct Reclaim;

impl Rebuild for Reclaim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn rebuild(&self, record: &Operation) -> Result<Plan> {
        let params: Params = serde_json::from_value(record.params.clone()).map_err(|_| {
            Error::InvalidValue { kind: "reclaim parameters", value: record.id.to_string() }
        })?;
        plan(&params)
    }
}

// ---------------------------------------------------------------------------
// The steps.
// ---------------------------------------------------------------------------

/// What the teardown of a unit's runtime did.
///
/// Journalled, because the registry write needs it and does not always run in the
/// process that produced it: which process groups the teardown could not stop is what
/// decides which session rows the reclaim leaves open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Teardown {
    /// What became of the processes attributed to the unit.
    pub stopped: Stopped,
    /// The containers that were removed.
    pub containers: Vec<String>,
    /// The signals that could not be read, and why.
    pub notes: Vec<Note>,
}

/// Stop everything attributed to the unit: its processes and its containers.
///
/// This step reads the machine through the same seams `nodal ps` does
/// ([`crate::runtime::processes`], [`docker`], [`stop`]) and uses the live
/// implementation of each, because a rebuilt plan is built from the journal alone and
/// cannot be handed one. A signal that cannot be read is a reason in the report and
/// never a failure: a host whose process table Nodal cannot see is still a host whose
/// home can be moved.
struct StopRuntime {
    /// The unit whose runtime this is.
    unit: UnitId,
    /// Its materialisation, which is the scope the signals are read against.
    environment: Environment,
    /// The process groups its tethers hold, from the journal rather than the machine.
    tethers: Vec<u32>,
}

impl Step for StopRuntime {
    fn key(&self) -> String {
        String::from(TEARDOWN)
    }

    /// Repeatable: a second run finds nothing attributed and stops nothing.
    fn apply(&self) -> Result<Output> {
        let mut seen = attributed(self.unit, std::slice::from_ref(&self.environment.home));
        let stopped = stop::processes(&stop::Live, &self.targets(&seen), stop::GRACE);
        let containers = match docker::remove(&docker::Cli, &seen.containers) {
            Ok(removed) => {
                seen.notes.extend(removed.why.map(|why| Note::new(Source::Docker, why)));
                removed.containers
            }
            Err(error) => {
                seen.notes.push(Note::new(Source::Docker, error.to_string()));
                Vec::new()
            }
        };
        let torn = Teardown { stopped, containers, notes: seen.notes };
        serde_json::to_value(torn).map_err(|source| Error::Render { kind: "teardown", source })
    }

    /// Nothing. A process that has been stopped cannot be started again by anything
    /// that knows only that it used to be running, and saying so here is more honest
    /// than an undo that quietly does nothing.
    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

impl StopRuntime {
    /// What this step signals, in the order it signals them: the recorded groups first,
    /// then the processes that carry the unit's identifier.
    ///
    /// The order is the point. A group takes its whole tree with it, so a process the
    /// scan named is usually gone before its turn comes, and the stop reports what it
    /// actually did rather than counting the same server twice.
    ///
    /// Nothing at the probable level is here. [`Seen::standing`] holds the processes a
    /// scan matched by working directory alone, and this step never signals one: the
    /// same match is made by a tmux pane, an editor server over SSH and a teammate's
    /// shell, and none of the three is the unit's to stop.
    fn targets(&self, seen: &Seen) -> Vec<Target> {
        let groups = self.tethers.iter().map(|pgid| Target::Group(*pgid));
        groups.chain(seen.certain.iter().map(|pid| Target::Process(*pid))).collect()
    }
}

/// What this machine can see of a unit: its processes, its containers, and whatever it
/// could not look at.
#[derive(Debug, Clone, Default)]
struct Seen {
    /// The processes that carry the unit's identifier. These are the certain level, and
    /// they are the only processes a teardown signals.
    certain: Vec<u32>,
    /// The processes a scan matched by working directory alone. These are the probable
    /// level, and they are reported and never signalled.
    standing: Vec<Standing>,
    /// The containers the unit labelled as its own.
    containers: Vec<String>,
    /// The signals that could not be read.
    notes: Vec<Note>,
}

/// Read both signals for one unit. Never fails, for the reason an
/// [`crate::runtime::attribute::Attributor`] never fails: a machine Nodal cannot look
/// at is still a machine whose home can be reclaimed, and what it could not look at is
/// a line of the report.
///
/// Read directly rather than through `nodal ps`, and that is deliberate. Attribution
/// answers about the homes the registry calls live, and a reclaim's whole business is
/// making one of them not live. A verification built on it would go quiet at exactly
/// the moment it is supposed to speak: after the registry write, `ps` would attribute
/// nothing to the unit whether or not anything was still running.
fn attributed(unit: UnitId, homes: &[PathBuf]) -> Seen {
    let mut seen = Seen::default();
    match scan(unit, homes) {
        Ok((certain, standing)) => {
            seen.certain = certain;
            seen.standing = standing;
        }
        Err(error) => seen.notes.push(Note::new(Source::Environment, error.to_string())),
    }
    match docker::survey(&docker::Cli) {
        Ok(docker::Survey::Ran(containers)) => seen.containers = labelled(containers, unit),
        Ok(docker::Survey::Unavailable { why }) => {
            seen.notes.push(Note::new(Source::Docker, why));
        }
        Err(error) => seen.notes.push(Note::new(Source::Docker, error.to_string())),
    }
    seen
}

/// The containers among these that carry this unit's label.
fn labelled(containers: Vec<docker::Container>, unit: UnitId) -> Vec<String> {
    let id = unit.to_string();
    containers
        .into_iter()
        .filter(|container| container.label(UNIT_LABEL) == Some(&id))
        .map(|container| container.name)
        .collect()
}

/// Read the process table once and sort what it says about this unit into the two
/// levels attribution has ([`crate::runtime::attribute::Confidence`]).
///
/// The first list is certain: each process carries `NODAL_ID`, which Nodal wrote into
/// the home's environment and nothing else writes. The second is probable: each process
/// stands in the home and says nothing about which unit it is working on.
///
/// `home` is resolved ([`guard::resolve`]), because the working directory the kernel
/// reports has every symbolic link on the way to it already taken out. A home reached
/// through a link — macOS reaches everything under `/var` that way, and so does anyone
/// whose state directory is a link — would otherwise match no process at all, and a
/// reclaim would quietly stop nothing and then quietly verify nothing.
///
/// The two processes a stop spares ([`stop::spared`]) are left out of the probable list
/// altogether. A person who typed `nodal reclaim` inside the home is standing in it,
/// and their own command must not be the reason their reclaim refuses.
///
/// # Errors
/// Whatever the process table reported, which on a host that has none is
/// [`Error::ProcessScanUnsupported`].
fn scan(unit: UnitId, homes: &[PathBuf]) -> Result<(Vec<u32>, Vec<Standing>)> {
    let placed: Vec<PathBuf> = homes.iter().map(|home| guard::resolve(home)).collect();
    let id = unit.to_string();
    let spared = stop::spared();
    let mut certain = Vec::new();
    let mut standing = Vec::new();
    for process in processes::Processes::scan(&processes::Live)? {
        if process.var(crate::env::vars::ID) == Some(id.as_str()) {
            certain.push(process.pid);
        } else if in_one_of(&process, &placed) && !spared.contains(&process.pid) {
            standing.push(Standing::new(process.pid, process.command.clone()));
        }
    }
    Ok((certain, standing))
}

/// Whether a process stands in one of these directories, which is the whole of the
/// probable signal.
fn in_one_of(process: &processes::Running, homes: &[PathBuf]) -> bool {
    let Some(cwd) = process.cwd.as_deref() else { return false };
    homes.iter().any(|home| cwd.starts_with(home))
}

/// Move the home into the project's trash directory, unless something Nodal did not
/// start is standing in it.
struct TrashHome {
    /// The unit's handle, which is what the refusal names.
    slug: crate::model::Slug,
    /// The unit whose home this is.
    unit: UnitId,
    /// Where the home is.
    home: PathBuf,
    /// Where it goes.
    path: PathBuf,
    /// Whether the move goes ahead over a process standing in the home.
    force: bool,
}

impl Step for TrashHome {
    fn key(&self) -> String {
        String::from("home.trash")
    }

    /// Repeatable in each of the three states a killed run can leave: the home where it
    /// was, the home already in the trash, and neither of the two there at all.
    ///
    /// The check comes first, and it is made here rather than in [`prepare`] because
    /// this is the last instant before the directory goes. The teardown has already
    /// stopped everything that carries the unit's identifier, so what a scan still
    /// finds standing in the home is something Nodal did not start: a tmux pane, an
    /// editor server over SSH, a teammate's shell. Moving the directory out from under
    /// one of those is the surprise this refusal exists to prevent.
    ///
    /// A host whose process table cannot be read moves the home. The reading is a note
    /// in the report, as every unread signal is, and refusing every reclaim on a
    /// machine Nodal cannot look at would be a worse answer than the note.
    fn apply(&self) -> Result<Output> {
        let standing = self.bystanders();
        if !standing.is_empty() && !self.force {
            return Err(Error::HomeInUse { slug: self.slug.clone(), standing });
        }
        move_tree(&self.home, &self.path)?;
        Ok(nothing())
    }

    /// Move it back. This is why the trash is a move and not a delete: the operation's
    /// own recovery needs the directory to still exist.
    fn undo(&self) -> Result<()> {
        move_tree(&self.path, &self.home)
    }
}

impl TrashHome {
    /// What is standing in the home at the probable level, and nothing when the process
    /// table could not be read.
    fn bystanders(&self) -> Vec<Standing> {
        scan(self.unit, std::slice::from_ref(&self.home))
            .map(|(_, standing)| standing)
            .unwrap_or_default()
    }
}

/// Take the build output and the installed dependencies out of the trashed copy.
///
/// The step is after the move and not before it, and that is the whole design. Before
/// the move, this directory is the home a person is working in and its warm build is
/// theirs; after it, the same directory is a copy nothing will ever build in again and
/// the same content is thirteen gigabytes held for a fortnight. So the prune acts on
/// the trash path, and a reclaim that was refused at the move leaves a home with
/// everything in it.
///
/// What it may remove is settled twice over ([`prune`]): an ignore rule has to cover
/// the directory, which is what keeps every tracked path out of reach, and the
/// exclusion table has to call it regenerable, which is what keeps a person's local
/// state in the trash.
struct TrashPrune {
    /// The trashed home, which is where the copy is now.
    path: PathBuf,
}

impl Step for TrashPrune {
    fn key(&self) -> String {
        String::from(PRUNE)
    }

    /// Repeatable: a second run finds nothing an ignore rule covers that the table
    /// calls regenerable, and removes nothing.
    ///
    /// It never fails. A trashed home that is not a repository, a listing that could
    /// not be made and a removal that was refused are each a note on the report, for
    /// the reason a failure here would be worse than the state it complains about: the
    /// home has already moved, and the operation's only other answer would be to undo
    /// that move and leave the unit live because a build directory would not go.
    fn apply(&self) -> Result<Output> {
        let report = prune::sweep(&self.path);
        serde_json::to_value(report).map_err(|source| Error::Render { kind: "trash prune", source })
    }

    /// Nothing, and the reason is worth stating because this step is destructive.
    ///
    /// Everything it removed is state a tool writes again from the tree that is still
    /// there, so a reclaim rolled back after this one leaves the person their home with
    /// a cold build in it. Nothing that was only in this home is gone: the uniqueness
    /// check ran before any of it, and an ignore rule covered every path this touched.
    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

/// Take an adopted checkout back out of Nodal: the files it wrote there, and the rules
/// it added to the repository's own exclude file.
///
/// This is the other half of the promise adoption makes. A checkout adopted in place is
/// a directory Nodal did not create and a person is still working in, so unregistering
/// it has to leave it as it was found rather than leaving `.nodal/`, an `.envrc` that
/// direnv goes on reading, and a marker naming a unit that no longer exists.
///
/// Only a root gets this step. A managed home is moved to the trash whole, and taking
/// its own files out of it first would be work with no observer.
struct Unadopt {
    /// The checkout that was adopted.
    home: PathBuf,
}

impl Step for Unadopt {
    fn key(&self) -> String {
        String::from("home.unadopt")
    }

    /// The marker goes first, because `.nodal/` is removed by the call after it and
    /// only when nothing is left in it.
    ///
    /// Repeatable: no part of it is a failure when it is already gone, and a directory
    /// a person has since deleted leaves nothing to do.
    fn apply(&self) -> Result<Output> {
        if !self.home.is_dir() {
            return Ok(nothing());
        }
        marker::remove(&self.home)?;
        crate::env::files::remove(&self.home)?;
        crate::env::files::unhide(&crate::env::files::exclude_dir(&self.home)?)?;
        Ok(nothing())
    }

    /// Nothing. Everything this removed is a file Nodal wrote and `nodal adopt
    /// --in-place` writes again, and none of it can be put back from here: the
    /// activation is assembled from sources this step does not have. A reclaim that
    /// rolls back after this leaves a registered unit whose checkout is not activated,
    /// which one command repairs and no work is lost to.
    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

/// Move a directory, making the parent of the destination first. A source that is not
/// there is not a failure: a step applies to a world it may already have changed, and
/// an undo to one it never changed.
fn move_tree(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    std::fs::rename(from, to).map_err(Error::io(from))
}

// ---------------------------------------------------------------------------
// What the plan is built from.
// ---------------------------------------------------------------------------

/// Everything worked out before the plan runs, including the refusals.
struct Prepared {
    /// The plan's input.
    params: Params,
    /// What the uniqueness check found, which is empty unless `--force` was given.
    findings: Vec<Finding>,
    /// The project's hooks and the approvals for them.
    runner: Runner,
}

impl Prepared {
    /// Run one phase's hook, in a directory that exists.
    ///
    /// A hook is not a step ([`crate::lifecycle::hooks`]), so this is where the two of
    /// them happen: before the plan, and after the registry write.
    fn hook(&self, phase: Phase, root: PathBuf, owner: &dyn Ownership) -> Result<Option<Ran>> {
        let source = &self.params.project.root;
        let directory =
            if matches!(phase, Phase::PreReclaim) { root.clone() } else { source.clone() };
        let context = Context {
            source: source.clone(),
            root,
            unit: self.params.unit.id,
            slug: self.params.unit.slug.clone(),
            branch: self.params.unit.branch.clone(),
            parent: self.params.environment.base_id.map(|base| base.to_string()),
            environment: self.params.environment.id,
        };
        self.runner.run(phase, &directory, &context, owner)
    }

    /// Where the home is once the plan has run, which is what `post_reclaim` is told.
    fn after(&self) -> PathBuf {
        self.params
            .entry
            .as_ref()
            .map_or_else(|| self.params.environment.home.clone(), |entry| entry.path.clone())
    }
}

/// Work out what the reclaim will do, and make every refusal before anything moves.
fn prepare(store: &mut Store, request: &Request) -> Result<Prepared> {
    let unit =
        crate::runtime::entry::unit_named(store.conn(), request.target.as_deref(), &request.cwd)?;
    let environment = latest(store.conn(), &unit)?;
    let project = project_of(store.conn(), &unit)?;
    let placed = placement(&environment)?;
    let recipe = recipe_of(&project.root);
    let findings = examine(&placed, &project.root, &unit, request.force)?;
    let snapshot = snapshot(&placed, &unit, &findings)?;
    let state_dir = home::directory()?;
    let entry = trashed((&project, &unit, &environment), &placed, &recipe, snapshot)?;
    let runner = Runner {
        project: project.root.clone(),
        hooks: recipe.hooks.clone(),
        approvals: Approvals::open(hooks::path_in(&state_dir))?,
        enabled: request.hooks,
    };
    let params =
        Params { project, unit, environment, entry, tethers: Vec::new(), force: request.force };
    Ok(Prepared { params, findings, runner })
}

/// The process groups an environment's open recorded groups hold: what
/// `nodal run --tether` started, and what a recipe hook left behind
/// ([`crate::lifecycle::hooks`]).
///
/// Read before the plan, so that the groups reach the journal: a reclaim killed
/// part-way and rebuilt from that journal stops the same groups the first attempt was
/// going to, and does not have to find them again in a registry it has since written.
///
/// Read after `pre_reclaim`, so that a group that hook left behind is one of them. The
/// two orderings are both required and they are not in tension: the reading is still
/// the last thing before the plan.
fn tethers(conn: &Connection, environment: EnvId) -> Result<Vec<u32>> {
    Ok(sessions::list_open_tethers(conn, environment)?
        .into_iter()
        .filter_map(|session| session.pgid)
        .collect())
}

/// Where a unit's home is, and whether it is one Nodal may move.
enum Placement {
    /// A home Nodal made, which goes to the trash.
    Managed(PathBuf),
    /// A checkout adopted in place, which is unregistered and left alone.
    Root(PathBuf),
    /// The directory the registry names is not there. Its rows are still closed, and
    /// there is nothing to move or to check.
    Gone,
}

impl Placement {
    /// The home, when there is one on disk to read.
    const fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::Managed(path) | Self::Root(path) => Some(path),
            Self::Gone => None,
        }
    }
}

/// Which of the three a unit's home is, refusing one that belongs to another unit.
fn placement(environment: &Environment) -> Result<Placement> {
    let home = &environment.home;
    if !home.is_dir() {
        return Ok(Placement::Gone);
    }
    if marker::read(home)?.is_some() {
        marker::verify(home, environment.unit_id)?;
    } else if environment.managed {
        return Err(Error::HomeUnmarked { home: home.clone() });
    }
    if environment.managed {
        Ok(Placement::Managed(home.clone()))
    } else {
        Ok(Placement::Root(home.clone()))
    }
}

/// The uniqueness check, and the refusal that comes of it.
///
/// Every destructive path calls this. `--force` does not skip it: the findings are
/// carried into the report, and the caller takes a snapshot before it goes on.
fn examine(placed: &Placement, source: &Path, unit: &Unit, force: bool) -> Result<Vec<Finding>> {
    let Some(home) = placed.path() else { return Ok(Vec::new()) };
    let found = uniqueness::check(home, Some(source))?;
    if found.is_clear() || force {
        return Ok(found.findings);
    }
    Err(Error::NotUnique { slug: unit.slug.clone(), findings: found.findings })
}

/// The work-in-progress snapshot a forced reclaim takes before anything is removed.
fn snapshot(placed: &Placement, unit: &Unit, findings: &[Finding]) -> Result<Option<String>> {
    let Some(home) = placed.path().filter(|_| !findings.is_empty()) else { return Ok(None) };
    let reference = refs::wip(&unit.id.to_string());
    let taken = Git::open(home)?.snapshot(&reference, SNAPSHOT_MESSAGE)?;
    Ok(taken.map(|snapshot| snapshot.reference))
}

/// The trash entry a managed home gets, and nothing for a root or a home that is gone.
fn trashed(
    subject: (&Project, &Unit, &Environment),
    placed: &Placement,
    recipe: &Recipe,
    snapshot: Option<String>,
) -> Result<Option<Trashed>> {
    let (project, unit, environment) = subject;
    let Placement::Managed(home) = placed else { return Ok(None) };
    let at = Timestamp::now();
    Ok(Some(Trashed {
        environment_id: environment.id,
        unit_id: unit.id,
        project_id: project.id,
        slug: unit.slug.clone(),
        home: home.clone(),
        path: home::trashed(&home::directory()?, &project.name, environment.id),
        snapshot,
        pruned_bytes: 0,
        trashed_at: at,
        expires_at: expiry(at, recipe.trash_retention_days()),
    }))
}

/// The unit's newest materialisation, refusing one that has been reclaimed already.
fn latest(conn: &Connection, unit: &Unit) -> Result<Environment> {
    let environment = environments::latest_for_unit(conn, unit.id)?
        .ok_or_else(|| Error::UnitNotMaterialized { slug: unit.slug.to_string() })?;
    if environment.state == EnvState::Absent {
        return Err(Error::AlreadyReclaimed { slug: unit.slug.clone() });
    }
    Ok(environment)
}

/// The project row a unit belongs to.
fn project_of(conn: &Connection, unit: &Unit) -> Result<Project> {
    crate::store::projects::get(conn, unit.project_id)?
        .ok_or_else(|| Error::StoreMissingRow { table: "project", id: unit.project_id.to_string() })
}

/// The project's recipe, or an empty one when the checkout is no longer there.
///
/// A project a person has deleted still has units in the registry, and reclaiming one
/// of them must not fail because the recipe cannot be read. What is lost is the
/// project's chosen retention and its hooks, and the defaults are the safe values for
/// both: fourteen days, and no command.
///
/// `super::gc` reads it for the same retention, so that the window a merged unit keeps
/// its home for and the window its trashed home keeps are one setting and not two.
pub(super) fn recipe_of(root: &Path) -> Recipe {
    crate::recipe::load(root).map(|effective| effective.recipe).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The verification.
// ---------------------------------------------------------------------------

/// Read everything the unit had, by identifier, and report whatever is still there.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read.
pub fn verify(store: &Store, params: &Params) -> Result<(Vec<Leftover>, Vec<Note>)> {
    let environment = params.environment.id;
    let mut leftovers = Vec::new();
    leftovers.extend(rows(store.conn(), environment)?);
    let seen = running(params, &mut leftovers);
    leftovers.extend(directories(params));
    Ok((leftovers, seen))
}

/// The registry rows that should have gone: ports, leases, sessions, and the states.
fn rows(conn: &Connection, environment: EnvId) -> Result<Vec<Leftover>> {
    let mut leftovers = Vec::new();
    for allocation in crate::store::port_allocations::list_for_environment(conn, environment)? {
        leftovers.push(Leftover::new("port", format!("{} ({})", allocation.port, allocation.name)));
    }
    for lease in crate::store::leases::list_for_environment(conn, environment)? {
        leftovers.push(Leftover::new("lease", lease.resource.to_string()));
    }
    for session in sessions::list_open(conn, environment)? {
        leftovers.push(Leftover::new("session", session.id.to_string()));
    }
    if let Some(row) =
        environments::get(conn, environment)?.filter(|row| row.state != EnvState::Absent)
    {
        leftovers.push(Leftover::new("row", format!("environment is {:?}", row.state)));
    }
    Ok(leftovers)
}

/// Add the tethers, processes and containers still attributed to the unit, and answer
/// with the signals that could not be read.
///
/// A tether is asked about first and by identifier: the registry recorded the group, so
/// "is it still there" is one signal rather than a reading of the process table. That is
/// why a surviving tether is reported on every host, including one whose process table
/// went unread.
///
/// The two processes the stop spares ([`stop::spared`]) are left out. A person who ran
/// `nodal reclaim` from inside the home is standing in a directory that has just moved,
/// which they can see; they are not a leftover.
fn running(params: &Params, leftovers: &mut Vec<Leftover>) -> Vec<Note> {
    for pgid in &params.tethers {
        if stop::Live.alive(Target::Group(*pgid)) {
            leftovers.push(Leftover::new("tether", pgid.to_string()));
        }
    }
    let seen = attributed(params.unit.id, &watched(params));
    let spared = stop::spared();
    for pid in seen.certain.iter().filter(|pid| !spared.contains(pid)) {
        leftovers.push(Leftover::new("process", pid.to_string()));
    }
    for process in &seen.standing {
        leftovers.push(Leftover::new("standing", process.describe()));
    }
    for name in seen.containers {
        leftovers.push(Leftover::new("container", name));
    }
    seen.notes
}

/// The directories the verification looks for a standing process in: the name the home
/// had, and the name it has now.
///
/// Both, because a home is moved rather than deleted. The kernel reports a working
/// directory by following the inode, so a process that stood in the home before the
/// move now stands in the trash path. Watching only the old name would report nothing at
/// all, which is the one answer this operation must not give.
fn watched(params: &Params) -> Vec<PathBuf> {
    let mut homes = vec![params.environment.home.clone()];
    homes.extend(params.entry.as_ref().map(|entry| entry.path.clone()));
    homes
}

/// The directory that should have moved, and the one that should now hold it.
fn directories(params: &Params) -> Vec<Leftover> {
    let Some(entry) = &params.entry else { return Vec::new() };
    let mut leftovers = Vec::new();
    if entry.home.exists() {
        leftovers.push(Leftover::new("directory", entry.home.display().to_string()));
    }
    if !entry.path.exists() {
        leftovers.push(Leftover::new("trash", format!("{} is not there", entry.path.display())));
    }
    leftovers
}

/// The answer, assembled from what each part of the operation left behind.
fn report(
    prepared: &Prepared,
    done: &Done,
    hooks: Vec<Ran>,
    pruned: Option<Pruned>,
    verified: (Vec<Leftover>, Vec<Note>),
) -> Result<Reclaimed> {
    let params = &prepared.params;
    let torn: Teardown = done.outputs.read(TEARDOWN)?.unwrap_or_default();
    let trimmed: prune::Report = done.outputs.read(PRUNE)?.unwrap_or_default();
    let released: Released = serde_json::from_value(done.committed.clone())
        .map_err(|_| Error::InvalidValue { kind: "released ports", value: KIND.to_owned() })?;
    let (leftovers, mut notes) = verified;
    // A signal both halves failed to read says so once. The teardown's note and the
    // verification's are the same sentence about the same machine.
    for note in torn.notes {
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    Ok(Reclaimed {
        now: Timestamp::now(),
        slug: params.unit.slug.to_string(),
        findings: prepared.findings.clone(),
        snapshot: params.entry.as_ref().and_then(|entry| entry.snapshot.clone()),
        stopped: torn.stopped,
        containers: torn.containers,
        released,
        trashed: params
            .entry
            .as_ref()
            .map(|entry| Trashed { pruned_bytes: trimmed.bytes, ..entry.clone() }),
        trimmed,
        pruned,
        root: root_of(params),
        hooks,
        notes,
        leftovers,
        worktree_remove: offer_remove(params, &prepared.findings),
    })
}

/// Run `git worktree remove` on an adopted worktree the person confirmed.
///
/// The command runs in the project's main checkout, which is the repository that
/// recorded the worktree. Nodal still never removes a worktree it did not make on its
/// own: this is the person's `git` command, run once.
///
/// # Errors
/// [`Error::Git`] when Git refused, [`Error::NotARepository`] when `path` is not one.
pub fn remove_worktree(path: &Path) -> Result<()> {
    let git = Git::open(path)?;
    let main =
        git.worktrees()?.into_iter().next().map_or_else(|| path.to_path_buf(), |row| row.path);
    Git::open(&main)?.remove_worktree(path)
}

/// The path of a done, clean, adopted worktree, when reclaim may offer to remove it.
///
/// Nothing unique, a linked worktree Nodal did not make, and an integration verdict of
/// done. A home Nodal made is never a row here. Findings from `--force` are unique work,
/// so they suppress the offer too.
fn offer_remove(params: &Params, findings: &[Finding]) -> Option<PathBuf> {
    if !findings.is_empty() || params.environment.managed {
        return None;
    }
    let home = &params.environment.home;
    let git = Git::open(home).ok()?;
    if !git.layout().ok()?.is_linked() {
        return None;
    }
    let standing = Bases::default().standing(&git, &params.unit).ok()?;
    standing.integration.is_integrated().then(|| home.clone())
}

/// The directory an adopted checkout was left at, when that is what this was.
fn root_of(params: &Params) -> Option<PathBuf> {
    if params.entry.is_some() || params.environment.managed {
        return None;
    }
    Some(params.environment.home.clone())
}
