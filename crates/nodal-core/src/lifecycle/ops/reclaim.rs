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
//! # Bases and roots
//!
//! A base is never reclaimed by this operation at all: it is not a unit's home, it is
//! what homes are cloned from, and `nodal base gc` is where it goes. A unit adopted in
//! place — a person's own checkout, `managed = false` — is unregistered and never
//! trashed: its rows are closed, its ports are given back, and the directory is left
//! exactly as it is, because Nodal did not create it and it is not Nodal's to move.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::git::{Git, refs};
use crate::lifecycle::hooks::{self, Approvals, Context, Phase, Ran, Runner};
use crate::lifecycle::journal::Operation;
use crate::lifecycle::step::{Commit, Plan, Step};
use crate::lifecycle::uniqueness::{self, Finding};
use crate::lifecycle::{Rebuild, guard, marker, run};
use crate::model::{
    EnvId, EnvState, Environment, Epistemic, Event, EventId, EventKind, Project, Recipe, RefName,
    Timestamp, Trashed, Unit, UnitId, UnitStatus, expiry,
};
use crate::output::view::{Leftover, Reclaimed};
use crate::runtime::actor;
use crate::runtime::attribute::{Note, Source};
use crate::runtime::processes;
use crate::runtime::stop::{self, Signals as _, Stopped, Target};
use crate::services::docker;
use crate::services::ports::{self, Released};
use crate::store::{Store, environments, events, sessions, trash, units};
use crate::workspace::home;
use crate::{Error, Result};

/// What this operation is called in the journal.
pub const KIND: &str = "reclaim";

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
    /// The process groups the unit's open tethers hold, read before anything moves.
    /// These are the teardown's first and most certain targets.
    ///
    /// Defaulted on the way in, because a reclaim journalled by an older Nodal has no
    /// such field and still has to be rebuilt and finished rather than refused.
    #[serde(default)]
    pub tethers: Vec<u32>,
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
    let prepared = prepare(store, request)?;
    let params = &prepared.params;
    let mut hooks_ran = Vec::new();
    hooks_ran.extend(prepared.hook(Phase::PreReclaim, params.environment.home.clone())?);
    let teardown = Arc::new(OnceLock::new());
    let released = Arc::new(OnceLock::new());
    run(store, &plan(params, &teardown, &released)?)?;
    hooks_ran.extend(prepared.hook(Phase::PostReclaim, prepared.after())?);
    Ok(report(&prepared, &teardown, &released, hooks_ran, verify(store, params)?))
}

/// The plan: tear the runtime down, then move the home. One registry write at the end.
///
/// The move is last on purpose. Every earlier step is something the unit's home does
/// not need to exist for, and a step that failed before the move leaves a home a person
/// can still open.
///
/// # Errors
/// [`Error::Render`] when the parameters cannot be written to the journal.
pub fn plan(
    params: &Params,
    teardown: &Arc<OnceLock<Teardown>>,
    released: &Arc<OnceLock<Released>>,
) -> Result<Plan> {
    let value = serde_json::to_value(params)
        .map_err(|source| Error::Render { kind: "operation parameters", source })?;
    let commit = commit_of(params, teardown, released);
    let plan = Plan::new(KIND, params.unit.slug.to_string(), value, commit).then(StopRuntime {
        unit: params.unit.id,
        environment: params.environment.clone(),
        tethers: params.tethers.clone(),
        report: Arc::clone(teardown),
    });
    let Some(entry) = &params.entry else { return Ok(plan) };
    Ok(plan.then(TrashHome { home: entry.home.clone(), path: entry.path.clone() }))
}

/// The registry write that finishes a reclaim.
///
/// Ports, leases and sessions are given up here rather than in a step, and that is the
/// difference between a port being free and a port being free *at the same moment* the
/// unit stops existing. A step that released them would leave a window in which the
/// registry says a reclaimed unit still holds a port, and a rolled-back reclaim would
/// have to take back a port another unit may already have been granted.
fn commit_of(
    params: &Params,
    teardown: &Arc<OnceLock<Teardown>>,
    released: &Arc<OnceLock<Released>>,
) -> Commit {
    let (unit, environment) = (params.unit.clone(), params.environment.clone());
    let entry = params.entry.clone();
    let released = Arc::clone(released);
    let teardown = Arc::clone(teardown);
    Box::new(move |tx: &Transaction<'_>| -> Result<()> {
        let now = Timestamp::now();
        let given = ports::release(tx, environment.id)?;
        let standing = still_standing(&teardown);
        for session in sessions::list_open(tx, environment.id)? {
            if session.pgid.is_some_and(|pgid| standing.contains(&pgid)) {
                continue;
            }
            sessions::end(tx, session.id, now)?;
        }
        environments::update_state(tx, environment.id, EnvState::Absent, now)?;
        units::update_status(tx, unit.id, UnitStatus::Archived, now)?;
        if let Some(entry) = &entry {
            trash::insert(tx, entry)?;
        }
        record(tx, &unit, &environment, entry.as_ref(), &given)?;
        let _ = released.set(given);
        Ok(())
    })
}

/// The process groups the teardown signalled and could not stop.
///
/// A tether's row is closed with the rest of the unit's sessions, except this one case.
/// An open row is the claim "this group is still the unit's to stop", and it is what
/// `nodal gc` acts on later. Closing the row of a group that is still running would
/// throw away the only record of it, so a group that survived every signal keeps its
/// row and appears in the report as a leftover as well.
fn still_standing(teardown: &Arc<OnceLock<Teardown>>) -> Vec<u32> {
    let Some(torn) = teardown.get() else { return Vec::new() };
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
    let mut refs = std::collections::BTreeMap::new();
    let mut reference = |name: &str, value: String| {
        if let Ok(name) = RefName::parse(name) {
            refs.insert(name, value);
        }
    };
    reference("ports", released.allocated.len().to_string());
    let body = match entry {
        Some(entry) => format!("reclaimed; home moved to {}", entry.path.display()),
        None => format!("unregistered; {} left in place", environment.home.display()),
    };
    if let Some(entry) = entry {
        reference("trash", entry.path.display().to_string());
        if let Some(snapshot) = &entry.snapshot {
            reference("snapshot", snapshot.clone());
        }
    }
    events::append(
        tx,
        &Event {
            id: EventId::from_ulid(ulid::Ulid::new()),
            unit: unit.id,
            environment: Some(environment.id),
            ts: Timestamp::now(),
            actor: actor::current()?,
            kind: EventKind::Note,
            epistemic: Epistemic::Observed,
            body,
            refs,
            raw_ref: None,
        },
    )
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
        plan(&params, &Arc::new(OnceLock::new()), &Arc::new(OnceLock::new()))
    }
}

// ---------------------------------------------------------------------------
// The steps.
// ---------------------------------------------------------------------------

/// What the teardown of a unit's runtime did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// Where the answer is left for the report. A step hands nothing to the step after
    /// it, and this hands nothing to one.
    report: Arc<OnceLock<Teardown>>,
}

impl Step for StopRuntime {
    fn key(&self) -> String {
        String::from("runtime.stop")
    }

    /// Repeatable: a second run finds nothing attributed and stops nothing.
    fn apply(&self) -> Result<()> {
        let mut seen = attributed(self.unit, &self.environment.home);
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
        let _ = self.report.set(Teardown { stopped, containers, notes: seen.notes });
        Ok(())
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
    /// then the processes a scan attributed.
    ///
    /// The order is the point. A group takes its whole tree with it, so a process the
    /// scan named is usually gone before its turn comes, and the stop reports what it
    /// actually did rather than counting the same server twice.
    fn targets(&self, seen: &Seen) -> Vec<Target> {
        let groups = self.tethers.iter().map(|pgid| Target::Group(*pgid));
        groups.chain(seen.pids.iter().map(|pid| Target::Process(*pid))).collect()
    }
}

/// What this machine can see of a unit: its processes, its containers, and whatever it
/// could not look at.
#[derive(Debug, Clone, Default)]
struct Seen {
    /// The processes standing in the home or carrying the unit's identifier.
    pids: Vec<u32>,
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
fn attributed(unit: UnitId, home: &Path) -> Seen {
    let mut seen = Seen::default();
    let placed = guard::resolve(home);
    match processes::Processes::scan(&processes::Live) {
        Ok(running) => {
            seen.pids = running
                .iter()
                .filter(|process| in_unit(process, unit, &placed))
                .map(|process| process.pid)
                .collect();
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

/// Whether a process belongs to this unit: it says so, or it is standing in the home.
///
/// `home` is the resolved form ([`guard::resolve`]), because the working directory the
/// kernel reports has every symbolic link on the way to it already taken out. A home
/// reached through a link — macOS reaches everything under `/var` that way, and so does
/// anyone whose state directory is a link — would otherwise match no process at all, and
/// a reclaim would quietly stop nothing and then quietly verify nothing.
fn in_unit(process: &processes::Running, unit: UnitId, home: &Path) -> bool {
    if process.var(crate::env::vars::ID) == Some(&unit.to_string()) {
        return true;
    }
    process.cwd.as_deref().is_some_and(|cwd| cwd.starts_with(home))
}

/// Move the home into the project's trash directory.
struct TrashHome {
    /// Where the home is.
    home: PathBuf,
    /// Where it goes.
    path: PathBuf,
}

impl Step for TrashHome {
    fn key(&self) -> String {
        String::from("home.trash")
    }

    /// Repeatable in each of the three states a killed run can leave: the home where it
    /// was, the home already in the trash, and neither of the two there at all.
    fn apply(&self) -> Result<()> {
        move_tree(&self.home, &self.path)
    }

    /// Move it back. This is why the trash is a move and not a delete: the operation's
    /// own recovery needs the directory to still exist.
    fn undo(&self) -> Result<()> {
        move_tree(&self.path, &self.home)
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
    fn hook(&self, phase: Phase, root: PathBuf) -> Result<Option<Ran>> {
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
        self.runner.run(phase, &directory, &context)
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
    let tethers = tethers(store.conn(), environment.id)?;
    let params = Params { project, unit, environment, entry, tethers };
    Ok(Prepared { params, findings, runner })
}

/// The process groups an environment's open tethers hold.
///
/// Read here, before the plan, so that the groups reach the journal: a reclaim killed
/// part-way and rebuilt from that journal stops the same groups the first attempt was
/// going to, and does not have to find them again in a registry it has since written.
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
    let seen = attributed(params.unit.id, &params.environment.home);
    let spared = stop::spared();
    for pid in seen.pids.iter().filter(|pid| !spared.contains(pid)) {
        leftovers.push(Leftover::new("process", pid.to_string()));
    }
    for name in seen.containers {
        leftovers.push(Leftover::new("container", name));
    }
    seen.notes
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
    teardown: &Arc<OnceLock<Teardown>>,
    released: &Arc<OnceLock<Released>>,
    hooks: Vec<Ran>,
    verified: (Vec<Leftover>, Vec<Note>),
) -> Reclaimed {
    let params = &prepared.params;
    let torn = teardown.get().cloned().unwrap_or_default();
    let (leftovers, mut notes) = verified;
    // A signal both halves failed to read says so once. The teardown's note and the
    // verification's are the same sentence about the same machine.
    for note in torn.notes {
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    Reclaimed {
        now: Timestamp::now(),
        slug: params.unit.slug.to_string(),
        findings: prepared.findings.clone(),
        snapshot: params.entry.as_ref().and_then(|entry| entry.snapshot.clone()),
        stopped: torn.stopped,
        containers: torn.containers,
        released: released.get().cloned().unwrap_or_default(),
        trashed: params.entry.clone(),
        root: root_of(params),
        hooks,
        notes,
        leftovers,
    }
}

/// The directory an adopted checkout was left at, when that is what this was.
fn root_of(params: &Params) -> Option<PathBuf> {
    if params.entry.is_some() || params.environment.managed {
        return None;
    }
    Some(params.environment.home.clone())
}
