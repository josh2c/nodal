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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::git::{self, Git, scrub};
use crate::lifecycle::journal::Operation;
use crate::lifecycle::step::{Output, Outputs, nothing};
use crate::lifecycle::{Plan, Rebuild, Recovery, Step};
use crate::model::recipe::{PackageManager, Recipe};
use crate::model::{Base, BaseId, CommitId, Platform, ProjectId, Timestamp, WorkspaceFp};
use crate::store::bases;
use crate::substrate::progress::Reporter;
use crate::workspace::remove::tree as remove_tree;
use crate::workspace::{self, Excludes};
use crate::{Error, Result};

/// What this operation is called in the journal, and the kind [`Rebuilder`] claims.
pub const KIND: &str = "base build";

/// The remote a base is cloned from and fetched from.
pub const ORIGIN: &str = "origin";

/// What the end of a directory's name says, while a base is being assembled in it.
const PARTIAL_SUFFIX: &str = ".partial";

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
    /// Where its content comes from.
    pub origin: Origin,
    /// The checkout that asked for it, read only for objects the remote does not have.
    pub objects: PathBuf,
    /// Paths the recipe keeps out of a copy.
    pub excludes: Vec<PathBuf>,
    /// The package manager's install, as an argument list. Empty when the project has
    /// no package manager.
    pub install: Vec<String>,
    /// The project's build command, as an argument list. Empty unless a warm build was
    /// asked for and the command can run without a shell.
    pub warm: Vec<String>,
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
            progress: Arc::clone(progress),
        })
        .then(Checkout {
            destination: params.destination.clone(),
            commit: params.commit.clone(),
            objects: params.objects.clone(),
            progress: Arc::clone(progress),
        });
    for (name, argv, note) in tools(params) {
        if !argv.is_empty() {
            plan = plan.then(Tool {
                destination: params.destination.clone(),
                name,
                argv,
                note,
                progress: Arc::clone(progress),
            });
        }
    }
    Ok(plan)
}

/// The tool steps a build has, in order: install first, then the warm build that needs
/// what the install put there.
fn tools(params: &Params) -> [(&'static str, Vec<String>, String); 2] {
    [
        ("install", params.install.clone(), phrase("installing dependencies", &params.install)),
        ("warm", params.warm.clone(), phrase("warming the build", &params.warm)),
    ]
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
    fn partial(&self) -> PathBuf {
        self.beside(&format!(".{id}{PARTIAL_SUFFIX}", id = ulid::Ulid::new()))
    }

    /// A sibling of the destination, named by adding to the destination's own name.
    fn beside(&self, suffix: &str) -> PathBuf {
        let mut name = self.destination.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    }

    /// Remove what earlier attempts left beside the destination, and say nothing when
    /// one will not go: an orphan may still hold it, and the attempt after this one
    /// will find it free.
    fn sweep(&self) {
        let Some(parent) = self.destination.parent() else { return };
        let Some(name) = self.destination.file_name().and_then(std::ffi::OsStr::to_str) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(parent) else { return };
        for entry in entries.flatten() {
            let found = entry.file_name();
            let Some(found) = found.to_str() else { continue };
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
        let parent = self.destination.parent().unwrap_or(Path::new("."));
        let backend = workspace::select_backend(parent);
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

    /// Assemble the content beside the destination, then rename it into place.
    ///
    /// The rename is why the destination existing is enough to say the step is done.
    /// A clone or a copy that a kill stops half-way leaves a directory with a `.git`
    /// in it and most of a repository under that, and a resumed build that accepted
    /// one would install into a tree that is missing files. A rename is one step in
    /// the filesystem, so a base either has its name or has nothing.
    fn apply(&self) -> Result<Output> {
        if self.destination.exists() {
            self.progress.line("the base directory is already there");
            return Ok(nothing());
        }
        let parent = self.destination.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        self.sweep();
        let partial = self.partial();
        self.progress.line(&format!("building from {}", self.origin.describe()));
        match &self.origin {
            Origin::Remote { url } => {
                git::clone(url, &partial)?;
            }
            Origin::Checkout { path } => {
                git::clone(path, &partial)?;
            }
            Origin::Neighbour { path, .. } => self.copy(path, &partial)?,
        }
        // A copy inherits the source's worktree registrations, hooks path and HEAD; a
        // fresh clone inherits none of that and the scrub is a no-op on it.
        Git::open(&partial)?.scrub(&scrub::Options::default())?;
        std::fs::rename(&partial, &self.destination).map_err(Error::io(&partial))?;
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        self.sweep();
        remove(&self.destination)
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
        let git = Git::open(&self.destination)?;
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
    /// What the step is called in the journal.
    name: &'static str,
    /// The program and its arguments.
    argv: Vec<String>,
    /// The progress line it writes before it runs.
    note: String,
    /// Where the step says what it is doing.
    progress: Arc<dyn Reporter>,
}

impl Step for Tool {
    fn key(&self) -> String {
        String::from(self.name)
    }

    fn apply(&self) -> Result<Output> {
        self.progress.line(&self.note);
        run(&self.destination, &self.argv)?;
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        // As the checkout: what this wrote is inside the directory the first step
        // removes, and an install has no smaller inverse of its own.
        Ok(())
    }
}

/// The one place `substrate` starts a process that is not `git`.
///
/// Output is captured rather than inherited, so an install's thousands of lines do not
/// bury the progress the build is writing; what a failure wrote is carried in the
/// error instead.
fn run(dir: &Path, argv: &[String]) -> Result<()> {
    let Some((program, rest)) = argv.split_first() else {
        return Ok(());
    };
    let output = Command::new(program)
        .args(rest)
        .current_dir(dir)
        .output()
        .map_err(|source| Error::ToolSpawn { program: program.clone(), source })?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Tool {
        program: program.clone(),
        args: rest.to_vec(),
        dir: dir.to_path_buf(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_owned(),
    })
}

/// Remove a directory if it is there. Idempotent, which is what every undo has to be.
///
/// A base part-way through a build already holds a checkout, so it uses the removal
/// that opens read-only content rather than stopping at it.
fn remove(path: &Path) -> Result<()> {
    remove_tree(path)
}

/// Installing dependencies, as the recipe's package manager spells it.
///
/// Plain `install` rather than a frozen or offline form: a base is built for a
/// workspace fingerprint that includes the lockfile, so the lockfile is exactly what
/// the install is being asked to realise, and a base built for a branch that is
/// updating its dependencies must still build.
#[must_use]
pub fn install_argv(recipe: &Recipe) -> Vec<String> {
    let Some(manager) = recipe.package_manager else {
        return Vec::new();
    };
    let verb = match manager {
        PackageManager::Cargo => "fetch",
        PackageManager::Uv => "sync",
        PackageManager::Pnpm
        | PackageManager::Yarn
        | PackageManager::Npm
        | PackageManager::Bun
        | PackageManager::Poetry => "install",
    };
    vec![manager.program().to_owned(), verb.to_owned()]
}

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
    use super::{argv, install_argv, warm_argv};
    use crate::model::recipe::{CommandLine, PackageManager, Recipe};

    fn recipe(manager: Option<PackageManager>, build: Option<&str>) -> Recipe {
        let mut recipe = Recipe { package_manager: manager, ..Recipe::default() };
        recipe.commands.build = build.map(|line| CommandLine::parse(line).unwrap());
        recipe
    }

    #[test]
    fn each_package_manager_installs_in_its_own_words() {
        assert_eq!(install_argv(&recipe(Some(PackageManager::Pnpm), None)), ["pnpm", "install"]);
        assert_eq!(install_argv(&recipe(Some(PackageManager::Cargo), None)), ["cargo", "fetch"]);
        assert_eq!(install_argv(&recipe(Some(PackageManager::Uv), None)), ["uv", "sync"]);
    }

    #[test]
    fn a_project_with_no_package_manager_has_nothing_to_install() {
        assert!(install_argv(&recipe(None, None)).is_empty());
    }

    #[test]
    fn a_warm_build_runs_only_when_it_was_asked_for() {
        let recipe = recipe(Some(PackageManager::Pnpm), Some("pnpm run build"));
        assert!(warm_argv(&recipe, false).is_empty());
        assert_eq!(warm_argv(&recipe, true), ["pnpm", "run", "build"]);
    }

    #[test]
    fn a_command_line_written_for_a_shell_is_not_run() {
        assert_eq!(argv("pnpm run build"), Some(vec!["pnpm".into(), "run".into(), "build".into()]));
        assert_eq!(argv("pnpm build && pnpm test"), None);
        assert_eq!(argv("VAR=1 pnpm build > log"), None);
        assert_eq!(argv("   "), None);
    }
}
