//! Building one base, as a lifecycle plan of steps with undo.
//!
//! A build is minutes of work — a clone over the network, an install that writes tens
//! of thousands of files, sometimes a compile — and the process doing it is a `nodal
//! new` a person may well interrupt. So it is a [`Plan`] like every other operation:
//! each step is idempotent, the journal records where it got to before and after each
//! one, and the registry row is written once at the end inside the same transaction
//! that closes the journal entry.
//!
//! It recovers by [`Recovery::Resume`] rather than by rolling back, which is the whole
//! reason that choice exists. Throwing away a clone and an install because a laptop
//! closed is the behaviour this module is here to avoid; the half-built directory is
//! nobody's home and harms nothing while it waits for the next invocation.
//!
//! Three origins, one plan. The first base of a project is a fresh clone: of the
//! project's remote, or of the checkout itself when the project names no remote. A
//! clone either way, so nothing local can reach it — not an uncommitted file, not a
//! stale `node_modules`, not a `.git` that belongs to another checkout. Every later
//! base is a copy-on-write copy of the nearest base already built here, which costs
//! metadata rather than a network round trip and leaves the installed dependencies in
//! place for the package manager to update rather than fetch.
//!
//! Every step of a build runs at the path the base is delivered at. A base assembled
//! under one name and renamed into another is a base that has done its work twice,
//! because the tools a build runs write the path they ran at into what they produce.
//! So the directory takes the base's own name from the clone onwards, and a mark
//! beside it says it is not a base yet until the last step takes the mark off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::git::{self, Git, scrub};
use crate::lifecycle::journal::Operation;
use crate::lifecycle::step::{Output, Outputs, nothing};
use crate::lifecycle::{Plan, Rebuild, Recovery, Step};
use crate::model::Digest;
use crate::model::base::Provenance;
use crate::model::recipe::{PackageManager, Recipe, ToolName, ToolVersion, VENV};
use crate::model::version::Version;
use crate::model::{Base, BaseId, CommitId, Platform, ProjectId, Timestamp, WorkspaceFp};
use crate::store::bases;
use crate::substrate::pin::Install;
use crate::substrate::progress::Reporter;
use crate::workspace::remove::tree as remove_tree;
use crate::workspace::sharing::Sharing;
use crate::workspace::{self, Excludes};
use crate::{Error, Result};

/// What this operation is called in the journal, and the kind [`Rebuilder`] claims.
pub const KIND: &str = "base build";

/// The remote a base is cloned from and fetched from.
pub const ORIGIN: &str = "origin";

/// What a name ends with while the base it belongs to is still being assembled.
///
/// A release before the mark assembled a base under this name and renamed it into
/// place at the end, and every attempt's scratch directory still carries it.
const PARTIAL_SUFFIX: &str = ".partial";

/// What the mark beside an unfinished base is named by.
const MARK_SUFFIX: &str = ".building";

/// How many lines of a failed tool's output an error carries, from each stream.
///
/// Enough to hold the part a package manager puts its reason in, and short enough that
/// an error is still something a person reads rather than scrolls.
const TAIL_LINES: usize = 40;

/// What the mark beside an unfinished base holds, for a person who opens it.
const MARK: &str = "this base is still being built\n";

/// The mark that says the directory at a base's own name is not a base yet.
///
/// A base is assembled at the name it is delivered at, and this file beside it is what
/// an unfinished one is told apart by — by a person, by `nodal doctor`, or by the next
/// build. [`Promote`] removes it, and removing it is the moment the base begins to
/// exist.
///
/// It is a mark and not a directory of its own because a build has to run the
/// project's install and its build command at the path the base is handed over at.
/// Those tools write the path they ran at into what they produce: Cargo records the
/// absolute path of every source file outside a package root, a Python environment
/// writes it into the first line of each script, and a Node install writes it into its
/// links. Work done at one path and delivered at another is work the next command
/// does again, which is the whole of what a warm base is for.
///
/// The name is stable, which is what makes a failed build retryable: the attempt that
/// resumes it finds the clone and the half-finished install waiting under the name the
/// base will keep.
///
/// It is named apart from [`legacy_of`] so that the two can be there at once. A build
/// carrying an earlier release's tree into place writes the mark first and moves the
/// tree second, and the two names not colliding is what makes that order possible.
#[must_use]
pub fn mark_of(destination: &Path) -> PathBuf {
    beside(destination, MARK_SUFFIX)
}

/// Where a release before the mark left the tree it was assembling.
#[must_use]
pub fn legacy_of(destination: &Path) -> PathBuf {
    beside(destination, PARTIAL_SUFFIX)
}

/// A sibling of a path, named by adding to the path's own name.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Whether a finished base is at this path.
///
/// Both halves are asked. A directory with the mark still beside it is a build that
/// stopped part of the way through, and nothing may be cloned from one: its install or
/// its warm build has no valid result, whatever the directory looks like from outside.
#[must_use]
pub fn is_built(destination: &Path) -> bool {
    destination.is_dir() && !mark_of(destination).exists() && !legacy_of(destination).is_dir()
}

/// Whether an earlier attempt at this base left work on the disk to carry on with.
///
/// Two shapes answer yes. The one this release writes is the directory at the base's
/// own name with the mark beside it. The other is the directory named by the mark,
/// which is where a release before this one assembled a base; a build resumes into
/// either, because a clone and an install cost minutes and belong to whichever attempt
/// finishes them.
#[must_use]
pub fn unfinished(destination: &Path) -> bool {
    legacy_of(destination).is_dir() || (destination.is_dir() && mark_of(destination).exists())
}

/// Where the work an unfinished build paid for is, for a progress line to name.
#[must_use]
pub fn kept_at(destination: &Path) -> PathBuf {
    let legacy = legacy_of(destination);
    if legacy.is_dir() { legacy } else { destination.to_path_buf() }
}

/// Write the mark that says the directory at `destination` is not a base yet.
fn mark(destination: &Path) -> Result<()> {
    let path = mark_of(destination);
    std::fs::write(&path, MARK).map_err(Error::io(&path))
}

/// Carry a tree an earlier release left beside the name into the name itself.
///
/// Every step of a build works at the path the base is delivered at, and a build that
/// an earlier release started left its clone and its install one name away. The
/// journal records that release's clone step as applied, so the step that would have
/// noticed is skipped by the attempt that resumes; this runs at the top of every step
/// that touches the tree instead, and does nothing at all once the tree is in place.
///
/// The mark is written before the tree moves, so the tree is never the only copy of
/// itself under a name nothing records. An attempt that dies between the two finds the
/// mark and the tree exactly where this left them, and the attempt after it finishes
/// the move. The tree is moved and never copied, so nothing here can lose it.
///
/// Both names being there at once is not a state this writes, and it is not one to
/// tidy away: a tree is what a build paid minutes for, and the only step that removes
/// one is the undo of the step that made it.
fn settle(destination: &Path) -> Result<()> {
    let legacy = legacy_of(destination);
    if !legacy.is_dir() || destination.exists() {
        return Ok(());
    }
    mark(destination)?;
    std::fs::rename(&legacy, destination).map_err(Error::io(&legacy))
}

/// The directory a step works in, with an earlier release's tree carried into it first.
///
/// # Errors
/// Whatever the move reports.
fn work(destination: &Path) -> Result<&Path> {
    settle(destination)?;
    Ok(destination)
}

/// Where a base's content comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "from")]
pub enum Origin {
    /// A fresh clone of the project's remote. The first base of a project that has
    /// one, always.
    Remote {
        /// The URL, as the checkout's `origin` gives it.
        url: String,
    },
    /// A fresh clone of the asking checkout, for a project that names no remote.
    ///
    /// A clone, not a copy. Git carries the objects across and makes the working tree
    /// from them, so an uncommitted file, an ignored directory and another tool's
    /// `.git` state stay where they are, exactly as they do for a clone of a remote.
    Checkout {
        /// The checkout's own path, which Git reads as a URL.
        path: String,
    },
    /// A copy-on-write copy of the nearest base already built on this machine.
    Neighbour {
        /// The base that was copied.
        base: BaseId,
        /// Where it is.
        path: PathBuf,
        /// How many commits separate the two, `None` when the neighbour could not
        /// measure it because it does not have the commit yet.
        distance: Option<u32>,
    },
}

impl Origin {
    /// A phrase for a progress line and for a report.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Remote { url } => format!("a fresh clone of {url}"),
            Self::Checkout { path } => format!("a fresh clone of the checkout at {path}"),
            Self::Neighbour { base, distance: Some(distance), .. } => {
                format!("base {base}, {distance} commit(s) away")
            }
            Self::Neighbour { base, distance: None, .. } => format!("base {base}"),
        }
    }
}

/// Everything one build was decided from.
///
/// This is what goes in the journal, so it holds the identifier the build generated and
/// the instant it was planned at as well as its inputs: a plan rebuilt from it has to
/// write the same row the interrupted one would have written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    /// The base being built.
    pub base: BaseId,
    /// The project it belongs to.
    pub project: ProjectId,
    /// The workspace key it is warm for.
    pub fingerprint: WorkspaceFp,
    /// The platform it is warm on.
    pub platform: Platform,
    /// The commit its working tree is put at.
    pub commit: CommitId,
    /// Where it goes.
    pub destination: PathBuf,
    /// The state root the destination is under. It is what the recorded answer about
    /// sharing file blocks is keyed by, and a build reads that record rather than
    /// asking a filesystem: a probe here would write into the project's bases
    /// directory, and a build rebuilt after a crash would write there again.
    pub state_dir: PathBuf,
    /// Where its content comes from.
    pub origin: Origin,
    /// The checkout that asked for it, read only for objects the remote does not have.
    pub objects: PathBuf,
    /// Paths the recipe keeps out of a copy.
    pub excludes: Vec<PathBuf>,
    /// Every package manager's install, in the order they run. Empty when the project
    /// has no package manager, and each is written through `corepack` or `mise` when
    /// the project pins a version and one of those is on the path ([`super::pin`]).
    pub installs: Vec<Install>,
    /// The project's build command, as an argument list. Empty unless a warm build was
    /// asked for and the command can run without a shell.
    pub warm: Vec<String>,
    /// The version of the Nodal that planned the build.
    pub nodal_version: Version,
    /// What every tool the recipe names answered when it was asked, at the plan.
    pub tools: BTreeMap<ToolName, ToolVersion>,
    /// The effective recipe the build was planned from, by content.
    pub recipe_digest: Digest,
    /// The instant the build was planned, which is what the row is stamped with.
    pub planned_at: Timestamp,
}

impl Params {
    /// The row this build commits.
    fn row(&self) -> Base {
        Base {
            id: self.base,
            project_id: self.project,
            ws_fingerprint: self.fingerprint.clone(),
            platform: self.platform.clone(),
            commit: self.commit.clone(),
            path: self.destination.clone(),
            built_at: self.planned_at,
            last_used: self.planned_at,
            provenance: Some(Provenance {
                nodal_version: self.nodal_version.clone(),
                install: self
                    .installs
                    .iter()
                    .flat_map(|install| [&install.prepare, &install.argv])
                    .filter(|argv| !argv.is_empty())
                    .cloned()
                    .collect(),
                warm: self.warm.clone(),
                tools: self.tools.clone(),
                recipe: self.recipe_digest.clone(),
            }),
        }
    }
}

/// The plan that builds one base.
///
/// # Errors
/// [`Error::Render`] when the parameters could not be written as JSON for the journal.
pub fn plan(params: &Params, progress: &Arc<dyn Reporter>) -> Result<Plan> {
    let row = params.row();
    let json =
        serde_json::to_value(params).map_err(|source| Error::Render { kind: KIND, source })?;
    let commit = Box::new(move |tx: &rusqlite::Transaction<'_>, _: &Outputs| {
        bases::insert(tx, &row)?;
        Ok(nothing())
    });
    let mut plan = Plan::new(KIND, params.base.to_string(), json, commit)
        .recovering(Recovery::Resume)
        .then(Materialise {
            destination: params.destination.clone(),
            origin: params.origin.clone(),
            excludes: params.excludes.clone(),
            state_dir: params.state_dir.clone(),
            progress: Arc::clone(progress),
        })
        .then(Checkout {
            destination: params.destination.clone(),
            commit: params.commit.clone(),
            objects: params.objects.clone(),
            progress: Arc::clone(progress),
        });
    for tool in tools(&params.installs, &params.destination, &params.warm) {
        if tool.argv.is_empty() {
            continue;
        }
        plan = plan.then(Tool {
            destination: params.destination.clone(),
            name: tool.name,
            manager: tool.manager,
            argv: tool.argv,
            env: tool.env,
            note: tool.note,
            progress: Arc::clone(progress),
        });
    }
    Ok(plan
        .then(Promote { destination: params.destination.clone(), progress: Arc::clone(progress) }))
}

/// One tool step of a build: what it is called in the journal, what it runs, and the
/// line a person watching it reads.
struct ToolStep {
    /// The journal key, which is what a resumed build and a failure report name.
    name: String,
    /// Whose install this is, for the two promises an install is held to
    /// ([`install`]). `None` for the warm build, which is the project's own command.
    manager: Option<PackageManager>,
    /// The program and its arguments.
    argv: Vec<String>,
    /// Variables it needs on top of the ones it inherits.
    env: Vec<(String, String)>,
    /// The progress line.
    note: String,
}

/// The tool steps a build has, in order: every install this base runs, then the warm
/// build that needs what the installs put there.
///
/// Each install is named after its own package manager, so that a repository of three
/// ecosystems has three keys in the journal rather than one key three times. A resumed
/// build then restarts at the manager it stopped on and does not repeat the two before
/// it.
///
/// An install the project excluded the output of is not a step here. Nothing would ever
/// read what it wrote: every home leaves that path out of the clone, so the base would
/// spend the minutes and hand over a tree no home receives that half of
/// ([`super::pin::Site`]). The create runs it in the home instead.
fn tools(installs: &[Install], destination: &Path, warm: &[String]) -> Vec<ToolStep> {
    let mut steps: Vec<ToolStep> = Vec::new();
    for install in installs.iter().filter(|install| install.at.in_base()) {
        let program = install.manager.program();
        if !install.prepare.is_empty() {
            steps.push(ToolStep {
                name: format!("install.{program}.environment"),
                manager: Some(install.manager),
                argv: runnable(&install.prepare, destination),
                env: install.env.clone(),
                note: phrase("making the environment", &install.prepare),
            });
        }
        let what = if install.lockfile.is_some() {
            "installing dependencies"
        } else {
            "installing dependencies with no lockfile to hold them to"
        };
        steps.push(ToolStep {
            name: format!("install.{program}"),
            manager: Some(install.manager),
            argv: runnable(&install.argv, destination),
            env: install.env.clone(),
            note: phrase(what, &install.argv),
        });
    }
    steps.push(ToolStep {
        name: String::from("warm"),
        manager: None,
        argv: warm.to_vec(),
        env: Vec::new(),
        note: phrase("warming the build", warm),
    });
    steps
}

/// The same argument list, with a program that lives inside the base named by its whole
/// path.
///
/// `pip` is run out of the environment the build made for it, so its program is
/// `.venv/bin/pip` — a path and not a name on the path. Rust does not say which
/// directory a relative program is resolved against when a command also sets its
/// working directory, so the one that is resolved here is the base's. A program with no
/// separator in it is a name for the path to find, and is left alone.
///
/// The progress line and the journal keep the relative form, because that is what the
/// recipe means and it is the same on every host.
///
/// A create resolves against the home for the same reason, for the install the base did
/// not run.
pub(crate) fn runnable(argv: &[String], destination: &Path) -> Vec<String> {
    let mut resolved = argv.to_vec();
    if let Some(program) = resolved.first_mut()
        && program.contains('/')
    {
        *program = destination.join(&*program).to_string_lossy().into_owned();
    }
    resolved
}

/// A progress line naming what is about to run.
fn phrase(what: &str, argv: &[String]) -> String {
    format!("{what}: {command}", command = argv.join(" "))
}

/// Rebuilds an interrupted base build from what the journal kept.
///
/// This goes in the table `nodal` passes to [`resolve`](crate::lifecycle::resolve),
/// which is how a build a killed process left half-done is finished by the next
/// invocation instead of being started again. It carries nothing, because the table is
/// a list of statics; a resumed build reports to standard error, which is where the
/// invocation that resumes it wants progress anyway.
pub struct BaseBuild;

impl Rebuild for BaseBuild {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn rebuild(&self, record: &Operation) -> Result<Plan> {
        let params: Params = serde_json::from_value(record.params.clone()).map_err(|_| {
            Error::InvalidValue { kind: "base build parameters", value: record.params.to_string() }
        })?;
        let progress: Arc<dyn Reporter> = Arc::new(crate::substrate::progress::Stderr);
        plan(&params, &progress)
    }
}

/// Put a repository at the destination: a fresh clone, or a copy of a neighbour.
struct Materialise {
    /// Where the base goes.
    destination: PathBuf,
    /// Where its content comes from.
    origin: Origin,
    /// Paths a copy leaves out.
    excludes: Vec<PathBuf>,
    /// The state root, which is what the recorded sharing answer is about.
    state_dir: PathBuf,
    /// Where the step says what it is doing.
    progress: Arc<dyn Reporter>,
}

impl Materialise {
    /// A directory of this attempt's own, beside the destination, to assemble in.
    ///
    /// Of this attempt's own, and not one name reused, because a `nodal` that a kill
    /// stops does not take its `git` with it. The clone keeps running, keeps writing,
    /// and finishes into a directory nobody is waiting for. An attempt that tried to
    /// clear that directory first would be racing a live writer, and would fail
    /// against it: a directory being written to cannot be removed. So each attempt
    /// takes a name no other attempt has, and what an orphan is still writing to is
    /// simply not in the way.
    fn scratch(&self) -> PathBuf {
        self.beside(&format!(".{id}{PARTIAL_SUFFIX}", id = ulid::Ulid::new()))
    }

    /// A sibling of the destination, named by adding to the destination's own name.
    fn beside(&self, suffix: &str) -> PathBuf {
        let mut name = self.destination.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    }

    /// Remove the scratch directories earlier attempts left beside the destination,
    /// and say nothing when one will not go: an orphan may still hold it, and the
    /// attempt after this one will find it free.
    ///
    /// The base's own mark is never swept, and neither is the directory an earlier
    /// release left under that name. Either holds a clone that a failed build paid
    /// for, and the next attempt resumes into it; removing one here would be the
    /// discarded clone this module exists to prevent, done by the tidying rather than
    /// by the failure.
    fn sweep(&self) {
        let Some(parent) = self.destination.parent() else { return };
        let Some(name) = self.destination.file_name().and_then(std::ffi::OsStr::to_str) else {
            return;
        };
        let kept = format!("{name}{PARTIAL_SUFFIX}");
        let Ok(entries) = std::fs::read_dir(parent) else { return };
        for entry in entries.flatten() {
            let found = entry.file_name();
            let Some(found) = found.to_str() else { continue };
            if found == kept {
                continue;
            }
            if found.starts_with(&format!("{name}.")) && found.ends_with(PARTIAL_SUFFIX) {
                drop(std::fs::remove_dir_all(entry.path()));
            }
        }
    }

    /// Copy the nearest base, which costs metadata on a filesystem that shares blocks.
    ///
    /// The exclusion list is settled against the source's own commit first. A base that
    /// is missing a path the commit tracks is dirty before a unit is cloned from it, so
    /// a default row the commit tracks yields and the copy says which one.
    fn copy(&self, source: &Path, into: &Path) -> Result<()> {
        let backend = workspace::select_backend(&Sharing::ensure(&self.state_dir));
        let mut excludes = Excludes::with_recipe(&self.excludes);
        for kept in workspace::tracked::enforce(source, &mut excludes)? {
            self.progress.line(&kept.to_string());
        }
        let report = backend.clone_tree(source, into, &excludes)?;
        self.progress.line(&format!(
            "copied {files} file(s) with the {backend} backend",
            files = report.files,
            backend = backend.name()
        ));
        Ok(())
    }
}

impl Step for Materialise {
    fn key(&self) -> String {
        String::from("clone")
    }

    /// Assemble the content in a scratch directory, then rename it to the base's name.
    ///
    /// The rename is why the directory existing is enough to say the step is done. A
    /// clone or a copy that a kill stops half-way leaves a directory with a `.git` in
    /// it and most of a repository under that, and a resumed build that accepted one
    /// would install into a tree that is missing files. A rename is one step in the
    /// filesystem, so the base's directory either holds a whole clone or does not
    /// exist.
    ///
    /// The scratch name is this attempt's own, and the base's is not. A `nodal` a kill
    /// stops does not take its `git` with it, so an attempt that assembled directly
    /// into the shared name would be racing a live writer. It assembles somewhere
    /// nobody else can be and takes the shared name in one move.
    ///
    /// The mark is written before that move and not after it, so there is no instant
    /// in which the directory stands at a base's name with nothing saying it is not
    /// one yet.
    fn apply(&self) -> Result<Output> {
        let destination = work(&self.destination)?;
        if destination.is_dir() {
            if mark_of(destination).exists() {
                self.progress.line("carrying on with the clone the last attempt made");
            } else {
                self.progress.line("the base directory is already there");
            }
            return Ok(nothing());
        }
        let parent = self.destination.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        self.sweep();
        let scratch = self.scratch();
        self.progress.line(&format!("building from {}", self.origin.describe()));
        match &self.origin {
            Origin::Remote { url } => {
                git::clone(url, &scratch)?;
            }
            Origin::Checkout { path } => {
                git::clone(path, &scratch)?;
            }
            Origin::Neighbour { path, .. } => self.copy(path, &scratch)?,
        }
        // A copy inherits the source's worktree registrations, hooks path and HEAD; a
        // fresh clone inherits none of that and the scrub is a no-op on it.
        Git::open(&scratch)?.scrub(&scrub::Options::default())?;
        mark(&self.destination)?;
        std::fs::rename(&scratch, &self.destination).map_err(Error::io(&scratch))?;
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        self.sweep();
        remove(&legacy_of(&self.destination))?;
        remove(&self.destination)?;
        let mark = mark_of(&self.destination);
        if mark.is_file() {
            std::fs::remove_file(&mark).map_err(Error::io(&mark))?;
        }
        Ok(())
    }
}

/// Put the base's working tree at the commit it is keyed to.
struct Checkout {
    /// The base.
    destination: PathBuf,
    /// The commit it is keyed to.
    commit: CommitId,
    /// The checkout that asked for the base, for objects the remote does not have.
    objects: PathBuf,
    /// Where the step says what it is doing.
    progress: Arc<dyn Reporter>,
}

impl Checkout {
    /// Make sure the commit is in the base's object database.
    ///
    /// The remote is asked first, always. Only when the remote does not have the
    /// commit — a branch nobody has pushed, which is a normal way to start a unit — are
    /// the objects taken from the checkout that asked for the base. Objects, not files:
    /// the working tree still comes from the clone, so nothing local is in the base.
    fn reach(&self, git: &Git) -> Result<()> {
        let rev = self.commit.as_str();
        if git.has_commit(rev)? {
            return Ok(());
        }
        self.progress.line("fetching the commit the base is keyed to");
        if git.fetch(ORIGIN).is_ok() && git.has_commit(rev)? {
            return Ok(());
        }
        self.progress.line("the remote does not have it; taking the objects from the checkout");
        git.fetch_from(&self.objects, rev)?;
        if git.has_commit(rev)? {
            return Ok(());
        }
        Err(Error::BaseCommitMissing { commit: rev.to_owned() })
    }
}

impl Step for Checkout {
    fn key(&self) -> String {
        String::from("checkout")
    }

    fn apply(&self) -> Result<Output> {
        let git = Git::open(work(&self.destination)?)?;
        self.reach(&git)?;
        self.progress.line(&format!("checking out {}", self.commit));
        git.checkout_detached(self.commit.as_str())?;
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        // The step before this one owns the directory, and removing it takes the
        // checkout with it. There is nothing smaller to take back.
        Ok(())
    }
}

/// Run one tool in the base: the package manager's install, or the build command.
struct Tool {
    /// Where it runs.
    destination: PathBuf,
    /// What the step is called in the journal. One install step per package manager,
    /// so the name carries which manager it belongs to.
    name: String,
    /// Whose install this is; `None` for the warm build.
    manager: Option<PackageManager>,
    /// The program and its arguments.
    argv: Vec<String>,
    /// Variables it needs on top of the ones it inherits.
    env: Vec<(String, String)>,
    /// The progress line it writes before it runs.
    note: String,
    /// Where the step says what it is doing.
    progress: Arc<dyn Reporter>,
}

impl Step for Tool {
    fn key(&self) -> String {
        self.name.clone()
    }

    fn apply(&self) -> Result<Output> {
        self.progress.line(&self.note);
        let dir = work(&self.destination)?;
        match self.manager {
            Some(manager) => install(dir, manager, &self.argv, &self.env)?,
            None => run(dir, &self.argv, &self.env)?,
        }
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        // As the checkout: what this wrote is inside the directory the first step
        // removes, and an install has no smaller inverse of its own.
        Ok(())
    }
}

/// Take the mark off the finished base.
///
/// The last step. Every step before it has worked at the name the base is delivered
/// at, with the mark beside it saying the tree there is not a base yet; this is the
/// moment a base begins to exist — and, because the registry write comes after it in
/// the same operation, the moment is one unlink away from the row that announces it.
///
/// Nothing moves here, and that is the point. A base whose install and whose warm
/// build ran at one path and was handed over at another is a base that has to do that
/// work again, because the tools record the path they ran at in what they produce.
struct Promote {
    /// Where the base goes.
    destination: PathBuf,
    /// Where the step says what it is doing.
    progress: Arc<dyn Reporter>,
}

impl Step for Promote {
    fn key(&self) -> String {
        String::from("promote")
    }

    fn apply(&self) -> Result<Output> {
        let destination = work(&self.destination)?;
        // The tree is asked about before the mark is. A mark that is not there means
        // the step has already run, but only where the tree it handed over is still
        // standing; with no tree there is no base, and saying otherwise would write a
        // row naming a directory nobody can clone from.
        if !destination.is_dir() {
            return Err(Error::BaseGone { path: destination.to_path_buf() });
        }
        let mark = mark_of(destination);
        if !mark.exists() {
            return Ok(nothing());
        }
        std::fs::remove_file(&mark).map_err(Error::io(&mark))?;
        self.progress.line("the base is built");
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        // The mark goes back rather than the tree going away. The clone and the
        // install under it cost minutes and are still exactly what the next attempt
        // wants.
        if !self.destination.is_dir() || mark_of(&self.destination).exists() {
            return Ok(());
        }
        mark(&self.destination)
    }
}

/// The one place `substrate` starts a process that is not `git`.
///
/// Every install comes through [`install`] on its way here, so a tool failure is
/// reported the same way wherever the install ran.
///
/// Output is captured rather than inherited, so an install's thousands of lines do not
/// bury the progress the build is writing; what a failure wrote is carried in the
/// error instead.
///
/// Both streams are carried, not standard error alone. Which stream a tool writes its
/// reason to is the tool's choice: pnpm reports a lockfile mismatch on standard output
/// and exits non-zero with an empty standard error, and a build that kept only the
/// second reported a failure with no reason in it.
pub(crate) fn run(dir: &Path, argv: &[String], env: &[(String, String)]) -> Result<()> {
    let Some((program, rest)) = argv.split_first() else {
        return Ok(());
    };
    let output = capture(Some(dir), program, rest, env)?;
    if output.status.success() {
        return Ok(());
    }
    Err(failed(dir, program, rest, &output))
}

/// A tool that exited non-zero, as the error that carries the tail of both its streams.
fn failed(dir: &Path, program: &str, args: &[String], output: &std::process::Output) -> Error {
    Error::Tool {
        program: program.to_owned(),
        args: args.to_vec(),
        dir: dir.to_path_buf(),
        code: output.status.code(),
        output: Box::new(crate::error::Streams {
            stdout: tail(&output.stdout),
            stderr: tail(&output.stderr),
        }),
    }
}

/// Run one install and hold it to the two promises every install is held to.
///
/// **A tracked file is never written by Nodal.** Whatever the tool did, the tracked
/// paths of `dir` are read afterwards, and one the tool changed is put back and the
/// step refused with the file and the tool named ([`Error::InstallWroteTracked`]). The
/// reading is `git status` over tracked paths and the restore is `git checkout --` in
/// `dir`, which is a base or a home and never the person's checkout. It does not depend
/// on which tool ran, so it holds for a manager added after this was written.
///
/// **A lockfile that disagrees with its manifest is a refusal with the reason.** A
/// frozen install that exits non-zero with the tool's own sentence for that case
/// ([`PackageManager::lockfile_disagrees`]) is reported as one
/// ([`Error::LockfileMismatch`]), naming the lockfile and the file it disagrees with.
///
/// A create reaches this too, for the install a base does not run
/// ([`crate::lifecycle::ops::new`]), so both copies are held to the same two promises
/// by the same code.
///
/// # Errors
/// [`Error::InstallWroteTracked`], [`Error::LockfileMismatch`], [`Error::Tool`] for
/// any other non-zero exit, [`Error::ToolSpawn`] when the program could not be
/// started, and [`Error::Git`] when the tree could not be read or put back.
pub(crate) fn install(
    dir: &Path,
    manager: PackageManager,
    argv: &[String],
    env: &[(String, String)],
) -> Result<()> {
    let Some((program, rest)) = argv.split_first() else {
        return Ok(());
    };
    let output = capture(Some(dir), program, rest, env)?;
    let restored = restore_tracked(dir)?;
    let tool = manager.program().to_owned();
    if output.status.success() {
        if restored.is_empty() {
            return Ok(());
        }
        return Err(Error::InstallWroteTracked { tool, paths: restored, dir: dir.to_path_buf() });
    }
    // Read off the whole of what the tool wrote, before it is tailed: npm follows its
    // sentence with sixty lines of usage, and a reading of the tail would never see it.
    if let Some(sentence) = disagreement(manager, &output) {
        return Err(Error::LockfileMismatch {
            tool,
            lockfile: lockfile_in(dir, manager),
            manifest: manager.manifest().to_owned(),
            sentence,
        });
    }
    Err(failed(dir, program, rest, &output))
}

/// The lockfile of `manager` that `dir` holds, by name, for a refusal to name.
///
/// A frozen install ran because one was there, so the first name that is not there
/// answers only for a tool that removed its own lockfile on the way out.
fn lockfile_in(dir: &Path, manager: PackageManager) -> String {
    let names = manager.lockfiles();
    names.iter().find(|name| dir.join(name).exists()).unwrap_or(&names[0]).to_string()
}

/// Put back every tracked path of `dir` a tool changed, and name them.
///
/// Read before the exit code is looked at, so that a tool that wrote a tracked file and
/// then failed leaves the copy as clean as one that wrote it and succeeded. Only paths
/// Git tracks are read: what an install writes under `node_modules` or `.venv` is the
/// install, and a copy is meant to hold it.
///
/// # Errors
/// [`Error::Git`] when `dir` could not be read or a path could not be put back.
fn restore_tracked(dir: &Path) -> Result<Vec<PathBuf>> {
    let git = Git::open(dir)?;
    let changed = git.changed_tracked()?;
    if !changed.is_empty() {
        git.restore(&changed)?;
    }
    Ok(changed)
}

/// The line on which `manager` said its lockfile disagrees with its manifest, if it did.
fn disagreement(manager: PackageManager, output: &std::process::Output) -> Option<String> {
    let phrases = manager.lockfile_disagrees();
    let (stdout, stderr) =
        (String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    [&stdout, &stderr]
        .into_iter()
        .flat_map(|stream| stream.lines())
        .find(|line| phrases.iter().any(|phrase| line.contains(phrase)))
        .map(|line| line.trim().to_owned())
}

/// The one spawn seam for every tool that is not `git`.
///
/// Both the install and the version probe that decides how to run it come through
/// here, because they start the same tool. One seam per tool is the rule the structure
/// ceilings in `ci/measure.sh` hold to.
///
/// # Errors
/// [`Error::ToolSpawn`] when the program could not be started at all.
fn capture(
    dir: Option<&Path>,
    program: &str,
    args: &[String],
    env: &[(String, String)],
) -> Result<std::process::Output> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().map_err(|source| Error::ToolSpawn { program: program.to_owned(), source })
}

/// The last [`TAIL_LINES`] lines of what a process wrote to one stream.
///
/// A tail and not the whole, because an install writes tens of thousands of lines and
/// an error nobody can read is an error without its reason. The end and not the start,
/// because a tool says what went wrong last.
fn tail(stream: &[u8]) -> String {
    let text = String::from_utf8_lossy(stream);
    let text = text.trim_end();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

/// This host, as [`pin`](super::pin) asks about it.
///
/// The version probe spawns a process, so it lives here: this module is the only one
/// in `substrate` that starts anything that is not `git`.
pub struct ThisHost;

impl super::pin::Host for ThisHost {
    fn on_path(&self, program: &str) -> bool {
        super::pin::on_path(program)
    }

    fn version(&self, program: &str) -> Option<String> {
        let asked = [String::from("--version")];
        let output = capture(None, program, &asked, &[]).ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines().next().map(|line| line.trim().to_owned()).filter(|line| !line.is_empty())
    }
}

/// Remove a directory if it is there. Idempotent, which is what every undo has to be.
///
/// A base part-way through a build already holds a checkout, so it uses the removal
/// that opens read-only content rather than stopping at it.
fn remove(path: &Path) -> Result<()> {
    remove_tree(path)
}

/// Installing dependencies, as one package manager spells it: frozen to `lockfile` when
/// the project carries one, plain when it carries none.
///
/// A base is built for a workspace fingerprint that includes the lockfile, so the
/// lockfile is exactly what the install is asked to realise, and an install that would
/// change it is asked to realise something else. A branch whose lockfile disagrees with
/// its manifest is refused with the tool's reason ([`Error::LockfileMismatch`]) rather
/// than built from a lockfile the tool rewrote on the way.
#[must_use]
pub fn install_argv(manager: PackageManager, lockfile: Option<&Path>) -> Vec<String> {
    // One table, two columns: the form that installs from the lockfile and refuses to
    // change it, and the plain form for a project that has none. Every home is a clone
    // of what the install left, so an install that rewrote the lockfile made every home
    // dirty at birth over a file nobody in it touched; `npm install` does that when the
    // lockfile's name disagrees with `package.json`, and it did on the founder's own
    // project. The frozen form cannot.
    //
    // `pip` is the row that is not a program on the path with a verb after it: it is
    // named by a path into the environment the build made for it
    // ([`super::pin::Install::prepare`]), and it is handed the file to install from.
    // The `pip` on the path would install into the host. Poetry has no frozen form.
    let program = manager.program().to_owned();
    let words: &[&str] = match (manager, lockfile) {
        (PackageManager::Pip, _) => {
            return vec![
                format!("{VENV}/bin/{program}"),
                String::from("install"),
                String::from("-r"),
                String::from(PIP_REQUIREMENTS),
            ];
        }
        (PackageManager::Cargo, Some(_)) => &["fetch", "--locked"],
        (PackageManager::Cargo, None) => &["fetch"],
        (PackageManager::Uv, Some(_)) => &["sync", "--frozen"],
        (PackageManager::Uv, None) => &["sync"],
        (PackageManager::Npm, Some(_)) => &["ci"],
        (PackageManager::Pnpm | PackageManager::Bun, Some(_)) => &["install", "--frozen-lockfile"],
        (PackageManager::Yarn, Some(_)) => &["install", "--immutable"],
        (
            PackageManager::Npm
            | PackageManager::Pnpm
            | PackageManager::Bun
            | PackageManager::Yarn
            | PackageManager::Poetry,
            _,
        ) => &["install"],
    };
    std::iter::once(program).chain(words.iter().map(|word| (*word).to_owned())).collect()
}

/// The file `pip` is handed. The same name the recipe reads `pip` out of
/// (`crate::recipe::infer::package_manager`), so the file that says the manager is
/// there is the file it installs from.
pub const PIP_REQUIREMENTS: &str = "requirements.txt";

/// The project's build command as an argument list, when a warm build was asked for.
#[must_use]
pub fn warm_argv(recipe: &Recipe, warm: bool) -> Vec<String> {
    if !warm {
        return Vec::new();
    }
    recipe.commands.build.as_ref().and_then(|line| argv(line.as_str())).unwrap_or_default()
}

/// Characters that mean a command line is written for a shell to read.
const SHELL_SYNTAX: &[char] = &[
    '|', '&', ';', '<', '>', '(', ')', '$', '`', '\\', '"', '\'', '*', '?', '[', ']', '{', '}',
    '~', '#', '!', '\n',
];

/// Split a recipe command line into a program and its arguments.
///
/// `None` when the line uses shell syntax. Nodal never spawns a shell, so a line it
/// cannot run itself is not run at all rather than run wrongly: a warm build is an
/// optimisation, and skipping one costs time where misreading one costs correctness.
fn argv(line: &str) -> Option<Vec<String>> {
    if line.contains(SHELL_SYNTAX) {
        return None;
    }
    let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    (!words.is_empty()).then_some(words)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Install, argv, install_argv, runnable, tools, warm_argv};
    use crate::model::recipe::{CommandLine, PackageManager, Recipe};
    use crate::substrate::pin::Site;

    fn recipe(managers: &[PackageManager], build: Option<&str>) -> Recipe {
        let mut recipe = Recipe { package_manager: managers.to_vec(), ..Recipe::default() };
        recipe.commands.build = build.map(|line| CommandLine::parse(line).unwrap());
        recipe
    }

    #[test]
    fn each_package_manager_installs_in_its_own_words() {
        assert_eq!(install_argv(PackageManager::Pnpm, None), ["pnpm", "install"]);
        assert_eq!(install_argv(PackageManager::Cargo, None), ["cargo", "fetch"]);
        assert_eq!(install_argv(PackageManager::Uv, None), ["uv", "sync"]);
        assert_eq!(
            install_argv(PackageManager::Pip, None),
            [".venv/bin/pip", "install", "-r", "requirements.txt"],
            "pip is run out of the environment the build made, and handed its file"
        );
    }

    /// A project that carries a lockfile is installed from it, in the form that refuses
    /// to change it. The plain form rewrote `package-lock.json` when its name disagreed
    /// with `package.json`, and every home was born dirty over it.
    #[test]
    fn a_project_with_a_lockfile_installs_frozen_to_it() {
        let frozen = |manager: PackageManager| {
            let name = manager.lockfiles()[0];
            install_argv(manager, Some(Path::new(name)))
        };
        assert_eq!(frozen(PackageManager::Npm), ["npm", "ci"]);
        assert_eq!(frozen(PackageManager::Pnpm), ["pnpm", "install", "--frozen-lockfile"]);
        assert_eq!(frozen(PackageManager::Yarn), ["yarn", "install", "--immutable"]);
        assert_eq!(frozen(PackageManager::Bun), ["bun", "install", "--frozen-lockfile"]);
        assert_eq!(frozen(PackageManager::Uv), ["uv", "sync", "--frozen"]);
        assert_eq!(frozen(PackageManager::Cargo), ["cargo", "fetch", "--locked"]);
        assert_eq!(frozen(PackageManager::Poetry), ["poetry", "install"], "no frozen form");
        assert_eq!(
            frozen(PackageManager::Pip),
            [".venv/bin/pip", "install", "-r", "requirements.txt"],
            "pip is already frozen to the file it is handed"
        );
    }

    /// The person hears, in the progress line, that an install has no lockfile to hold
    /// it to, so that a tool that writes one is no surprise.
    #[test]
    fn an_install_with_no_lockfile_says_so_in_its_line() {
        let plain = Install {
            manager: PackageManager::Pnpm,
            prepare: Vec::new(),
            argv: vec![String::from("pnpm"), String::from("install")],
            env: Vec::new(),
            at: Site::Base,
            lockfile: None,
        };
        let frozen = Install { lockfile: Some(PathBuf::from("pnpm-lock.yaml")), ..plain.clone() };
        let note = |install: &Install| {
            tools(std::slice::from_ref(install), Path::new("/base"), &[]).remove(0).note
        };
        assert_eq!(
            note(&plain),
            "installing dependencies with no lockfile to hold them to: pnpm install"
        );
        assert_eq!(note(&frozen), "installing dependencies: pnpm install");
    }

    /// A program named by a path is resolved against the base, because Rust does not
    /// say which directory a relative program is resolved against when the command also
    /// sets its working directory. A program that is a name is left for the path.
    #[test]
    fn a_program_inside_the_base_is_run_by_its_whole_path() {
        let base = Path::new("/state/b/ABCD1234");
        let inside = runnable(&install_argv(PackageManager::Pip, None), base);
        assert_eq!(inside[0], "/state/b/ABCD1234/.venv/bin/pip");
        assert_eq!(&inside[1..], ["install", "-r", "requirements.txt"]);

        let named = runnable(&install_argv(PackageManager::Pnpm, None), base);
        assert_eq!(named, ["pnpm", "install"], "a name is for the path to find");
        assert!(runnable(&[], base).is_empty(), "an empty list has no program to resolve");
    }

    #[test]
    fn a_warm_build_runs_only_when_it_was_asked_for() {
        let recipe = recipe(&[PackageManager::Pnpm], Some("pnpm run build"));
        assert!(warm_argv(&recipe, false).is_empty());
        assert_eq!(warm_argv(&recipe, true), ["pnpm", "run", "build"]);
    }

    /// An install the project excluded the output of is not a step of the base at all.
    /// The base used to spend the minutes and hand over a tree no home receives that
    /// half of.
    #[test]
    fn a_base_runs_no_install_whose_output_the_homes_never_receive() {
        let moved = Install {
            manager: PackageManager::Pnpm,
            prepare: Vec::new(),
            argv: vec![String::from("pnpm"), String::from("install")],
            env: Vec::new(),
            at: Site::Home { output: PathBuf::from("node_modules") },
            lockfile: None,
        };
        let kept = Install { at: Site::Base, ..moved.clone() };

        let names = |install: &Install| -> Vec<String> {
            tools(std::slice::from_ref(install), Path::new("/base"), &[])
                .into_iter()
                .map(|step| step.name)
                .collect()
        };
        assert_eq!(names(&kept), ["install.pnpm", "warm"]);
        assert_eq!(names(&moved), ["warm"], "the base would write what no home reads");
    }

    #[test]
    fn a_command_line_written_for_a_shell_is_not_run() {
        assert_eq!(argv("pnpm run build"), Some(vec!["pnpm".into(), "run".into(), "build".into()]));
        assert_eq!(argv("pnpm build && pnpm test"), None);
        assert_eq!(argv("VAR=1 pnpm build > log"), None);
        assert_eq!(argv("   "), None);
    }
}
