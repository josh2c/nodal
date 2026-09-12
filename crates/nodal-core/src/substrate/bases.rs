//! Getting a base: finding the warm one, or building the one that is missing.
//!
//! Nothing in Nodal asks a person to build a base. [`ensure`] is what `nodal new` calls
//! before it materialises a home, and it either finds a base for the workspace
//! fingerprint or builds one while saying so. `nodal base build` calls the same
//! function; it exists to warm a base before it is wanted, not to be a step anybody has
//! to remember.
//!
//! The key is the workspace fingerprint and the platform, so two branches whose
//! lockfiles differ get two bases and two branches that only differ in source share
//! one. That is the whole of the cache policy; [`super::lru`] is the whole of the
//! eviction policy.

use std::path::Path;
use std::sync::Arc;

use crate::fingerprint::{self, GitTreeAtCommit, TreeSource, current_platform};
use crate::git::Git;
use crate::lifecycle;
use crate::lifecycle::journal;
use crate::model::recipe::Recipe;
use crate::model::{
    Base, BaseId, CommitId, OperationId, Platform, Project, ProjectId, Timestamp, WorkspaceFp,
};
use crate::output::view::BaseRow;
use crate::store::{Store, bases, environments};
use crate::substrate::build::{self, Origin, Params, ThisHost};
use crate::substrate::lru;
use crate::substrate::pin;
use crate::substrate::progress::Reporter;
use crate::workspace::home;
use crate::workspace::remove::tree as remove_tree;
use crate::{Error, Result};

/// What a base is wanted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The project the base belongs to. Its name is what its directory is called.
    pub project: Project,
    /// The checkout that asked. Read for its remote, its commit and its tree; never
    /// copied.
    pub source: std::path::PathBuf,
    /// The project's effective recipe.
    pub recipe: Recipe,
    /// The state directory the base goes under, which a test may move.
    pub state_dir: std::path::PathBuf,
    /// Whether a build runs the project's build command once it has installed.
    pub warm: bool,
}

/// A base that is ready to be cloned from, and how it came to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The base.
    pub base: Base,
    /// The workspace key it answers.
    pub fingerprint: WorkspaceFp,
    /// Where its content came from, when this call built it. `None` when it was
    /// already there.
    pub origin: Option<Origin>,
}

impl Outcome {
    /// Whether this call built the base rather than finding it.
    #[must_use]
    pub const fn built(&self) -> bool {
        self.origin.is_some()
    }
}

/// The base a unit of this workspace is cloned from, built if there is not one yet.
///
/// The order here is the whole of the promise this module makes about a failed build.
/// The pin is acted on first, before a directory exists, so a host that cannot run the
/// project's package manager is told so rather than told so twenty thousand files
/// later. Then the failed attempts are looked at, so a clone that has already been paid
/// for is offered back instead of being made twice.
///
/// # Errors
/// [`Error::NotARepository`] when the source is not a checkout, [`Error::ToolPin`] when
/// the project pins a package-manager version this host cannot run,
/// [`Error::OperationStep`] when a build step failed, and whatever the registry
/// reports.
pub fn ensure(
    store: &mut Store,
    request: &Request,
    progress: &Arc<dyn Reporter>,
) -> Result<Outcome> {
    let git = Git::open(&request.source)?;
    let platform = current_platform()?;
    let commit = CommitId::parse(git.rev_parse("HEAD")?.to_string())?;
    let fingerprint = workspace_key(&git, &platform)?;
    if let Some(base) = warm(store, request.project.id, &fingerprint, &platform)? {
        progress.line(&format!("base {id} is warm for this workspace", id = base.id));
        return Ok(Outcome { base, fingerprint, origin: None });
    }
    let install = pin::install(&request.recipe, &ThisHost)?;
    let key = Key { fingerprint, platform, commit };
    if let Some(outcome) = carried_on(store, &key, progress)? {
        return Ok(outcome);
    }
    build_one(store, request, &key, &install, progress)
}

/// Offer the person what a failed attempt at this base left, and use it if they agree.
///
/// `None` when there is nothing to carry on with, or when the person would rather start
/// again. Nothing is removed in either case: the clone a failed attempt made is still
/// on disk, and a person who declined this offer has not asked for it to go.
fn carried_on(
    store: &mut Store,
    key: &Key,
    progress: &Arc<dyn Reporter>,
) -> Result<Option<Outcome>> {
    let Some(stopped) = stopped_at(store, key)? else { return Ok(None) };
    progress.line(&format!(
        "an earlier build of this base stopped at the {step} step: {why}",
        step = stopped.step,
        why = stopped.why
    ));
    if !progress.agrees(&format!("retry from the {step} step?", step = stopped.step)) {
        progress.line(&format!(
            "starting again; what the earlier build made is still at {kept}",
            kept = build::kept_at(&stopped.params.destination).display()
        ));
        return Ok(None);
    }
    let id = stopped.params.base;
    lifecycle::retry(store, stopped.operation, &build::plan(&stopped.params, progress)?)?;
    let base = bases::get(store.conn(), id)?
        .ok_or(Error::StoreMissingRow { table: "base", id: id.to_string() })?;
    Ok(Some(Outcome {
        base,
        fingerprint: key.fingerprint.clone(),
        origin: Some(stopped.params.origin),
    }))
}

/// A failed build of the base this workspace wants, and where it got to.
struct Stopped {
    /// The run, as the journal names it.
    operation: OperationId,
    /// What that run was building.
    params: Params,
    /// The step that failed, by its journal key.
    step: String,
    /// What the step reported, which for a tool holds the tail of both its streams.
    why: String,
}

/// The newest failed build for this workspace key whose work is still on the disk.
///
/// Still on the disk is half of the question. A person who removed what the attempt
/// left by hand has answered it, and being asked about a directory that is not there
/// would be a question with no good answer.
fn stopped_at(store: &Store, key: &Key) -> Result<Option<Stopped>> {
    for record in journal::failed(store.conn(), build::KIND)? {
        let Ok(params) = serde_json::from_value::<Params>(record.params.clone()) else { continue };
        if params.fingerprint != key.fingerprint || params.platform != key.platform {
            continue;
        }
        if !build::unfinished(&params.destination) {
            continue;
        }
        let Some((step, why)) = failing_step(store, record.id)? else { continue };
        return Ok(Some(Stopped { operation: record.id, params, step, why }));
    }
    Ok(None)
}

/// The step a failed run stopped at, and what it reported.
fn failing_step(store: &Store, id: OperationId) -> Result<Option<(String, String)>> {
    for step in journal::steps(store.conn(), id)? {
        if step.state != journal::StepState::Applying {
            continue;
        }
        let why = step
            .output
            .as_ref()
            .and_then(|output| output.get("failed"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("no reason was recorded")
            .to_owned();
        return Ok(Some((step.key, why)));
    }
    Ok(None)
}

/// The workspace key of a checkout's HEAD, on this platform.
fn workspace_key(git: &Git, platform: &Platform) -> Result<WorkspaceFp> {
    let entries = GitTreeAtCommit::new(git, "HEAD").entries()?;
    Ok(fingerprint::compute_workspace(&fingerprint::select(&entries), platform)?.key)
}

/// What a base is keyed by, and what its tree is put at.
struct Key {
    /// The workspace key.
    fingerprint: WorkspaceFp,
    /// The platform.
    platform: Platform,
    /// The commit the tree is put at.
    commit: CommitId,
}

/// The recorded base for a key, when there is one and its directory is still there.
///
/// A row whose directory has gone — a `nodal base gc` that was killed between the two
/// halves of an eviction, or a person clearing a disk — is removed rather than
/// returned, so the next step builds the base again instead of cloning from nothing.
fn warm(
    store: &Store,
    project: ProjectId,
    fingerprint: &WorkspaceFp,
    platform: &Platform,
) -> Result<Option<Base>> {
    let Some(base) = bases::find(store.conn(), project, fingerprint, platform)? else {
        return Ok(None);
    };
    if !base.path.is_dir() {
        tracing::warn!(base = %base.id, path = %base.path.display(), "base directory has gone");
        bases::delete(store.conn(), base.id)?;
        return Ok(None);
    }
    // A row is written after the mark comes off, so a marked directory under one is a
    // build that was undone or interrupted after it committed. Nothing is cloned from
    // it: its install or its warm build has no valid result. The row stays, because
    // the next build resumes into exactly this directory.
    if !build::is_built(&base.path) {
        tracing::warn!(base = %base.id, path = %base.path.display(), "base is still being built");
        return Ok(None);
    }
    bases::touch(store.conn(), base.id, Timestamp::now())?;
    Ok(Some(base))
}

/// Plan and run one build, then read back the row it committed.
fn build_one(
    store: &mut Store,
    request: &Request,
    key: &Key,
    install: &pin::Install,
    progress: &Arc<dyn Reporter>,
) -> Result<Outcome> {
    let id = BaseId::from_ulid(ulid::Ulid::new());
    let origin = origin_for(store, request, key)?;
    let params = Params {
        base: id,
        project: request.project.id,
        fingerprint: key.fingerprint.clone(),
        platform: key.platform.clone(),
        commit: key.commit.clone(),
        destination: home::for_base(&request.state_dir, &request.project.name, id),
        state_dir: request.state_dir.clone(),
        origin: origin.clone(),
        objects: request.source.clone(),
        excludes: request.recipe.base.exclude.clone(),
        install: install.argv.clone(),
        install_env: install.env.clone(),
        warm: build::warm_argv(&request.recipe, request.warm),
        planned_at: Timestamp::now(),
    };
    lifecycle::run(store, &build::plan(&params, progress)?)?;
    let base = bases::get(store.conn(), id)?
        .ok_or(Error::StoreMissingRow { table: "base", id: id.to_string() })?;
    progress.line(&format!("base {id} is built"));
    Ok(Outcome { base, fingerprint: key.fingerprint.clone(), origin: Some(origin) })
}

/// Where the new base's content comes from: the nearest base already here, or a clone
/// when this is the project's first base on this platform.
///
/// The clone is of the project's remote where there is one. A project that names no
/// remote — one that has not been pushed anywhere yet — is cloned from its own
/// checkout instead, which is a clone and not a copy: Git carries the committed
/// objects and builds the working tree from them, so the guarantee that no local state
/// reaches a base is the same one, from the same mechanism.
fn origin_for(store: &Store, request: &Request, key: &Key) -> Result<Origin> {
    let candidates: Vec<Base> = bases::list_for_project(store.conn(), request.project.id)?
        .into_iter()
        .filter(|base| base.platform == key.platform && build::is_built(&base.path))
        .collect();
    if let Some((base, distance)) = nearest(&candidates, &key.commit) {
        return Ok(Origin::Neighbour { base: base.id, path: base.path.clone(), distance });
    }
    let git = Git::open(&request.source)?;
    match git.remote_url(build::ORIGIN)? {
        Some(url) => Ok(Origin::Remote { url }),
        None => Ok(Origin::Checkout { path: url_of(&request.source)? }),
    }
}

/// A path as Git takes a URL. Refused rather than mangled when it is not UTF-8.
fn url_of(path: &Path) -> Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| Error::InvalidValue {
        kind: "checkout path",
        value: path.to_string_lossy().into_owned(),
    })
}

/// The base nearest to a commit, and how far away it is.
///
/// Distance is the size of the symmetric difference between the two commits, measured
/// inside the candidate, because that is what decides how much of its installed state
/// is still right. A candidate that does not have the commit yet cannot measure it and
/// is ranked last, but is still a better start than the network: its dependencies are
/// installed and the commit is one fetch away.
fn nearest<'a>(candidates: &'a [Base], commit: &CommitId) -> Option<(&'a Base, Option<u32>)> {
    let mut measured: Vec<(&Base, Option<u32>)> =
        candidates.iter().map(|base| (base, distance(base, commit))).collect();
    measured.sort_by_key(|(base, distance)| {
        (distance.unwrap_or(u32::MAX), std::cmp::Reverse(base.last_used), base.id)
    });
    measured.first().copied()
}

/// How far a base's commit is from another, `None` when it cannot be measured there.
fn distance(base: &Base, commit: &CommitId) -> Option<u32> {
    let git = Git::open(&base.path).ok()?;
    if !git.has_commit(commit.as_str()).ok()? {
        return None;
    }
    git.distance(base.commit.as_str(), commit.as_str()).ok()
}

/// Every base of a project, with the number of units holding it, newest use first.
///
/// # Errors
/// Whatever the registry reports.
pub fn list(store: &Store, project: ProjectId) -> Result<Vec<BaseRow>> {
    let mut rows = Vec::new();
    for base in bases::list_for_project(store.conn(), project)? {
        let pins = environments::count_for_base(store.conn(), base.id)?;
        rows.push(BaseRow { base, pins, disk_bytes: None });
    }
    rows.reverse();
    Ok(rows)
}

/// How many units hold a base against eviction.
///
/// # Errors
/// Whatever the registry reports.
pub fn pins(store: &Store, id: BaseId) -> Result<u32> {
    environments::count_for_base(store.conn(), id)
}

/// The base a name refers to. A full identifier, or enough of the front of one to be
/// unambiguous, which is what `nodal base ls` prints.
///
/// # Errors
/// [`Error::BaseUnknown`] when no base of the project starts with the name, or more
/// than one does.
pub fn resolve(store: &Store, project: ProjectId, name: &str) -> Result<Base> {
    let mut found = bases::list_for_project(store.conn(), project)?
        .into_iter()
        .filter(|base| base.id.to_string().starts_with(name));
    let first = found.next().ok_or_else(|| Error::BaseUnknown { name: name.to_owned() })?;
    if found.next().is_some() {
        return Err(Error::BaseUnknown { name: name.to_owned() });
    }
    Ok(first)
}

/// Remove one base: its directory first, then its row.
///
/// A base with units on it is refused. The directory goes before the row so that a
/// process killed between the two leaves a row pointing at nothing, which [`warm`]
/// clears the next time the key is wanted; the other order would leave a directory
/// nothing knows about, which nothing would ever clean up.
///
/// # Errors
/// [`Error::BasePinned`] when a unit still holds the base, [`Error::Io`] when the
/// directory could not be removed, and whatever the registry reports.
pub fn evict(store: &Store, id: BaseId, progress: &dyn Reporter) -> Result<Base> {
    let base =
        bases::get(store.conn(), id)?.ok_or_else(|| Error::BaseUnknown { name: id.to_string() })?;
    let pins = environments::count_for_base(store.conn(), id)?;
    if pins > 0 {
        return Err(Error::BasePinned { base: id, pins });
    }
    progress.line(&format!("removing base {id}"));
    remove(&base.path)?;
    bases::delete(store.conn(), id)?;
    Ok(base)
}

/// Evict the idle bases a project has beyond the number it keeps.
///
/// # Errors
/// As [`evict`], except that a pinned base is skipped here rather than refused: this
/// is the sweep, and refusing it would be refusing to collect anything.
pub fn gc(
    store: &Store,
    project: ProjectId,
    keep: usize,
    progress: &dyn Reporter,
) -> Result<Vec<Base>> {
    let candidates: Vec<lru::Candidate> = list(store, project)?
        .into_iter()
        .map(|row| lru::Candidate {
            id: row.base.id,
            last_used: row.base.last_used,
            pins: row.pins,
        })
        .collect();
    lru::evictable(&candidates, keep).into_iter().map(|id| evict(store, id, progress)).collect()
}

/// Remove a directory if it is there.
///
/// A base is a git checkout with its dependencies installed, so it is full of content
/// written read-only, and evicting one uses the removal that opens what it must.
fn remove(path: &Path) -> Result<()> {
    remove_tree(path)
}
