//! `nodal merge`: one command from a dirty unit to a merged target and no unit left.
//!
//! Five stages, in this order, each of which a `--no-` flag turns off:
//!
//! | stage    | what it does                                                       |
//! |----------|--------------------------------------------------------------------|
//! | commit   | commits everything the home holds, on the unit's branch            |
//! | squash   | folds the branch into one commit on its merge base with the target |
//! | rebase   | rebases that commit onto the target as the target is now           |
//! | forward  | moves the target in the project's own checkout, by fast-forward    |
//! | remove   | reclaims the unit through the ordinary reclaim, so the home is in the trash |
//!
//! The first four are steps of one [`Plan`] and are journalled, undone in reverse, and
//! rebuilt after a kill like any other operation. The fifth is not: it is
//! [`super::reclaim`], called as a person would call it, so that a merged unit is
//! removed by the one code path that knows how to check a home before it moves it.
//!
//! # Three refusals, and none of them is a force
//!
//! **The target is moved by fast-forward or not at all.** The commit the target branch
//! points at must be an ancestor of the commit it is moving to. A target somebody else
//! has moved is refused and the person runs the merge again, which rebases onto where
//! it is now. There is no flag that overrides this, and nothing here pushes: the
//! fast-forward moves a local branch in the project's own checkout, and sending it
//! anywhere stays the person's own command (`decisions/DL-034`).
//!
//! **Nothing is squashed before it is recorded.** The branch tip is written to
//! `refs/nodal/<id>/premerge` before the squash rewrites anything, and that ref is never
//! overwritten. Every commit the squash folded is reachable from it until `nodal gc`
//! removes the trashed home it lives in.
//!
//! **A unit is removed by the reclaim's rules.** The reclaim's uniqueness check runs
//! exactly as it does when a person types `nodal reclaim`, so anything the merge did not
//! integrate — an untracked file, work on another branch — refuses the removal and is
//! reported. The merge is not undone by that refusal: the target is already forwarded,
//! and the unit is simply still there.
//!
//! # A conflict is an answer
//!
//! A rebase that stops for a conflict leaves the home in the middle of a rebase, which
//! is a state Git and a person both know what to do with. So it is not a failure: the
//! operation finishes, the report says which paths conflict and what the two ways out
//! are, and the unit's status does not move. A second `nodal merge` continues the rebase
//! and carries on through the remaining stages; `nodal merge --abort` stops it and puts
//! the branch back at `premerge`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::git::{Git, Oid, merge as plumbing, refs};
use crate::lifecycle::hooks::{self, Approvals, Context, Phase, Ran, Runner};
use crate::lifecycle::journal::Operation;
use crate::lifecycle::step::{Commit, Output, Outputs, Plan, Step, nothing};
use crate::lifecycle::{Rebuild, marker, run};
use crate::model::{
    BranchName, EnvState, Environment, EventKind, Project, Timestamp, Unit, UnitStatus,
};
use crate::output::view::{Merged, StageLine};
use crate::store::{Store, environments, events, units};
use crate::workspace::home;
use crate::{Error, Result};

/// What this operation is called in the journal.
pub const KIND: &str = "merge";

/// The key of the last step, whose answer both the registry write and the report read.
const FORWARD: &str = "target.forward";

/// The branch names a target is looked for under when the unit does not name one, in
/// order. `origin/HEAD` is what a clone recorded as the project's own default branch.
const FALLBACK_BRANCHES: [&str; 2] = ["main", "master"];

/// Where `refs/remotes/origin/HEAD` is read from.
const ORIGIN_HEAD: &str = "refs/remotes/origin/HEAD";

/// One stage of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Commit what the home holds, before anything else reads the branch.
    Commit,
    /// Fold the branch into one commit.
    Squash,
    /// Rebase the branch onto the target.
    Rebase,
    /// Reclaim the unit once the target carries its work.
    Remove,
}

/// Every stage, in the order they run. The `--no-` flags are a table over this
/// (`docs/code-structure.md`, data over code), so a stage is added by adding a line.
pub const STAGES: &[Stage] = &[Stage::Commit, Stage::Squash, Stage::Rebase, Stage::Remove];

impl Stage {
    /// The word this stage is called by, in the flag and in the report.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
            Self::Remove => "remove",
        }
    }
}

/// Which stages of the pipeline run. Each `--no-` flag drops exactly one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Stages(BTreeSet<Stage>);

impl Stages {
    /// Every stage: what a merge with no flags does.
    #[must_use]
    pub fn all() -> Self {
        Self(STAGES.iter().copied().collect())
    }

    /// The same, without one stage.
    #[must_use]
    pub fn without(mut self, stage: Stage) -> Self {
        self.0.remove(&stage);
        self
    }

    /// Whether this stage runs.
    #[must_use]
    pub fn runs(&self, stage: Stage) -> bool {
        self.0.contains(&stage)
    }
}

/// What a person asked `nodal merge` for.
#[derive(Debug, Clone)]
pub struct Request {
    /// The unit's handle, or nothing to mean the unit the working directory is in.
    pub target: Option<String>,
    /// The commit message. `None` takes the unit's objective, and then its handle.
    pub message: Option<String>,
    /// Which stages run.
    pub stages: Stages,
    /// Whether the project's own hooks run. `false` is `--no-hooks`.
    pub hooks: bool,
    /// Where the command was run, which decides the unit when no target was given.
    pub cwd: PathBuf,
}

/// The branch the work is merged into, and where it stood when the plan was made.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    /// Its name in the project's own checkout.
    pub branch: String,
    /// The commit it pointed at when the plan was made.
    pub oid: Oid,
}

/// Everything the plan is built from, and the whole of what the journal keeps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Params {
    /// The project the unit belongs to.
    pub project: Project,
    /// The unit being merged.
    pub unit: Unit,
    /// Its materialisation, which is the home the branch lives in.
    pub environment: Environment,
    /// The branch the work is merged into.
    pub target: Target,
    /// The commit the unit's branch pointed at before the merge touched it.
    pub head: Oid,
    /// The message the commit and the squash carry.
    pub message: String,
    /// Which stages this run does.
    pub stages: Stages,
    /// Whether this run is continuing a rebase an earlier one stopped in.
    pub resuming: bool,
}

impl Params {
    /// The unit's home.
    fn home(&self) -> &Path {
        &self.environment.home
    }

    /// The ref the branch tip is recorded on before the squash rewrites it.
    fn premerge(&self) -> String {
        refs::premerge(&self.unit.id.to_string())
    }

    /// The ref the target is fetched onto inside the home.
    fn fetched(&self) -> String {
        refs::target(&self.unit.id.to_string())
    }
}

/// The plan, as a person reads it before they agree to it.
///
/// # Errors
/// Whatever [`prepare`] reported.
pub fn preview(store: &mut Store, request: &Request) -> Result<Merged> {
    let prepared = prepare(store, request)?;
    let params = &prepared.params;
    let kept = Git::at(params.home()).read_ref(&params.premerge())?.map(|_| params.premerge());
    Ok(Merged::planned((&params.unit, &params.target.branch), intent(params)?, kept))
}

/// Run the whole pipeline: commit, squash, rebase, fast-forward, reclaim.
///
/// # Errors
/// [`Error::MergeNoTarget`] when no branch of the project answers as the one the unit
/// merges into, [`Error::MergeTargetMoved`] when the target has moved under the rebase,
/// [`Error::MergeNotOnBranch`] when the home is not on the unit's branch,
/// [`Error::HookNotApproved`] when a declared hook is not the approved one, and whatever
/// Git, the filesystem or the registry reported.
pub fn merge(store: &mut Store, request: &Request) -> Result<Merged> {
    let prepared = prepare(store, request)?;
    let params = &prepared.params;
    let mut ran = Vec::new();
    ran.extend(prepared.hook(Phase::PreMerge, params.home().to_path_buf())?);
    let finished = run(store, &plan(params)?)?;
    let done = report(params, ran, Sent::of(&finished.outputs)?.forwarded)?;
    if done.conflict.is_some() {
        return Ok(done);
    }
    let mut done = done;
    done.hooks.extend(prepared.hook(Phase::PostMerge, params.home().to_path_buf())?);
    remove(store, request, params, &mut done);
    Ok(done)
}

/// Stop a rebase a merge left, and put the branch back where the merge found it.
///
/// # Errors
/// [`Error::MergeNotStopped`] when the unit is not in the middle of a merge, and
/// whatever Git or the registry reported.
pub fn abort(store: &mut Store, request: &Request) -> Result<Merged> {
    let unit =
        crate::runtime::entry::unit_named(store.conn(), request.target.as_deref(), &request.cwd)?;
    let environment = live(store.conn(), &unit)?;
    let git = Git::open(&environment.home)?;
    if !git.is_rebasing()? {
        return Err(Error::MergeNotStopped { slug: unit.slug.clone() });
    }
    git.abort_rebase()?;
    let premerge = refs::premerge(&unit.id.to_string());
    let mut stages: Vec<StageLine> =
        vec![StageLine::new("rebase", "stopped, and the branch is off it")];
    if let Some(oid) = git.read_ref(&premerge)? {
        plumbing::restore(&environment.home, unit.branch.as_str(), &oid)?;
        stages
            .push(StageLine::new("restore", format!("{} is back at {}", unit.branch, short(&oid))));
        git.delete_ref(&premerge)?;
    }
    git.delete_ref(&refs::target(&unit.id.to_string()))?;
    Ok(Merged::aborted(&unit, stages))
}

/// The plan: fetch the target, commit, record, squash, rebase, fast-forward.
///
/// The order is the order the stages are named in, with one addition at the front: the
/// target is fetched first, because the squash needs the merge base with it and the
/// rebase needs the commit itself.
///
/// # Errors
/// [`Error::Render`] when the parameters cannot be written to the journal.
pub fn plan(params: &Params) -> Result<Plan> {
    let value = serde_json::to_value(params)
        .map_err(|source| Error::Render { kind: "operation parameters", source })?;
    let mut plan = Plan::new(KIND, params.unit.slug.to_string(), value, commit_of(params));
    plan = plan.then(FetchTarget {
        home: params.home().to_path_buf(),
        source: params.project.root.clone(),
        target: params.target.clone(),
        into: params.fetched(),
    });
    if !params.resuming {
        plan = rewriting(plan, params);
    }
    if params.stages.runs(Stage::Rebase) {
        plan = plan
            .then(Rebase { home: params.home().to_path_buf(), onto: params.target.oid.clone() });
    }
    Ok(plan.then(Forward {
        home: params.home().to_path_buf(),
        source: params.project.root.clone(),
        branch: params.unit.branch.clone(),
        target: params.target.clone(),
    }))
}

/// The steps that rewrite the unit's branch, which a resumed merge has behind it.
fn rewriting(plan: Plan, params: &Params) -> Plan {
    let mut plan = plan;
    if params.stages.runs(Stage::Commit) {
        plan = plan.then(CommitWork {
            home: params.home().to_path_buf(),
            message: params.message.clone(),
            head: params.head.clone(),
        });
    }
    if params.stages.runs(Stage::Squash) {
        plan = plan
            .then(RecordPremerge {
                home: params.home().to_path_buf(),
                reference: params.premerge(),
            })
            .then(Squash {
                home: params.home().to_path_buf(),
                onto: params.target.oid.clone(),
                message: params.message.clone(),
                premerge: params.premerge(),
            });
    }
    plan
}

/// The registry write that finishes a merge.
///
/// A merge whose rebase stopped for a conflict has not merged anything and must not
/// record that it did. The step that moves the target is the one that reads the home
/// and finds out ([`Sent`]), and it hands the answer here through the journal, so a
/// rebuilt plan and a first run agree without either of them asking Git twice.
///
/// Reading it here instead would be worse than duplicated work. This closure runs
/// inside the registry's one `IMMEDIATE` transaction, which every `nodal` on the
/// machine queues behind, and a `git rev-parse` inside it holds that lock across a
/// process spawn.
///
/// A journal with no answer in it is treated as "stopped". That is the safe half: a
/// unit wrongly recorded merged starts a collection clock over work that was never
/// merged, and a unit wrongly recorded stopped is merged again by a person who looks.
fn commit_of(params: &Params) -> Commit {
    let (unit, target) = (params.unit.clone(), params.target.branch.clone());
    Box::new(move |tx: &Transaction<'_>, outputs: &Outputs| -> Result<Output> {
        let body = if Sent::of(outputs)?.stopped {
            format!("merge into {target} stopped for a conflict")
        } else {
            units::update_status(tx, unit.id, UnitStatus::Merged, Timestamp::now())?;
            format!("merged into {target}")
        };
        record(tx, &unit, &target, body)?;
        Ok(nothing())
    })
}

/// Write the line a later `nodal explain` reads: what was merged, and where.
fn record(tx: &Transaction<'_>, unit: &Unit, target: &str, body: String) -> Result<()> {
    events::note(
        tx,
        (unit.id, None),
        EventKind::Sync,
        body,
        &[("target", target.to_owned()), ("branch", unit.branch.to_string())],
    )
}

/// Finding an interrupted merge again, from what the journal kept.
pub struct Merge;

impl Rebuild for Merge {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn rebuild(&self, record: &Operation) -> Result<Plan> {
        let params: Params = serde_json::from_value(record.params.clone()).map_err(|_| {
            Error::InvalidValue { kind: "merge parameters", value: record.id.to_string() }
        })?;
        plan(&params)
    }
}

// ---------------------------------------------------------------------------
// The steps.
// ---------------------------------------------------------------------------

/// Fetch the branch the work merges into, from the project's own checkout.
struct FetchTarget {
    /// The home the objects go into.
    home: PathBuf,
    /// The checkout they come from.
    source: PathBuf,
    /// The branch, and where it stood when the plan was made.
    target: Target,
    /// The ref inside the home the fetch lands on.
    into: String,
}

impl Step for FetchTarget {
    fn key(&self) -> String {
        String::from("target.fetch")
    }

    /// Repeatable: a second fetch of the same branch writes the same ref.
    ///
    /// It also refuses a target that has moved since the plan was made. The plan holds
    /// the commit the fast-forward will be measured against, so a target that moved
    /// between the plan and this step would be rebased onto one commit and checked
    /// against another.
    fn apply(&self) -> Result<Output> {
        let found =
            plumbing::fetch_branch(&self.home, &self.source, &self.target.branch, &self.into)?;
        if found == self.target.oid {
            return Ok(nothing());
        }
        Err(Error::MergeTargetMoved {
            branch: self.target.branch.clone(),
            found: found.as_str().to_owned(),
        })
    }

    /// Drop the ref. The objects stay until the home's own `git gc` collects them,
    /// which is what an undo of a fetch can honestly promise.
    fn undo(&self) -> Result<()> {
        Git::at(&self.home).delete_ref(&self.into)
    }
}

/// Commit everything the home holds onto the unit's branch.
struct CommitWork {
    /// The home.
    home: PathBuf,
    /// The message the commit carries.
    message: String,
    /// The commit the branch was at before, which is what the undo goes back to.
    head: Oid,
}

impl Step for CommitWork {
    fn key(&self) -> String {
        String::from("work.commit")
    }

    /// Repeatable: a home with nothing left to commit is left alone.
    fn apply(&self) -> Result<Output> {
        plumbing::commit_all(&self.home, &self.message)?;
        Ok(nothing())
    }

    /// Take the commit back and leave its content in the working tree, which is the
    /// state the merge found: changed files, nothing committed.
    fn undo(&self) -> Result<()> {
        plumbing::uncommit(&self.home, &self.head)
    }
}

/// Write the branch tip to the ref the squash may be undone from.
struct RecordPremerge {
    /// The home.
    home: PathBuf,
    /// The ref.
    reference: String,
}

impl Step for RecordPremerge {
    fn key(&self) -> String {
        String::from("premerge.record")
    }

    /// Repeatable, and deliberately not idempotent in the other direction: a ref that
    /// is already there is left as it is. A second run of a killed merge would
    /// otherwise record the squashed tip and lose the commits the ref exists to keep.
    fn apply(&self) -> Result<Output> {
        let git = Git::at(&self.home);
        if git.read_ref(&self.reference)?.is_some() {
            return Ok(nothing());
        }
        let head = git.rev_parse("HEAD")?;
        git.write_ref(&self.reference, &head, "nodal: before the merge squashed it")?;
        Ok(nothing())
    }

    /// Drop the ref, which is safe only in the order the runner undoes steps in: the
    /// squash is undone first, and it is undone from this ref.
    fn undo(&self) -> Result<()> {
        Git::at(&self.home).delete_ref(&self.reference)
    }
}

/// Fold the branch into one commit on its merge base with the target.
struct Squash {
    /// The home.
    home: PathBuf,
    /// The target, whose merge base with the branch the commit is parented on.
    onto: Oid,
    /// The message the commit carries.
    message: String,
    /// The ref the branch tip was recorded on, which the undo reads.
    premerge: String,
}

impl Step for Squash {
    fn key(&self) -> String {
        String::from("branch.squash")
    }

    /// Repeatable: a branch that is already one commit is left alone.
    fn apply(&self) -> Result<Output> {
        plumbing::squash(&self.home, &self.onto, &self.message)?;
        Ok(nothing())
    }

    /// Put the branch back at the commit the record kept.
    fn undo(&self) -> Result<()> {
        restore_head(&self.home, &self.premerge)
    }
}

/// Rebase the branch onto the target.
struct Rebase {
    /// The home.
    home: PathBuf,
    /// The commit to rebase onto.
    onto: Oid,
}

impl Step for Rebase {
    fn key(&self) -> String {
        String::from("branch.rebase")
    }

    /// Repeatable in each of the three states a killed run can leave: the branch not
    /// rebased, the branch rebased, and a rebase stopped part-way, which is continued.
    ///
    /// A conflict is not a failure here. The step leaves the home in the middle of a
    /// rebase and answers `Ok`, and the steps after it read that state and do nothing.
    fn apply(&self) -> Result<Output> {
        plumbing::rebase(&self.home, &Git::at(&self.home).git_dir()?, &self.onto)?;
        Ok(nothing())
    }

    /// Stop the rebase, then put the branch back where the record says it was.
    fn undo(&self) -> Result<()> {
        let git = Git::at(&self.home);
        git.abort_rebase()?;
        restore_head(&self.home, &refs::premerge(&marker_of(&self.home)?))
    }
}

/// What the last step of a merge found and did: the two facts the registry write and
/// the report are built from.
///
/// One value rather than two outputs, because they are read together and are answers
/// to the same question — whether this merge finished.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Sent {
    /// Whether the home is in the middle of a rebase, which is what says the merge
    /// stopped for a conflict rather than finishing.
    stopped: bool,
    /// Whether the target branch moved.
    forwarded: bool,
}

impl Sent {
    /// This, as the step returns it.
    fn value(self) -> Result<Output> {
        serde_json::to_value(self).map_err(|source| Error::Render { kind: "merge result", source })
    }

    /// What the merge's last step recorded, or the safe answer when it recorded nothing.
    ///
    /// Nothing is what a run whose last step never applied leaves, and the safe reading
    /// of that is a merge that stopped: see [`commit_of`].
    fn of(outputs: &Outputs) -> Result<Self> {
        Ok(outputs.read(FORWARD)?.unwrap_or(Self { stopped: true, forwarded: false }))
    }
}

/// Move the target branch in the project's own checkout, by fast-forward.
struct Forward {
    /// The home the tip is read from.
    home: PathBuf,
    /// The checkout whose branch moves.
    source: PathBuf,
    /// The unit's branch, which is what the target moves to.
    branch: BranchName,
    /// The target, and where it stood when the plan was made.
    target: Target,
}

impl Step for Forward {
    fn key(&self) -> String {
        String::from(FORWARD)
    }

    /// Repeatable: a target already at the tip is left alone. A home that is in the
    /// middle of a rebase has nothing finished to offer, so nothing moves.
    ///
    /// This is also the step that reads whether the rebase stopped, because it is the
    /// last one and it has to ask anyway. Both halves of the answer go to the commit
    /// and to the report as this step's output ([`Sent`]).
    fn apply(&self) -> Result<Output> {
        let git = Git::at(&self.home);
        if git.is_rebasing()? {
            return Sent { stopped: true, forwarded: false }.value();
        }
        let tip = git.rev_parse(self.branch.as_str())?;
        // The checkout does not have the unit's commits: they were made in the home,
        // and a merge sends nothing to a remote. The objects come across by path, and
        // the branch that moves next is what keeps them.
        plumbing::fetch_objects(&self.source, &self.home, self.branch.as_str())?;
        let moved = plumbing::fast_forward(&self.source, &self.target.branch, &tip)?;
        Sent { stopped: false, forwarded: moved }.value()
    }

    /// Put the target back at the commit the plan recorded. Refuses to lose a change
    /// somebody has in the checkout's working tree.
    fn undo(&self) -> Result<()> {
        plumbing::restore(&self.source, &self.target.branch, &self.target.oid)
    }
}

/// Put a branch back at the commit a ref of Nodal's own recorded.
fn restore_head(home: &Path, reference: &str) -> Result<()> {
    let git = Git::at(home);
    let Some(oid) = git.read_ref(reference)? else { return Ok(()) };
    plumbing::restore(home, &current_branch(&git)?, &oid)
}

/// The branch a home is on, which every step of a merge acts on.
fn current_branch(git: &Git) -> Result<String> {
    git.current_branch()?.ok_or_else(|| Error::GitUnknownBranch {
        repo: git.root().to_path_buf(),
        branch: String::from("HEAD"),
    })
}

/// The unit a home is marked for, which is how a rebuilt step names its own refs.
fn marker_of(home: &Path) -> Result<String> {
    Ok(marker::read(home)?
        .ok_or_else(|| Error::HomeUnmarked { home: home.to_path_buf() })?
        .to_string())
}

// ---------------------------------------------------------------------------
// What the plan is built from.
// ---------------------------------------------------------------------------

/// Everything worked out before the plan runs.
struct Prepared {
    /// The plan's input.
    params: Params,
    /// The project's hooks and the approvals for them.
    runner: Runner,
}

impl Prepared {
    /// Run one phase's hook, in a directory that exists.
    ///
    /// `pre_merge` runs in the home, because that is the tree it is about.
    /// `post_merge` runs in the project root, because that is the tree that moved.
    fn hook(&self, phase: Phase, root: PathBuf) -> Result<Option<Ran>> {
        let source = &self.params.project.root;
        let directory =
            if matches!(phase, Phase::PreMerge) { root.clone() } else { source.clone() };
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
}

/// Work out what the merge will do, and make every refusal before anything moves.
fn prepare(store: &mut Store, request: &Request) -> Result<Prepared> {
    let unit =
        crate::runtime::entry::unit_named(store.conn(), request.target.as_deref(), &request.cwd)?;
    let environment = live(store.conn(), &unit)?;
    let project = project_of(store.conn(), &unit)?;
    let git = home_of(&environment, &unit)?;
    let resuming = git.is_rebasing()?;
    let head = git.rev_parse(unit.branch.as_str())?;
    let target = resolve(&project, &unit)?;
    let recipe = crate::recipe::load(&project.root).map(|loaded| loaded.recipe).unwrap_or_default();
    let state_dir = home::directory()?;
    let runner = Runner {
        project: project.root.clone(),
        hooks: recipe.hooks.clone(),
        approvals: Approvals::open(hooks::path_in(&state_dir))?,
        enabled: request.hooks,
    };
    let message = message_of(request, &unit);
    let params = Params {
        project,
        unit,
        environment,
        target,
        head,
        message,
        stages: request.stages.clone(),
        resuming,
    };
    Ok(Prepared { params, runner })
}

/// The home the branch lives in, refused when it is not one this unit may act on.
///
/// A rebase in progress is the one half-finished Git operation a merge does not refuse,
/// because continuing it is what a second `nodal merge` is for. Every other one — a
/// merge, a cherry-pick, a bisect — is somebody else's unfinished work and is refused
/// exactly as a create refuses it.
fn home_of(environment: &Environment, unit: &Unit) -> Result<Git> {
    let home = &environment.home;
    if !home.is_dir() {
        return Err(Error::NotAHome { path: home.clone() });
    }
    marker::verify(home, unit.id)?;
    let git = Git::open(home)?;
    let report = git.preflight()?;
    let others: Vec<crate::git::preflight::State> = report
        .states
        .iter()
        .copied()
        .filter(|state| *state != crate::git::preflight::State::Rebase)
        .collect();
    if !others.is_empty() {
        return Err(Error::GitInProgress { repo: home.clone(), states: others });
    }
    // A rebase detaches HEAD, so a home stopped in one is asked which branch the rebase
    // is about instead. A rebase of another branch is not this unit's merge to resume.
    let on = if report.states.contains(&crate::git::preflight::State::Rebase) {
        git.rebasing_branch()?
    } else {
        git.current_branch()?
    };
    if on.as_deref() != Some(unit.branch.as_str()) {
        return Err(Error::MergeNotOnBranch { home: home.clone(), branch: unit.branch.clone() });
    }
    Ok(git)
}

/// The branch the unit's work merges into, and where it stands now.
///
/// The unit's own parent branch answers first, because it is what the work started
/// from. After it come the branch a clone recorded as the project's default, and then
/// the two names a repository without that record uses. Every candidate is a local
/// branch of the project's own checkout: that is the branch this command moves, and a
/// remote-tracking copy is not something a fast-forward may write.
fn resolve(project: &Project, unit: &Unit) -> Result<Target> {
    let git = Git::open(&project.root)?;
    let mut tried = Vec::new();
    for candidate in candidates(&git, unit)? {
        if tried.contains(&candidate) {
            continue;
        }
        if let Some(oid) = git.read_ref(&format!("refs/heads/{candidate}"))? {
            return Ok(Target { branch: candidate, oid });
        }
        tried.push(candidate);
    }
    Err(Error::MergeNoTarget { project: project.root.clone(), slug: unit.slug.clone(), tried })
}

/// The branch names to try, best first.
fn candidates(git: &Git, unit: &Unit) -> Result<Vec<String>> {
    let mut names: Vec<String> = unit.parent_branch.iter().map(ToString::to_string).collect();
    if let Some(reference) = git.symbolic_ref(ORIGIN_HEAD)? {
        names.extend(short_branch(&reference).map(ToOwned::to_owned));
    }
    names.extend(FALLBACK_BRANCHES.iter().map(|name| (*name).to_owned()));
    Ok(names)
}

/// The branch a remote-tracking ref names, without the remote.
fn short_branch(reference: &str) -> Option<&str> {
    reference.strip_prefix("refs/remotes/origin/")
}

/// The message the commit and the squash carry: what was asked for, then what the unit
/// says it is for, then its handle.
fn message_of(request: &Request, unit: &Unit) -> String {
    request.message.clone().unwrap_or_else(|| {
        unit.objective.as_ref().map_or_else(|| unit.slug.to_string(), ToString::to_string)
    })
}

/// The unit's newest materialisation, refusing one that has been reclaimed already.
fn live(conn: &Connection, unit: &Unit) -> Result<Environment> {
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

// ---------------------------------------------------------------------------
// What the merge says it did.
// ---------------------------------------------------------------------------

/// What the plan will do, read from the repositories before it does any of it.
fn intent(params: &Params) -> Result<Vec<StageLine>> {
    let git = Git::at(params.home());
    let mut stages = Vec::new();
    if params.resuming {
        stages.push(StageLine::new("rebase", "continue the rebase this unit is stopped in"));
    } else if params.stages.runs(Stage::Commit) {
        stages.push(StageLine::new("commit", dirty(&git)?));
    }
    if params.stages.runs(Stage::Squash) && !params.resuming {
        stages.push(StageLine::new(
            "squash",
            format!("into one commit on {}: {}", params.target.branch, params.message),
        ));
    }
    if params.stages.runs(Stage::Rebase) && !params.resuming {
        stages.push(StageLine::new("rebase", format!("onto {}", params.target.branch)));
    }
    stages.push(StageLine::new(
        "forward",
        format!("{} in {}", params.target.branch, params.project.root.display()),
    ));
    stages.push(StageLine::new(
        "remove",
        if params.stages.runs(Stage::Remove) {
            "reclaim the unit to the trash"
        } else {
            "not asked for"
        },
    ));
    Ok(stages)
}

/// How much work is sitting in the home, in the words the plan uses.
fn dirty(git: &Git) -> Result<String> {
    let status = git.status()?;
    let count = status.entries.len();
    Ok(if count == 0 {
        String::from("nothing to commit")
    } else {
        format!("{count} changed path(s)")
    })
}

/// What the merge did, read back from the two repositories rather than asserted.
fn report(params: &Params, hooks: Vec<Ran>, forwarded: bool) -> Result<Merged> {
    let git = Git::at(params.home());
    let stopped = git.is_rebasing()?;
    let tip = if stopped { None } else { git.rev_parse_opt(params.unit.branch.as_str())? };
    let mut stages = Vec::new();
    if let Some(tip) = &tip {
        let folded = git.count(&format!("{}..{}", params.target.oid.as_str(), tip.as_str()))?;
        stages.push(StageLine::new("branch", format!("{} commit(s) at {}", folded, short(tip))));
    }
    stages.push(StageLine::new(
        "forward",
        if forwarded {
            format!("{} moved", params.target.branch)
        } else {
            format!("{} was already there", params.target.branch)
        },
    ));
    Ok(Merged {
        now: Timestamp::now(),
        slug: params.unit.slug.to_string(),
        branch: params.unit.branch.to_string(),
        target: Some(params.target.branch.clone()),
        premerge: Git::at(params.home()).read_ref(&params.premerge())?.map(|_| params.premerge()),
        stages,
        conflict: if stopped { Some(conflict(params)?) } else { None },
        hooks,
        reclaimed: None,
        refused: None,
        planned: false,
    })
}

/// What a person has to do about a rebase that stopped.
fn conflict(params: &Params) -> Result<crate::output::view::Conflict> {
    Ok(crate::output::view::Conflict {
        paths: plumbing::conflicts(params.home())?,
        home: params.home().to_path_buf(),
        resume: format!("nodal merge {}", params.unit.slug),
        abort: format!("nodal merge {} --abort", params.unit.slug),
    })
}

/// Reclaim the merged unit, and report a refusal rather than raising it.
///
/// The merge is already done when this runs: the target carries the work. A home the
/// reclaim will not take — an untracked file, a second branch with commits of its own —
/// is a reason to leave the unit alone and say so, not a reason to undo a merge.
fn remove(store: &mut Store, request: &Request, params: &Params, done: &mut Merged) {
    if !params.stages.runs(Stage::Remove) {
        done.stages.push(StageLine::new("remove", "not asked for"));
        return;
    }
    let asked = super::reclaim::Request {
        target: Some(params.unit.slug.to_string()),
        force: false,
        hooks: request.hooks,
        cwd: request.cwd.clone(),
    };
    match super::reclaim::reclaim(store, &asked) {
        Ok(report) => {
            done.stages.push(StageLine::new("remove", "the home is in the trash"));
            done.reclaimed = Some(Box::new(report));
        }
        Err(why) => {
            done.stages.push(StageLine::new("remove", "refused"));
            done.refused = Some(why.to_string());
        }
    }
}

/// A commit as a person names one.
fn short(oid: &Oid) -> String {
    oid.as_str().chars().take(8).collect()
}
