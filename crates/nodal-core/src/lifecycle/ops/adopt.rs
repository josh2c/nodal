//! `nodal adopt`: a checkout that is already there, or a branch that is not.
//!
//! Every unit `nodal new` makes is one Nodal placed. The work on a real machine is not
//! like that. It is in twelve directories another tool made over three weeks, on
//! branches nobody named twice, and the reason it is hard to clean up is not the disk:
//! it is that nothing says what any of them was for. Adoption is how that work becomes
//! units without being moved, copied or interrupted.
//!
//! There are two forms, and which one runs is decided by what the target already is.
//!
//! **In place.** A checkout or a linked worktree becomes a unit *where it stands*. The
//! only writes are `.nodal/` and `.envrc`, and both are put in the checkout's own
//! `.git/info/exclude` before either is written, so `git status` in that directory says
//! exactly what it said the moment before. Nothing is cloned, no branch is created and
//! no file of the person's is touched. The environment row is written with
//! `managed = false`, which is what makes the directory a **root**: a reclaim
//! unregisters it and never moves it ([`super::reclaim`]), because Nodal did not create
//! it and it is not Nodal's to trash. `--all` runs this form for every worktree of the
//! project except the main checkout and any worktree that is already a unit.
//!
//! **Materialised.** A branch nothing has checked out gets a home of its own, made the
//! way [`super::new`] makes one: a clone of a warm base, the caches that record their
//! own path removed, the Git state of the copy scrubbed, and then the branch — which
//! already exists, so it is fetched from the project's own checkout rather than
//! created. That fetch is the one step this operation adds to a create, and it is what
//! makes the home carry a branch the base has never heard of.
//!
//! A branch that *is* checked out somewhere is refused rather than materialised
//! ([`crate::Error::AdoptBranchCheckedOut`]). A second home for it would leave whatever
//! is uncommitted in that checkout behind, and the whole point of the command is to not
//! lose it.
//!
//! # Recovering what a directory was for
//!
//! A worktree an agent tool made carries no statement of intent, and that is the fact
//! that makes a machine hard to clean up. Claude Code does keep one, in the opening
//! prompt of the session that ran there, and [`crate::doctor::intent`] already reads
//! it: `nodal doctor` prints it beside every worktree it finds. Adoption uses
//! that same seam rather than a second reader of the same files, and the answer becomes
//! the unit's objective.
//!
//! It is recorded as **recovered, not stated**. A recovered line is a reading of
//! somebody's opening prompt — it may be stale, it may be one of five things that
//! session went on to do — and a person deciding three weeks later what to do with the
//! unit needs to be told which of the two they are reading. That is what
//! [`crate::model::Unit::objective_epistemic`] carries, and `nodal ls`, `nodal show`
//! and the unit's own log all say it.
//!
//! # What an adoption does not do
//!
//! Adoption **in place** runs no recipe hook at all. The directory it is given is one
//! Nodal did not make, and a hook that installs into a person's working checkout is not
//! something a command that promises to change nothing may run.
//!
//! The **materialised** form runs `post_new`, exactly as `nodal new` does. It made a
//! home from a base, and that is the home a recipe's `post_new` is declared about.
//! Neither form runs `pre_new`: it is the hook for the moment before a home exists, and
//! neither form has such a moment to offer. The hook contract (`docs/contracts.md`)
//! states both sentences.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};

use crate::env::files;
use crate::git::Git;
use crate::lifecycle::hooks::{self, Approvals, Context, Phase, Runner};
use crate::lifecycle::journal::Operation;
use crate::lifecycle::ops::new;
use crate::lifecycle::step::{Commit, Output, Outputs, Plan, Step, nothing};
use crate::lifecycle::{Rebuild, guard, marker, run};
use crate::model::{
    BranchName, EnvId, Environment, Epistemic, EventKind, HostName, Objective, PortBlock, PortName,
    Project, Recipe, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use crate::output::view::{AdoptedAll, AdoptedRow, Arrival, Created};
use crate::services::ports;
use crate::store::{Store, environments, events, units};
use crate::substrate::{self, Reporter};
use crate::workspace::relocate::{self, InvalidateCache};
use crate::workspace::sharing::Sharing;
use crate::workspace::{Excludes, home, select_backend};
use crate::{Error, Result};

/// What this operation is called in the journal.
pub const KIND: &str = "adopt";

/// Whether a base built for an adoption also runs the project's build command. It does
/// not, for the reason [`super::new`] gives: an adoption waits for its base.
const WARM_BUILD: bool = false;

/// How many characters of a recovered prompt become the objective. A prompt is a
/// paragraph and an objective is a line; this is the width of the line, and what is cut
/// is marked so that nobody reads a truncated sentence as the whole of what was asked.
const OBJECTIVE_WIDTH: usize = 160;

/// What is put at the end of a recovered prompt that was longer than the line.
const ELLIPSIS: char = '…';

/// What a person asked `nodal adopt` for.
#[derive(Debug, Clone)]
pub struct Request {
    /// The branch or the directory that was named.
    pub target: String,
    /// A directory in the project. The repository holding it says which project this
    /// is, exactly as it does for a create.
    pub cwd: PathBuf,
    /// Whether the checkout becomes a unit where it stands. A directory can be adopted
    /// no other way, so this is stated rather than inferred: a person who typed a path
    /// is told what the flag means instead of having a checkout adopted under them.
    pub in_place: bool,
    /// What the unit is for, when a person states it. A stated objective is never
    /// replaced by a recovered one.
    pub objective: Option<Objective>,
    /// The handle to use, when a person chose one.
    pub name: Option<Slug>,
    /// Where the session records that may hold the intent are, as
    /// [`crate::doctor::intent::config_directory`] answers. `None` recovers nothing,
    /// which is what a machine with no agent tool on it gives.
    pub sessions: Option<PathBuf>,
    /// Whether the project's own hooks run. `false` is `--no-hooks`. It reaches only
    /// the materialised form; adoption in place runs no hook whatever this says.
    pub hooks: bool,
}

/// Where the home of an adopted unit comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "form")]
pub enum Source {
    /// The checkout was already there and stays exactly where it is.
    InPlace,
    /// The home is a fresh clone of a base, and the branch comes from the project's own
    /// checkout.
    Materialized {
        /// The base that is cloned.
        base_path: PathBuf,
        /// The checkout the branch is fetched from.
        from: PathBuf,
    },
}

/// Everything the plan is built from, and the whole of what the journal keeps.
///
/// The same rule [`super::new::Params`] follows: every identifier, path and instant is
/// decided before the first step runs, so a process that finds the run in the journal
/// rebuilds the plan it was following rather than a plan of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    /// The project the unit belongs to.
    pub project: Project,
    /// The project's effective recipe.
    pub recipe: Recipe,
    /// Where Nodal keeps its state on this machine.
    pub state_dir: PathBuf,
    /// The unit row the operation ends by writing.
    pub unit: Unit,
    /// The environment row beside it. `managed` is `false` for a checkout adopted in
    /// place, and that one field is what makes the directory a root.
    pub environment: Environment,
    /// The block this project's ports come from.
    pub block: PortBlock,
    /// The names ports are granted under.
    pub ports: Vec<PortName>,
    /// Where the home comes from.
    pub source: Source,
    /// Whether the objective was recovered rather than stated, which the unit's log
    /// says in words.
    pub recovered: bool,
}

/// Adopt a checkout or a branch, and report the unit it became.
///
/// `progress` is where a base build says what it is doing, which only the materialised
/// form can reach.
///
/// # Errors
/// [`Error::AdoptProjectRoot`], [`Error::AdoptAlreadyAUnit`], [`Error::AdoptDetached`],
/// [`Error::AdoptNeedsInPlace`] and [`Error::AdoptBranchCheckedOut`] for a target that
/// cannot become a unit, [`Error::UnitBranchHeld`] when another open unit already holds
/// the branch, [`Error::OperationStep`] when a step failed, in which case the steps
/// before it were undone, and whatever Git, the filesystem or the registry reported.
pub fn adopt(
    store: &mut Store,
    request: &Request,
    progress: &Arc<dyn Reporter>,
) -> Result<Created> {
    let params = prepare(store, request, progress)?;
    let environment = params.environment.id;
    let done = run(store, &plan(&params)?)?;
    post_new(&params, request.hooks)?;
    let arrival = match params.source {
        Source::InPlace => Arrival::AdoptedInPlace,
        Source::Materialized { .. } => Arrival::Adopted,
    };
    let created =
        Created::of(&params.unit, &new::read_back(store, environment)?, arrival, Timestamp::now())?;
    Ok(created.keeping(done.outputs.read(new::MATERIALIZE)?.unwrap_or_default()))
}

/// Adopt every worktree of the project, one at a time, through the ordinary adopt path.
///
/// The main checkout and a worktree that is already a unit are skipped, and the report
/// says so. Every other row is the existing adopt of that directory. `--in-place` is
/// required: these are checkouts somebody is already working in.
///
/// # Errors
/// [`Error::AdoptAllNeedsInPlace`] when `--in-place` was not given, and whatever Git,
/// the filesystem or the registry reported before any row could be considered.
pub fn adopt_all(
    store: &mut Store,
    request: &Request,
    progress: &Arc<dyn Reporter>,
) -> Result<AdoptedAll> {
    if !request.in_place {
        return Err(Error::AdoptAllNeedsInPlace);
    }
    let git = Git::open(&request.cwd)?;
    git.ensure_no_operation_in_progress()?;
    let root = main_checkout(&git)?;
    let listed = git.worktrees()?;
    let mut rows = Vec::with_capacity(listed.len());
    for (index, registered) in listed.into_iter().enumerate() {
        let main = index == 0 || same_tree(&registered.path, &root);
        rows.push(adopt_one(store, request, progress, registered.path, main));
    }
    Ok(AdoptedAll { now: Timestamp::now(), rows })
}

/// Adopt one worktree, or record why it was skipped or refused.
fn adopt_one(
    store: &mut Store,
    request: &Request,
    progress: &Arc<dyn Reporter>,
    path: PathBuf,
    main: bool,
) -> AdoptedRow {
    if main {
        return AdoptedRow::skipped(path, "the main checkout");
    }
    let per = Request {
        target: path.display().to_string(),
        name: None,
        objective: None,
        ..request.clone()
    };
    match adopt(store, &per, progress) {
        Ok(created) => AdoptedRow::adopted(path, created.unit.slug.to_string()),
        Err(Error::AdoptAlreadyAUnit { .. }) => AdoptedRow::skipped(path, "already a unit"),
        Err(Error::AdoptProjectRoot { .. }) => AdoptedRow::skipped(path, "the main checkout"),
        Err(error) => AdoptedRow::failed(path, error.to_string()),
    }
}

/// Whether two checkouts are the same directory on this machine.
fn same_tree(left: &Path, right: &Path) -> bool {
    guard::resolve(left) == guard::resolve(right)
}

/// Run `post_new` in the home, for the form of adoption that made one.
///
/// The two forms did different things, so they run different hooks. Adoption in place
/// created nothing: the directory was the person's before the command and is byte for
/// byte the same after it, and running a project's own commands inside a live checkout
/// on the strength of registering it is not something registering it asked for. The
/// materialised form made a home from a base, which is what `nodal new` does, and a
/// recipe that declares `post_new` declares it about exactly that home; so it runs
/// there, in the same phase and with the same context a create gives it.
///
/// Neither form runs `pre_new`. That hook is about the moment before a home exists, and
/// by the time either form of adoption can be refused or not, the thing it would run
/// ahead of has either already been there for weeks or is a clone with no decision left
/// in it.
fn post_new(params: &Params, enabled: bool) -> Result<()> {
    if params.source == Source::InPlace {
        return Ok(());
    }
    let runner = Runner {
        project: params.project.root.clone(),
        hooks: params.recipe.hooks.clone(),
        approvals: Approvals::open(hooks::path_in(&params.state_dir))?,
        enabled,
    };
    runner.run(Phase::PostNew, &params.environment.home, &context_of(params))?;
    Ok(())
}

/// The `NODAL_*` context the hook is given, as a create builds it.
///
/// `NODAL_PARENT_ID` names the base the home was cloned from, so it is empty for a
/// checkout adopted in place — which never reaches here, and would say so if it did.
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

/// Work out what the operation will do, and refuse everything that cannot be done.
///
/// Every refusal is made before the base is asked for, for the reason a create gives:
/// asking for a base may build one, and a build is minutes.
fn prepare(store: &mut Store, request: &Request, progress: &Arc<dyn Reporter>) -> Result<Params> {
    let (target, root) = resolve(request)?;
    let effective = crate::recipe::load(&root)?;
    let project = new::ensure_project(store, &root, &effective.recipe)?;
    if let Target::Standing { checkout, .. } = &target {
        // Before the branch rule, because "this directory is already a unit" is the
        // more useful of the two answers when both are true.
        guard::adoption(store.conn(), checkout, &project.root)?;
    }

    let branch = target.branch().clone();
    new::refuse_held_branch(store.conn(), project.id, &branch)?;
    let objective = objective_of(request, &target);
    let name = new::choose_name(
        store.conn(),
        project.id,
        request.name.as_ref(),
        handle_source(&branch, objective.as_ref().map(|(text, _)| text)).as_ref(),
    )?;

    let state_dir = home::directory()?;
    let block = ports::ensure_block(store, project.id)?;
    let unit = adopted_unit(project.id, &name.slug, &branch, objective.as_ref());
    let mut environment = match &target {
        Target::Standing { checkout, .. } => {
            standing_environment(unit.id, checkout, unit.created_at)
        }
        Target::Branch { .. } => {
            let fresh = new::new_environment(unit.id, &project.name, &state_dir, unit.created_at);
            guard::placement(store.conn(), &fresh.home, &root)?;
            fresh
        }
    };

    let source = match &target {
        Target::Standing { .. } => Source::InPlace,
        Target::Branch { .. } => {
            let wanted = substrate::Request {
                project: project.clone(),
                source: root.clone(),
                recipe: effective.recipe.clone(),
                state_dir: state_dir.clone(),
                warm: WARM_BUILD,
            };
            let base = substrate::ensure(store, &wanted, progress)?;
            environment.base_id = Some(base.base.id);
            environment.ws_fp_materialized = Some(base.fingerprint);
            Source::Materialized { base_path: base.base.path, from: root }
        }
    };

    Ok(Params {
        ports: new::port_names(&effective.recipe),
        recipe: effective.recipe,
        project,
        state_dir,
        unit,
        environment,
        block,
        source,
        recovered: objective.is_some_and(|(_, epistemic)| epistemic == Epistemic::Observed),
    })
}

/// The plan: what the home needs, and then one registry write.
///
/// # Errors
/// [`Error::Render`] when the parameters cannot be written to the journal.
pub fn plan(params: &Params) -> Result<Plan> {
    let home = params.environment.home.clone();
    let value = serde_json::to_value(params)
        .map_err(|source| Error::Render { kind: "operation parameters", source })?;
    let plan = Plan::new(KIND, params.unit.slug.to_string(), value, commit_of(params));
    let plan = match &params.source {
        Source::InPlace => plan,
        Source::Materialized { base_path, from } => plan
            .then(new::Materialize {
                base: base_path.clone(),
                home: home.clone(),
                excludes: Excludes::with_recipe(&params.recipe.base.exclude),
                backend: select_backend(&Sharing::ensure(&params.state_dir)),
            })
            .then(new::Relocate {
                home: home.clone(),
                base: base_path.clone(),
                relocator: InvalidateCache::with_recipe(&params.recipe.base.invalidate),
            })
            .then(new::Scrub { home: home.clone() })
            .then(FetchBranch {
                home: home.clone(),
                from: from.clone(),
                branch: params.unit.branch.clone(),
            })
            .then(new::TakeBranch {
                home: home.clone(),
                branch: params.unit.branch.clone(),
                start: None,
            }),
    };
    Ok(plan
        .then(files::Hide { home: home.clone() })
        .then(marker::WriteMarker { home, unit: params.unit.id })
        .then(new::Activate {
            project: params.project.clone(),
            unit: params.unit.clone(),
            environment: params.environment.clone(),
            recipe: params.recipe.clone(),
            block: params.block,
            state_dir: params.state_dir.clone(),
        }))
}

/// The registry write that finishes an adoption.
///
/// It writes the same rows a create writes, and one more thing: the log lines that say
/// where this unit came from. A unit a person adopted three weeks after the fact is
/// exactly the unit whose history nobody remembers, so the operation puts what it knows
/// into the record rather than only into the report it prints once.
fn commit_of(params: &Params) -> Commit {
    let (unit, environment) = (params.unit.clone(), params.environment.clone());
    let (block, names) = (params.block, params.ports.clone());
    let (source, recovered) = (params.source.clone(), params.recovered);
    let idle_hours = params.recipe.lock_idle_hours();
    Box::new(move |tx: &Transaction<'_>, outputs: &Outputs| -> Result<Output> {
        units::insert(tx, &unit)?;
        environments::insert(tx, &environment)?;
        let granted = ports::allocate(tx, block, environment.id, &names)?;
        environments::set_ports(tx, environment.id, &granted)?;
        note(tx, (&unit, environment.id), &origin_of(&source, &environment))?;
        if recovered {
            note(tx, (&unit, environment.id), RECOVERED)?;
        }
        let relocation: Option<relocate::Report> = outputs.read(new::RELOCATE)?;
        new::record_relocation(tx, unit.id, environment.id, relocation.as_ref())?;
        // Whoever adopted the checkout holds the write on it, the same as whoever made
        // a unit from a base. An adopted directory is a person's own working copy, so
        // the one thing that must not happen is a second actor writing in it unheard.
        crate::runtime::lock::open(tx, unit.id, idle_hours, Timestamp::now())?;
        Ok(nothing())
    })
}

/// What the log says about a recovered objective.
const RECOVERED: &str = "objective recovered from the opening prompt of the session that ran in this \
     directory; it was read, not stated";

/// What the log says about where the home came from.
fn origin_of(source: &Source, environment: &Environment) -> String {
    match source {
        Source::InPlace => format!(
            "adopted the checkout at {home} where it stands; nothing was copied, and a \
             reclaim unregisters it rather than moving it",
            home = environment.home.display()
        ),
        Source::Materialized { base_path, from } => format!(
            "materialised a home at {home} from the base at {base}, and fetched the \
             branch from {from}",
            home = environment.home.display(),
            base = base_path.display(),
            from = from.display()
        ),
    }
}

/// One line of the unit's log, as Nodal saw it rather than as anybody said it.
fn note(tx: &Transaction<'_>, subject: (&Unit, EnvId), body: &str) -> Result<()> {
    let (unit, environment) = subject;
    events::note(
        tx,
        (unit.id, Some(environment)),
        EventKind::Note,
        body.to_owned(),
        &[("operation", String::from(KIND))],
    )
}

/// Finding an interrupted adoption again, from what the journal kept.
pub struct Adopt;

impl Rebuild for Adopt {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn rebuild(&self, record: &Operation) -> Result<Plan> {
        let params: Params = serde_json::from_value(record.params.clone()).map_err(|_| {
            Error::InvalidValue { kind: "adoption parameters", value: record.id.to_string() }
        })?;
        // A resumed run finishes the home and writes no report, so the notes it would
        // have carried have nowhere to go.
        plan(&params)
    }
}

// ---------------------------------------------------------------------------
// The one step a create does not have.
// ---------------------------------------------------------------------------

/// Bring the branch's commits into the home from the project's own checkout.
///
/// A base is a clone of the project as it was when the base was built, so a branch made
/// after that — or one that never left this machine — is not in it. The objects come
/// from the checkout by path, which is where a local branch is, and they land on the
/// branch's own name so that the step after this one has something to switch to.
struct FetchBranch {
    /// The home the objects go into.
    home: PathBuf,
    /// The checkout they come from.
    from: PathBuf,
    /// The branch being adopted.
    branch: BranchName,
}

impl Step for FetchBranch {
    fn key(&self) -> String {
        String::from("git.fetch-branch")
    }

    /// Repeatable: fetching a ref that is already at that commit moves nothing.
    fn apply(&self) -> Result<Output> {
        let name = self.branch.as_str();
        Git::open(&self.home)?.fetch_branch(&self.from, name, &format!("refs/heads/{name}"))?;
        Ok(nothing())
    }

    /// Nothing. What this wrote is inside a directory the first step's undo removes.
    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// What the plan is built from.
// ---------------------------------------------------------------------------

/// What the target names.
enum Target {
    /// A checkout that is already on disk, with the branch it has checked out.
    Standing {
        /// Where it is.
        checkout: PathBuf,
        /// What it has checked out.
        branch: BranchName,
    },
    /// A branch nothing on this machine has checked out.
    Branch {
        /// The branch.
        branch: BranchName,
    },
}

impl Target {
    /// The branch the unit will own either way.
    const fn branch(&self) -> &BranchName {
        match self {
            Self::Standing { branch, .. } | Self::Branch { branch } => branch,
        }
    }

    /// The checkout, for a target that is one. A directory Nodal has yet to make has
    /// nothing to ask.
    fn checkout(&self) -> &Path {
        match self {
            Self::Standing { checkout, .. } => checkout,
            Self::Branch { .. } => Path::new("."),
        }
    }
}

/// What a target names, and the project's own checkout either way.
///
/// Three questions, in this order. A target that is a directory is a checkout, and a
/// checkout can be adopted only where it stands. A target that names a branch some
/// worktree of the project holds is that worktree. Anything else is a branch of the
/// project, which must exist.
///
/// Which project the unit belongs to is decided by [`project_of`].
fn resolve(request: &Request) -> Result<(Target, PathBuf)> {
    let path = Path::new(&request.target);
    if path.is_dir() {
        if !request.in_place {
            return Err(Error::AdoptNeedsInPlace { checkout: path.to_path_buf() });
        }
        let target = standing(path)?;
        let git = Git::open(target.checkout())?;
        git.ensure_no_operation_in_progress()?;
        return Ok((target, project_of(&git, &request.cwd)?));
    }
    let git = Git::open(&request.cwd)?;
    git.ensure_no_operation_in_progress()?;
    let root = main_checkout(&git)?;
    let branch = BranchName::parse(&request.target)?;
    if let Some(held) = holder_of(&git, &branch)? {
        if !request.in_place {
            return Err(Error::AdoptBranchCheckedOut { branch, checkout: held });
        }
        return Ok((standing(&held)?, root));
    }
    if !Git::open(&root)?.branch_exists(branch.as_str())? {
        return Err(Error::GitUnknownBranch { repo: root, branch: branch.to_string() });
    }
    Ok((Target::Branch { branch }, root))
}

/// One checkout, and the branch it has out.
fn standing(checkout: &Path) -> Result<Target> {
    let git = Git::open(checkout)?;
    let checkout = git.top_level()?;
    let Some(branch) = git.current_branch()? else {
        return Err(Error::AdoptDetached { checkout });
    };
    Ok(Target::Standing { checkout, branch: BranchName::parse(branch)? })
}

/// The worktree of the project holding `branch`, when one does.
///
/// Every worktree of a repository is in its own record, the main checkout included, and
/// a branch can be checked out in only one of them. So this answers with the directory
/// whose uncommitted work a materialised home would leave behind.
fn holder_of(git: &Git, branch: &BranchName) -> Result<Option<PathBuf>> {
    Ok(git
        .worktrees()?
        .into_iter()
        .find(|registered| registered.branch.as_deref() == Some(branch.as_str()))
        .map(|registered| registered.path))
}

/// The project a checkout named by path is adopted into.
///
/// A **linked worktree** belongs to the repository it was made from, and that
/// repository's main worktree is the project, whichever directory the command was run
/// in. That is the case adoption exists for: the worktrees another tool left under
/// `.claude/` are all of one repository.
///
/// Any other checkout is a repository in its own right, and its own main worktree is
/// itself, so it cannot say which project it is a unit of. The working directory says,
/// as it does for every other command: a second clone of a project, adopted from inside
/// that project, is a unit of it. A person standing outside any repository is told that
/// the checkout is a project's own, which is the true answer to what they asked.
fn project_of(git: &Git, cwd: &Path) -> Result<PathBuf> {
    if git.layout()?.is_linked() {
        return main_checkout(git);
    }
    match Git::open(cwd) {
        Ok(here) => main_checkout(&here),
        Err(_) => git.top_level(),
    }
}

/// The project's own checkout: the main worktree of the repository the command was run
/// in.
///
/// A linked worktree shares a repository with the checkout it was made from, and the
/// record of a repository's worktrees names the main one first. That is the project,
/// whichever of its worktrees a person happened to type the command in.
fn main_checkout(git: &Git) -> Result<PathBuf> {
    if !git.layout()?.is_linked() {
        return git.top_level();
    }
    match git.worktrees()?.first() {
        Some(main) => Ok(main.path.clone()),
        None => git.top_level(),
    }
}

/// What the unit is for: what a person stated, or what the records of the session that
/// ran in the checkout say, or nothing.
///
/// A stated objective wins and is never replaced. Recovery is tried only for a checkout
/// that is already on disk, because it is a reading of the sessions that ran *there*.
fn objective_of(request: &Request, target: &Target) -> Option<(Objective, Epistemic)> {
    if let Some(stated) = &request.objective {
        return Some((stated.clone(), Epistemic::Stated));
    }
    let (Target::Standing { checkout, .. }, Some(sessions)) = (target, &request.sessions) else {
        return None;
    };
    let prompt = crate::doctor::intent::recover(sessions, checkout)?;
    Some((one_line(&prompt).ok()?, Epistemic::Observed))
}

/// A prompt as an objective: its first line, cut to the width of one.
///
/// A prompt is a paragraph and an objective is a line, so this takes the first line
/// that has anything in it and marks the cut when there was more.
fn one_line(prompt: &str) -> Result<Objective> {
    let first = prompt.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let mut text: String = first.chars().take(OBJECTIVE_WIDTH).collect();
    if first.chars().count() > OBJECTIVE_WIDTH {
        text.push(ELLIPSIS);
    }
    Objective::parse(text)
}

/// What the handle is derived from when a person did not choose one.
///
/// The branch, because a branch is what every other tool already shows for this work
/// and a person looking for the unit will type the name they see in `git branch`. The
/// objective is what a create derives from, and it stands in only for a branch whose
/// last segment holds nothing a handle can be made of.
fn handle_source(branch: &BranchName, objective: Option<&Objective>) -> Option<Objective> {
    let last = branch.as_str().rsplit('/').next().unwrap_or_default();
    if last.chars().any(|character| character.is_ascii_alphanumeric())
        && let Ok(from_branch) = Objective::parse(last.replace(['_', '.'], " "))
    {
        return Some(from_branch);
    }
    objective.cloned()
}

/// The unit row an adoption will write.
fn adopted_unit(
    project: crate::model::ProjectId,
    slug: &Slug,
    branch: &BranchName,
    objective: Option<&(Objective, Epistemic)>,
) -> Unit {
    let now = Timestamp::now();
    Unit {
        id: UnitId::from_ulid(ulid::Ulid::new()),
        project_id: project,
        slug: slug.clone(),
        objective: objective.map(|(text, _)| text.clone()),
        objective_epistemic: objective.map(|(_, epistemic)| *epistemic),
        branch: branch.clone(),
        parent_branch: None,
        // Not recorded, and not guessable. Nodal did not make this worktree and was not
        // there when its branch left the base; the merge base it has now is where the
        // branch stands, which is a different fact and would be a wrong answer to the
        // question this column asks ([`crate::model::Unit::base_commit`]).
        base_commit: None,
        status: UnitStatus::Open,
        created_at: now,
        updated_at: now,
    }
}

/// The environment row for a checkout adopted where it stands.
///
/// `managed` is `false`, and every other difference from a created unit's row follows
/// from it: there is no base, because nothing was cloned, and no workspace fingerprint
/// was materialised, because nothing was materialised.
fn standing_environment(unit: UnitId, checkout: &Path, at: Timestamp) -> Environment {
    Environment {
        id: EnvId::from_ulid(ulid::Ulid::new()),
        unit_id: unit,
        attempt: 1,
        home: guard::resolve(checkout),
        managed: false,
        base_id: None,
        ws_fp_materialized: None,
        schema_fp_materialized: None,
        host: HostName::current(),
        db_name: None,
        ports: crate::model::Ports::default(),
        fixed_port: None,
        state: crate::model::EnvState::Stopped,
        created_at: at,
        last_active: at,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{OBJECTIVE_WIDTH, handle_source, one_line};
    use crate::model::{BranchName, Objective};

    fn branch(name: &str) -> BranchName {
        BranchName::parse(name).unwrap()
    }

    #[test]
    fn a_prompt_becomes_the_first_line_of_it() {
        let prompt = "\n  Fix the token refresh  \n\nand then look at the session store\n";
        assert_eq!(one_line(prompt).unwrap().as_str(), "Fix the token refresh");
    }

    #[test]
    fn a_prompt_longer_than_a_line_is_cut_and_says_so() {
        let prompt = "a".repeat(OBJECTIVE_WIDTH + 20);
        let objective = one_line(&prompt).unwrap();
        assert_eq!(objective.as_str().chars().count(), OBJECTIVE_WIDTH + 1);
        assert!(objective.as_str().ends_with('…'), "{objective}");
    }

    #[test]
    fn a_prompt_with_nothing_in_it_is_not_an_objective() {
        assert!(one_line("   \n\n").is_err());
    }

    #[test]
    fn a_handle_comes_from_the_last_segment_of_the_branch() {
        let from = |name: &str| handle_source(&branch(name), None).unwrap().to_string();
        assert_eq!(from("feature/token-refresh"), "token-refresh");
        assert_eq!(from("audit_F12"), "audit F12");
        assert_eq!(from("nodal/worker-import"), "worker-import");
    }

    /// A branch whose last segment holds nothing a name can be made of falls back to
    /// what the unit is for, which is what a create derives from.
    #[test]
    fn a_branch_with_no_name_in_it_falls_back_to_the_objective() {
        let objective = Objective::parse("worker import").unwrap();
        let source = handle_source(&branch("wip/-"), Some(&objective)).unwrap();
        assert_eq!(source.as_str(), "worker import");
    }
}
