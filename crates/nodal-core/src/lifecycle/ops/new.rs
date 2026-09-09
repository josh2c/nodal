//! `nodal new`: a unit, the branch it owns, and a home to work in.
//!
//! Everything this operation does already exists somewhere else. It clones a tree with
//! the backend the filesystem allows ([`crate::workspace`]), removes from the copy the
//! caches that record the path they were made at ([`crate::workspace::relocate`]),
//! scrubs the Git state the clone inherited ([`crate::git::Git::scrub`]), takes the
//! unit's branch, marks the home ([`crate::lifecycle::marker`]), writes the activation
//! files ([`crate::env::files`]) and grants ports ([`crate::services::ports`]). What
//! this file adds is the order, the undo of each part, and the single registry write at
//! the end.
//!
//! The tree it clones is a base ([`crate::substrate`]), never the person's checkout. A
//! checkout carries uncommitted work, and a home cloned from one is a home that starts
//! with somebody else's half-finished edit in it. So the base for the workspace comes
//! first, and the create clones that.
//!
//! Three rules decide the shape of it.
//!
//! The base is a **prerequisite, not a step**. Resolving it runs a plan of its own,
//! with its own journal entry and its own registry write, and only then is the create
//! planned. The two want opposite things of an interrupted run: a killed base build is
//! resumed, because throwing away a clone and an install because a laptop closed is
//! the whole cost this module exists to avoid, and a killed create is rolled back to
//! nothing, because a home no registry row knows about is the worst outcome there is.
//! A plan has one [`crate::lifecycle::Recovery`] and one registry write, so one plan
//! cannot hold both rules. Two plans in sequence hold them exactly: a create killed
//! anywhere is taken back and leaves the base it was going to clone standing, and a
//! base build killed anywhere is finished by the next invocation and the create that
//! wanted it is simply asked for again.
//!
//! A home that exists and that no registry row knows about is the worst outcome, so
//! nothing is written to the registry until every step has succeeded, and a run
//! interrupted by a kill rolls back to nothing on the next invocation
//! ([`crate::lifecycle::resolve`]). Every step here is therefore idempotent and has an
//! undo, and the undo of the first step — removing the home — is the one that does the
//! real work; the steps inside the home take themselves back where doing so is visible,
//! and say so where it is not.
//!
//! No secret value is ever held in a [`Plan`]. A plan is rebuilt from the journal, and
//! the journal is a table in the registry, so a plan that carried resolved values would
//! be a plan that wrote them there. The activation is therefore assembled inside the
//! step that writes it, from sources the step asks at the moment it runs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};

use crate::env::files;
use crate::env::secrets::MachineSecrets;
use crate::env::{Produced, resolve as resolve_env};
use crate::fingerprint;
use crate::git::{Git, scrub};
use crate::lifecycle::hooks::{self, Approvals, Context, Phase, Runner};
use crate::lifecycle::journal::Operation;
use crate::lifecycle::owner;
use crate::lifecycle::step::{Commit, Output, Outputs, Plan, Step, nothing};
use crate::lifecycle::{Rebuild, guard, marker, run};
use crate::model::{
    BranchName, EnvId, EnvState, Environment, Epistemic, EventKind, Objective, PortBlock, PortName,
    Ports, Project, ProjectId, ProjectName, Recipe, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use crate::output::view::{Arrival, Created};
use crate::services::ports;
use crate::store::{Store, environments, events, projects, units};
use crate::substrate::{self, Reporter};
use crate::workspace::relocate::{CacheRelocator, InvalidateCache};
use crate::workspace::{Excludes, Materializer, home, relocate, select_backend, tracked};
use crate::{Error, Result};

/// What this operation is called in the journal.
pub const KIND: &str = "new";

/// The prefix every unit's branch carries, so a unit's branch is recognisable in
/// `git branch` and in a pull request list.
pub const BRANCH_PREFIX: &str = "nodal/";

/// The port every unit is granted, whatever the project runs: the development server.
const APP_PORT: &str = "app";

/// The key of the step whose report the registry write reads. Named once, because the
/// step that writes it and the commit that reads it are two hundred lines apart and a
/// typo between them would be a silently missing event rather than a failure.
pub(super) const RELOCATE: &str = "home.relocate";

/// The key of the step that makes the clone, whose answer the report reads: which
/// default exclusion rows yielded to a path the project tracks.
pub(super) const MATERIALIZE: &str = "home.materialize";

/// Whether a base built for a create also runs the project's build command.
///
/// It does not. A create waits for its base, so what the base does is what the person
/// waits for, and installing dependencies is the part a unit cannot start without.
/// `nodal base build --warm` is where a person asks for the rest, before they want it.
const WARM_BUILD: bool = false;

/// How many words of a stated objective become the unit's handle. Two is what reads as
/// a name — `worker-import`, `payroll-export` — rather than as a sentence.
const SLUG_WORDS: usize = 2;

/// The handle a unit gets when nothing was stated to derive one from.
const DEFAULT_SLUG: &str = "unit";

/// How many handles are tried before a project is declared to have too many units of
/// one name. A person who has reached this has a naming problem, not a Nodal problem.
const SLUG_ATTEMPTS: u32 = 1_000;

/// What a person asked `nodal new` for.
#[derive(Debug, Clone)]
pub struct Request {
    /// A directory in the project. The repository holding it is read to decide which
    /// base the unit wants; it is never what the home is cloned from.
    pub source: PathBuf,
    /// What the unit is for, as it was stated.
    pub objective: Option<Objective>,
    /// How that objective is known. [`Epistemic::Stated`] is what a person typing one
    /// means; [`Epistemic::Observed`] is what an adapter recovering one from an agent's
    /// own records means, and a person reading the unit later is told which they have
    /// ([`crate::model::Unit::objective_epistemic`]).
    pub objective_epistemic: Epistemic,
    /// The handle to use, when a person chose one instead of letting it be derived.
    pub name: Option<Slug>,
    /// The branch the work starts from, when it is known.
    pub parent_branch: Option<BranchName>,
    /// Whether the project's own hooks run. `false` is `--no-hooks`.
    pub hooks: bool,
}

impl Default for Request {
    /// A request that runs the project's hooks, because that is what a person who
    /// wrote one into their recipe asked for. `--no-hooks` is the exception and is
    /// stated.
    fn default() -> Self {
        Self {
            source: PathBuf::new(),
            objective: None,
            objective_epistemic: Epistemic::Stated,
            name: None,
            parent_branch: None,
            hooks: true,
        }
    }
}

/// Everything the plan is built from, and the whole of what the journal keeps.
///
/// The rows the operation will write are in here as values rather than as the
/// ingredients of values. That is what makes a rebuilt plan the same plan: the
/// identifiers, the home path and the instant are decided once, before the first step
/// runs, and a process that finds the run in the journal reads them rather than
/// generating them again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    /// The project the unit belongs to. Its row is written before the operation starts.
    pub project: Project,
    /// The project's effective recipe, which decides the exclusions and the ports.
    pub recipe: Recipe,
    /// Where the base is: the tree the home is a clone of. *Which* base it is, is the
    /// environment row's `base_id`, so the two cannot disagree.
    pub base_path: PathBuf,
    /// Where Nodal keeps its state on this machine.
    pub state_dir: PathBuf,
    /// The unit row the operation ends by writing.
    pub unit: Unit,
    /// The environment row it writes beside it, before its ports are granted.
    pub environment: Environment,
    /// The block this project's ports come from.
    pub block: PortBlock,
    /// The names ports are granted under.
    pub ports: Vec<PortName>,
}

/// Create a unit: choose its name, place its home, run the plan, and report the result.
///
/// `progress` is where the base build says what it is doing, on the first create of a
/// workspace. Every later create finds the base warm and reports one line.
///
/// # Errors
/// [`Error::UnitBranchHeld`] when another open unit already holds the branch,
/// [`Error::InsideSource`] when the home would overlap a tree Nodal knows,
/// [`Error::OperationStep`] when a step failed, in which case the steps before it were
/// undone, and whatever Git, the filesystem or the registry reported.
pub fn create(
    store: &mut Store,
    request: &Request,
    progress: &Arc<dyn Reporter>,
) -> Result<Created> {
    let params = prepare(store, request, progress)?;
    let environment = params.environment.id;
    let runner = hooks_of(&params, request.hooks)?;
    let context = context_of(&params);
    runner.run(Phase::PreNew, &params.project.root, &context)?;
    let done = run(store, &plan(&params)?)?;
    runner.run(Phase::PostNew, &params.environment.home, &context)?;
    let created = Created::of(
        &params.unit,
        &read_back(store, environment)?,
        Arrival::Created,
        Timestamp::now(),
    )?;
    Ok(created.keeping(done.outputs.read(MATERIALIZE)?.unwrap_or_default()))
}

/// The project's hooks and this machine's approvals for them.
///
/// A hook is not a step and cannot be one ([`crate::lifecycle::hooks`]), so `pre_new`
/// runs after every refusal has been made and before the first step, and `post_new`
/// after the registry write. `pre_new` runs in the project root, because the home it
/// is about does not exist yet; `NODAL_ROOT` names the home it is going to be.
fn hooks_of(params: &Params, enabled: bool) -> Result<Runner> {
    Ok(Runner {
        project: params.project.root.clone(),
        hooks: params.recipe.hooks.clone(),
        approvals: Approvals::open(hooks::path_in(&params.state_dir))?,
        enabled,
    })
}

/// The `NODAL_*` context both hooks of a create are given.
fn context_of(params: &Params) -> Context {
    Context {
        source: params.project.root.clone(),
        root: params.environment.home.clone(),
        unit: params.unit.id,
        slug: params.unit.slug.clone(),
        branch: params.unit.branch.clone(),
        parent: params.environment.base_id.map(|base| base.to_string()),
        environment: params.environment.id,
    }
}

/// Work out what the operation will do, and get the base it will clone.
///
/// Order matters here in one way that is not obvious from reading it. Every refusal —
/// a held branch, a home that would overlap a tree Nodal knows — is made before the
/// base is asked for, because asking for a base may build one, and a build is minutes.
/// Nothing that can refuse the create runs after that call.
fn prepare(store: &mut Store, request: &Request, progress: &Arc<dyn Reporter>) -> Result<Params> {
    let git = Git::open(&request.source)?;
    git.ensure_no_operation_in_progress()?;
    let source = git.top_level()?;
    let effective = crate::recipe::load(&source)?;
    let project = ensure_project(store, &source, &effective.recipe)?;
    let block = ports::ensure_block(store, project.id)?;

    let name =
        choose_name(store.conn(), project.id, request.name.as_ref(), request.objective.as_ref())?;
    let branch = branch_of(&name.branch)?;
    refuse_held_branch(store.conn(), project.id, &branch)?;

    let state_dir = home::directory()?;
    let unit = new_unit(project.id, &name.slug, &branch, request);
    let mut environment = new_environment(unit.id, &project.name, &state_dir, unit.created_at);
    guard::placement(store.conn(), &environment.home, &source)?;

    let wanted = substrate::Request {
        project: project.clone(),
        source,
        recipe: effective.recipe.clone(),
        state_dir: state_dir.clone(),
        warm: WARM_BUILD,
    };
    let base = substrate::ensure(store, &wanted, progress)?;
    environment.base_id = Some(base.base.id);
    environment.ws_fp_materialized = Some(base.fingerprint);

    Ok(Params {
        ports: port_names(&effective.recipe),
        recipe: effective.recipe,
        project,
        base_path: base.base.path,
        state_dir,
        unit,
        environment,
        block,
    })
}

/// The plan: seven steps in the home, then one registry write.
///
/// The default exclusion rows that yielded to a path the project tracks are the clone
/// step's own output ([`MATERIALIZE`]), because the report is written after the plan has
/// run and the process that writes it may not be the one that made the clone.
///
/// # Errors
/// [`Error::Render`] when the parameters cannot be written to the journal.
pub fn plan(params: &Params) -> Result<Plan> {
    let home = params.environment.home.clone();
    let value = serde_json::to_value(params)
        .map_err(|source| Error::Render { kind: "operation parameters", source })?;
    Ok(Plan::new(KIND, params.unit.slug.to_string(), value, commit_of(params))
        .then(Materialize {
            base: params.base_path.clone(),
            home: home.clone(),
            excludes: Excludes::with_recipe(&params.recipe.base.exclude),
            backend: select_backend(&params.state_dir),
        })
        .then(Relocate {
            home: home.clone(),
            base: params.base_path.clone(),
            relocator: InvalidateCache::with_recipe(&params.recipe.base.invalidate),
        })
        .then(Scrub { home: home.clone() })
        .then(TakeBranch {
            home: home.clone(),
            branch: params.unit.branch.clone(),
            start: params.unit.parent_branch.clone(),
        })
        .then(files::Hide { home: home.clone() })
        .then(marker::WriteMarker { home, unit: params.unit.id })
        .then(Activate {
            project: params.project.clone(),
            unit: params.unit.clone(),
            environment: params.environment.clone(),
            recipe: params.recipe.clone(),
            state_dir: params.state_dir.clone(),
        }))
}

/// The registry write that finishes a create.
///
/// The unit row is what the branch rule is enforced on: a second open unit on the same
/// branch is refused here by the partial unique index, whatever two racing processes
/// each read a moment earlier. The ports are granted inside the same transaction,
/// because a grant names the environment row and cannot be made before it exists.
///
/// The relocation event is written here for the same reason and not by the step that
/// did the removal: an event names a unit, and no unit row exists until this runs. What
/// the removal found reaches this through the journal ([`RELOCATE`]), so a create
/// resumed by a later process writes the same event the first run would have.
fn commit_of(params: &Params) -> Commit {
    let (unit, environment) = (params.unit.clone(), params.environment.clone());
    let (block, names) = (params.block, params.ports.clone());
    Box::new(move |tx: &Transaction<'_>, outputs: &Outputs| -> Result<Output> {
        units::insert(tx, &unit)?;
        environments::insert(tx, &environment)?;
        let granted = ports::allocate(tx, block, environment.id, &names)?;
        environments::set_ports(tx, environment.id, &granted)?;
        record_relocation(tx, unit.id, environment.id, outputs.read(RELOCATE)?.as_ref())?;
        Ok(nothing())
    })
}

/// Write down that a cache was invalidated, so that a later `nodal explain` can say why
/// a build in this home started cold.
///
/// A relocation that removed nothing is not an event. The log is what a person reads to
/// understand a unit, and a line saying that nothing happened is noise in it.
pub(super) fn record_relocation(
    tx: &Transaction<'_>,
    unit: UnitId,
    environment: EnvId,
    report: Option<&relocate::Report>,
) -> Result<()> {
    let Some(report) = report.filter(|report| !report.changed_nothing()) else {
        return Ok(());
    };
    events::note(
        tx,
        (unit, Some(environment)),
        EventKind::Note,
        report.describe(),
        &[
            ("relocator", report.relocator.clone()),
            ("from", report.from.display().to_string()),
            ("to", report.to.display().to_string()),
            ("removed", report.removed.len().to_string()),
        ],
    )
}

/// Finding an interrupted create again, from what the journal kept.
pub struct New;

impl Rebuild for New {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn rebuild(&self, record: &Operation) -> Result<Plan> {
        let params: Params = serde_json::from_value(record.params.clone()).map_err(|_| {
            Error::InvalidValue { kind: "create parameters", value: record.id.to_string() }
        })?;
        // A resumed run finishes the home and writes no report, so the notes it would
        // have carried have nowhere to go.
        plan(&params)
    }
}

// ---------------------------------------------------------------------------
// The steps.
// ---------------------------------------------------------------------------

/// Clone the base into the home.
pub(super) struct Materialize {
    /// The base being cloned. A warm tree at the workspace's commit, with the
    /// dependencies installed and nothing uncommitted in it.
    pub(super) base: PathBuf,
    /// Where it is cloned to.
    pub(super) home: PathBuf,
    /// What the clone leaves out.
    pub(super) excludes: Excludes,
    /// The one backend chosen for this operation, before it started.
    pub(super) backend: Box<dyn Materializer>,
}

impl Step for Materialize {
    fn key(&self) -> String {
        String::from(MATERIALIZE)
    }

    /// A clone refuses a destination that is already there, and a run killed part-way
    /// through one leaves exactly that. So the destination is removed first: what is
    /// under it is this operation's own half-made home and nothing else.
    ///
    /// The exclusion list is settled before that removal, because a list that would
    /// drop a tracked path the project wrote stops the operation and must not first take
    /// away the home a resumed run would find. A default row the commit tracks yields
    /// instead, and the rows that yielded are what this step reports.
    ///
    /// Repeatable, and the same answer twice: the rows a tree tracks do not change
    /// between two attempts at the same clone.
    fn apply(&self) -> Result<Output> {
        let mut excludes = self.excludes.clone();
        let kept = tracked::enforce(&self.base, &mut excludes)?;
        remove_tree(&self.home)?;
        if let Some(parent) = self.home.parent() {
            std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        self.backend.clone_tree(&self.base, &self.home, &excludes)?;
        serde_json::to_value(kept)
            .map_err(|source| Error::Render { kind: "kept exclusion rows", source })
    }

    fn undo(&self) -> Result<()> {
        remove_tree(&self.home)
    }
}

/// Remove the caches the copy cannot use at the path it is now at.
///
/// A fresh unit must not carry a cache keyed to the base's path, and the base is where
/// such a cache comes from: the install and the build that made it ran there. The
/// exclusion list keeps one out of the clone where it sits at the root of the tree;
/// this step is what finds the ones under a second package, and it is what reports the
/// removal so that the operation can record it.
pub(super) struct Relocate {
    /// The home that was cloned.
    pub(super) home: PathBuf,
    /// The base it was cloned from, which is the path its content was made at.
    pub(super) base: PathBuf,
    /// What is removed, and why.
    pub(super) relocator: InvalidateCache,
}

impl Step for Relocate {
    fn key(&self) -> String {
        String::from(RELOCATE)
    }

    /// The report goes to the commit through the journal, because the commit is the
    /// only place a unit row exists for the event to name.
    ///
    /// Repeatable, and the same answer twice: a second sweep of a home whose caches
    /// have gone removes nothing and reports nothing, so a resumed run reads the first
    /// run's report out of the journal rather than overwriting it with an empty one.
    fn apply(&self) -> Result<Output> {
        let report = self.relocator.relocate(&self.home, &self.base, &self.home)?;
        if report.changed_nothing() {
            return Ok(nothing());
        }
        tracing::info!(
            home = %self.home.display(),
            removed = report.removed.len(),
            examined = report.examined,
            "removed caches that record the path they were made at"
        );
        serde_json::to_value(report)
            .map_err(|source| Error::Render { kind: "relocation report", source })
    }

    /// Nothing. What this removed was inside a directory the first step's undo removes,
    /// and a cache that cannot be used is not something an undo owes anybody back.
    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

/// Take the Git state the clone inherited from its source out of the copy.
pub(super) struct Scrub {
    /// The home that was cloned.
    pub(super) home: PathBuf,
}

impl Step for Scrub {
    fn key(&self) -> String {
        String::from("git.scrub")
    }

    fn apply(&self) -> Result<Output> {
        Git::open(&self.home)?.scrub(&scrub::Options::default())?;
        Ok(nothing())
    }

    /// Nothing. What this changed is inside a directory the first step's undo removes,
    /// and putting another repository's worktree registrations back into a copy that is
    /// about to be deleted would be work with no observer.
    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

/// Create the unit's branch and check it out.
pub(super) struct TakeBranch {
    /// The home whose repository the branch is created in.
    pub(super) home: PathBuf,
    /// The branch the unit owns.
    pub(super) branch: BranchName,
    /// The branch the work starts from. `None` starts it where the clone's HEAD is,
    /// which is the branch the project was on when it was cloned.
    pub(super) start: Option<BranchName>,
}

impl Step for TakeBranch {
    fn key(&self) -> String {
        String::from("git.branch")
    }

    /// Repeatable in each of the three states a killed run can leave: the branch not
    /// there, the branch there but not checked out, and the branch already checked out.
    fn apply(&self) -> Result<Output> {
        let git = Git::open(&self.home)?;
        let name = self.branch.as_str();
        if git.current_branch()?.as_deref() == Some(name) {
            return Ok(nothing());
        }
        if git.branch_exists(name)? {
            git.switch(name)?;
            return Ok(nothing());
        }
        git.switch_new(name, self.start.as_ref().map(BranchName::as_str))?;
        Ok(nothing())
    }

    /// Nothing, for the reason [`Scrub::undo`] gives: the branch exists only in the
    /// repository the first step's undo removes.
    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

/// Assemble the home's environment and write the files that deliver it.
pub(super) struct Activate {
    /// The project, for the identity variables.
    pub(super) project: Project,
    /// The unit, for the same.
    pub(super) unit: Unit,
    /// The materialisation, for the same.
    pub(super) environment: Environment,
    /// What the project declares it needs.
    pub(super) recipe: Recipe,
    /// Where the per-machine secrets file lives.
    pub(super) state_dir: PathBuf,
}

impl Step for Activate {
    fn key(&self) -> String {
        String::from("env.activate")
    }

    /// The sources are asked here rather than while the plan is built, so that no
    /// resolved value is ever part of a plan or of what the journal keeps.
    ///
    /// A declared name nothing answers is a line of the report, never a failure, and a
    /// name the recipe expects a service to generate is one of those until the task
    /// that starts the services fills it in.
    fn apply(&self) -> Result<Output> {
        let machine = MachineSecrets::open(MachineSecrets::path_in(&self.state_dir))?;
        let subject = (&self.unit, &self.environment, &self.project);
        let activation = resolve_env(subject, &self.recipe, &Produced::default(), &[&machine])?;
        let manifest = activation.manifest(&self.unit, &self.environment, &self.project);
        files::write(&self.environment.home, &activation, &manifest)?;
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        files::remove(&self.environment.home)
    }
}

/// Remove a directory and everything under it. Removing one that is not there is not a
/// failure: an undo runs against a world it may never have changed.
pub(super) fn remove_tree(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

// ---------------------------------------------------------------------------
// What the plan is built from.
// ---------------------------------------------------------------------------

/// The project at `root`, recorded if this is the first time Nodal has seen it.
///
/// A project row is not part of the create and is never rolled back with one: it is a
/// fact about this machine, like the block of ports the project hands out from, and it
/// outlives every unit made in it.
///
/// Public because `nodal base` needs the same row before it can key a base, and one
/// definition of "which project is this" is the point: two would let a base and the
/// unit cloned from it belong to different projects.
///
/// The root is recorded with its symbolic links resolved ([`guard::resolve`]), because
/// a project is a tree and not a name for one. A command run in the project reaches the
/// row by the name the operating system gives a running process, which on macOS is the
/// resolved one whatever the person typed.
///
/// # Errors
/// [`Error::Render`] when the recipe could not be digested, and whatever the registry
/// reports.
pub fn ensure_project(store: &mut Store, root: &Path, recipe: &Recipe) -> Result<Project> {
    let root = guard::resolve(root);
    let root = root.as_path();
    if let Some(found) = projects::find_by_root(store.conn(), root)? {
        return Ok(found);
    }
    let fresh = Project {
        id: ProjectId::from_ulid(ulid::Ulid::new()),
        root: root.to_path_buf(),
        name: name_of(root),
        recipe_hash: fingerprint::compute_recipe(recipe)?,
        created_at: Timestamp::now(),
    };
    let tx = store.transaction()?;
    let project = if let Some(found) = projects::find_by_root(&tx, root)? {
        found
    } else {
        projects::insert(&tx, &fresh)?;
        fresh
    };
    tx.commit().map_err(crate::store::row::store_error(store.conn()))?;
    Ok(project)
}

/// What a project is called: the name of the directory it is rooted at.
///
/// Public because a project can be named before it is recorded: a directory that holds
/// a `nodal.toml` and no units yet is a project a person can be told about, and it must
/// be told about under the name the first `nodal new` will give it
/// ([`crate::runtime::entry::declared_at`]).
#[must_use]
pub fn name_of(root: &Path) -> ProjectName {
    root.file_name()
        .and_then(|name| ProjectName::parse(name.to_string_lossy()).ok())
        .unwrap_or_else(|| ProjectName::parse("project").unwrap_or_else(|_| unreachable!()))
}

/// The handle a unit is given and the name its branch is built from.
pub(super) struct Name {
    /// The handle, unique among every unit of the project.
    pub(super) slug: Slug,
    /// The name the person asked for, which the branch carries even when the handle had
    /// to be given a suffix to stay unique.
    pub(super) branch: String,
}

/// Choose the unit's handle, and the name its branch is built from.
///
/// A handle a person chose is used as it is; one derived from a line of text is given a
/// suffix until it is free. The branch never takes the suffix: two units of one name
/// are two attempts at the same work, and the second's refusal has to name the first.
///
/// `derive_from` is what the handle is made of when nobody chose one. A create passes
/// the objective; an adoption passes the branch, because the branch is the name every
/// other tool already shows for that work ([`super::adopt`]).
pub(super) fn choose_name(
    conn: &rusqlite::Connection,
    project: ProjectId,
    chosen: Option<&Slug>,
    derive_from: Option<&Objective>,
) -> Result<Name> {
    let asked = match chosen {
        Some(name) => name.clone(),
        None => derive_slug(derive_from)?,
    };
    let branch = asked.to_string();
    for attempt in 1..=SLUG_ATTEMPTS {
        let slug = suffixed(&asked, attempt)?;
        if units::find_by_slug(conn, project, &slug)?.is_none() {
            return Ok(Name { slug, branch });
        }
    }
    Err(Error::InvalidValue { kind: "unit handle", value: branch })
}

/// The handle with its attempt number, which the first attempt does not carry.
fn suffixed(slug: &Slug, attempt: u32) -> Result<Slug> {
    if attempt == 1 {
        return Ok(slug.clone());
    }
    Slug::parse(format!("{slug}-{attempt}"))
}

/// A handle from a stated objective: its first two words.
///
/// `"worker import: handle missing supervisor_id"` becomes `worker-import`. Anything
/// that is not a letter or a digit separates words, so punctuation ends one rather than
/// joining two.
fn derive_slug(objective: Option<&Objective>) -> Result<Slug> {
    let text = objective.map(Objective::as_str).unwrap_or_default();
    let words: Vec<String> = text
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(SLUG_WORDS)
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() {
        return Slug::parse(DEFAULT_SLUG);
    }
    Slug::parse(words.join("-"))
}

/// The branch a name owns.
pub(super) fn branch_of(name: &str) -> Result<BranchName> {
    BranchName::parse(format!("{BRANCH_PREFIX}{name}"))
}

/// Refuse a branch another open unit holds, and say which one.
///
/// The rule itself is the registry's partial unique index, and that is what decides a
/// race between two creates. This read is what turns the rule into an answer: a person
/// told only that a write conflicted still has to go and find out who has the branch.
pub(super) fn refuse_held_branch(
    conn: &rusqlite::Connection,
    project: ProjectId,
    branch: &BranchName,
) -> Result<()> {
    match units::find_open_by_branch(conn, project, branch)? {
        Some(holder) => Err(Error::UnitBranchHeld {
            branch: branch.clone(),
            slug: holder.slug,
            unit: holder.id,
        }),
        None => Ok(()),
    }
}

/// The unit row a create will write.
fn new_unit(project: ProjectId, slug: &Slug, branch: &BranchName, request: &Request) -> Unit {
    let now = Timestamp::now();
    Unit {
        id: UnitId::from_ulid(ulid::Ulid::new()),
        project_id: project,
        slug: slug.clone(),
        objective: request.objective.clone(),
        objective_epistemic: request.objective.as_ref().map(|_| request.objective_epistemic),
        branch: branch.clone(),
        parent_branch: request.parent_branch.clone(),
        status: UnitStatus::Open,
        created_at: now,
        updated_at: now,
    }
}

/// The environment row a create will write, before its ports are granted.
pub(super) fn new_environment(
    unit: UnitId,
    project: &ProjectName,
    state_dir: &Path,
    at: Timestamp,
) -> Environment {
    let id = EnvId::from_ulid(ulid::Ulid::new());
    Environment {
        id,
        unit_id: unit,
        attempt: 1,
        home: home::in_directory(state_dir, project, id),
        managed: true,
        base_id: None,
        ws_fp_materialized: None,
        schema_fp_materialized: None,
        host: owner::current_host(),
        db_name: None,
        ports: Ports::default(),
        fixed_port: None,
        state: EnvState::Stopped,
        created_at: at,
        last_active: at,
    }
}

/// The names a unit is granted ports under: the development server, and one for each
/// service the project gives every unit its own copy of.
pub(super) fn port_names(recipe: &Recipe) -> Vec<PortName> {
    let mut names = Vec::with_capacity(recipe.services.per_unit.len() + 1);
    names.extend(PortName::parse(APP_PORT));
    for service in &recipe.services.per_unit {
        names.extend(PortName::parse(service.as_str()));
    }
    names
}

/// The environment row as the commit left it, with its ports.
pub(super) fn read_back(store: &Store, id: EnvId) -> Result<Environment> {
    environments::get(store.conn(), id)?
        .ok_or_else(|| Error::StoreMissingRow { table: "environment", id: id.to_string() })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{DEFAULT_SLUG, branch_of, derive_slug, port_names, suffixed};
    use crate::model::{Objective, Recipe, ServiceName, Slug};

    fn objective(text: &str) -> Objective {
        Objective::parse(text).unwrap()
    }

    #[test]
    fn a_handle_is_the_first_two_words_of_what_was_stated() {
        let stated = objective("worker import: handle missing supervisor_id");
        assert_eq!(derive_slug(Some(&stated)).unwrap().as_str(), "worker-import");
        assert_eq!(
            derive_slug(Some(&objective("payroll export CSV"))).unwrap(),
            Slug::parse("payroll-export").unwrap()
        );
    }

    #[test]
    fn nothing_stated_still_gives_a_handle() {
        assert_eq!(derive_slug(None).unwrap().as_str(), DEFAULT_SLUG);
        assert_eq!(derive_slug(Some(&objective("!!! ???"))).unwrap().as_str(), DEFAULT_SLUG);
    }

    #[test]
    fn the_second_unit_of_a_name_is_suffixed_and_the_first_is_not() {
        let slug = Slug::parse("worker-import").unwrap();
        assert_eq!(suffixed(&slug, 1).unwrap(), slug);
        assert_eq!(suffixed(&slug, 2).unwrap().as_str(), "worker-import-2");
    }

    #[test]
    fn a_branch_carries_the_prefix_that_makes_it_recognisable() {
        assert_eq!(branch_of("worker-import").unwrap().as_str(), "nodal/worker-import");
    }

    #[test]
    fn a_unit_gets_a_port_for_the_server_and_one_for_each_service_of_its_own() {
        let mut recipe = Recipe::default();
        assert_eq!(port_names(&recipe).len(), 1);
        recipe.services.per_unit = vec![ServiceName::parse("postgrest").unwrap()];
        let names: Vec<String> = port_names(&recipe).iter().map(ToString::to_string).collect();
        assert_eq!(names, ["app", "postgrest"]);
    }
}
