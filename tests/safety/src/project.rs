//! A project to make units in, and the state directory they go in.
//!
//! Eight suites in the CLI crate each built this: a temporary directory holding a
//! one-commit repository and an empty state directory beside it, a runner for the
//! binary, and a handful of readings of what a command left on the disk. The copies
//! agreed on all of it and differed only in which methods each suite had needed so far,
//! so a suite that wanted one more reading grew a ninth copy of the rest. This is that
//! fixture, once.
//!
//! The repository is the smallest one a recipe can be inferred from: a package
//! manifest, one source file and an ignore file. What a test needs beyond that, it
//! writes itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_core::model::{Event, Unit};
use nodal_core::store::{events, projects, units};
use tempfile::TempDir;

use crate::runner::Runner;
use crate::state::InState;

/// What the project holds before its first commit.
///
/// The defaults are the project every suite started from. A suite states only the field
/// it needs another value for.
pub struct Layout<'a> {
    /// The ignore rules the first commit carries.
    pub ignore: &'a str,
    /// How many files are written under `vendor`, for a clone long enough to interrupt.
    pub bulk: usize,
}

impl Default for Layout<'_> {
    fn default() -> Self {
        Self { ignore: "node_modules/\n", bulk: 0 }
    }
}

/// A project, the state directory its units go in, and the binary that makes them.
pub struct Workspace {
    /// The temporary root, kept so that it outlives the test.
    root: TempDir,
    /// The binary under test, and where it writes.
    runner: Runner,
    /// The project's repository, which is what a merge fast-forwards.
    pub source: PathBuf,
    /// Nodal's state directory: the registry, every base, every home, and the trash.
    pub state: PathBuf,
}

impl InState for Workspace {
    fn state_dir(&self) -> &Path {
        &self.state
    }
}

impl Workspace {
    /// A one-commit repository and an empty state directory beside it.
    ///
    /// # Panics
    ///
    /// If the temporary directory or the repository could not be made.
    #[must_use]
    pub fn new(binary: impl AsRef<Path>) -> Self {
        Self::laid_out(binary, &Layout::default())
    }

    /// The same, with the project laid out as `layout` says.
    ///
    /// # Panics
    ///
    /// As [`Workspace::new`].
    #[must_use]
    pub fn laid_out(binary: impl AsRef<Path>, layout: &Layout<'_>) -> Self {
        let root = TempDir::new().expect("a temporary directory");
        let source = root.path().join("project");
        let state = root.path().join("state");
        std::fs::create_dir_all(source.join("app")).expect("the project directory is made");
        write(&source.join("app").join("main.txt"), "shared\n");
        write(&source.join("package.json"), "{\"name\":\"demo\"}\n");
        write(&source.join(".gitignore"), layout.ignore);
        write_bulk(&source.join("vendor"), layout.bulk);

        crate::git::init(&source, "main");
        crate::git::git_ok(&source, &["add", "-A"]);
        crate::git::git_ok(&source, &["commit", "-qm", "first"]);

        let runner = Runner::new(binary, &state, &source);
        Self { root, runner, source, state }
    }

    /// The same, with a recipe this test wrote and this machine has approved.
    ///
    /// Approving is what `nodal init` does, so the fixture runs it rather than writing
    /// the approvals file: an approval a test made by hand would not be evidence that
    /// the command a person runs makes one.
    ///
    /// # Panics
    ///
    /// As [`Workspace::new`], and if the approval failed.
    #[must_use]
    pub fn with_recipe(binary: impl AsRef<Path>, recipe: &str) -> Self {
        let workspace = Self::new(binary);
        workspace.approve_recipe(recipe);
        workspace
    }

    /// Put a recipe in the project and approve it, for a project already laid out.
    ///
    /// # Panics
    ///
    /// If the recipe could not be written, or the approval failed.
    pub fn approve_recipe(&self, recipe: &str) {
        self.write_recipe(recipe);
        drop(crate::text::stdout(&self.nodal(&["init", "--force"])));
    }

    /// The temporary root, which is where a test writes anything beside the project.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// Put a recipe in the project, without approving anything.
    ///
    /// # Panics
    ///
    /// If it could not be written.
    pub fn write_recipe(&self, recipe: &str) {
        write(&self.source.join("nodal.toml"), recipe);
    }

    /// `nodal` with this workspace's state directory, run in the project.
    ///
    /// # Panics
    ///
    /// If the binary could not be started. A command that ran and failed is an answer.
    #[must_use]
    pub fn nodal(&self, args: &[&str]) -> Output {
        self.runner.nodal(args)
    }

    /// The same invocation, run somewhere other than the project.
    ///
    /// # Panics
    ///
    /// As [`Workspace::nodal`].
    #[must_use]
    pub fn nodal_in(&self, cwd: &Path, args: &[&str]) -> Output {
        self.runner.nodal_in(cwd, args)
    }

    /// The invocation itself, not yet run.
    #[must_use]
    pub fn command(&self, args: &[&str]) -> Command {
        self.runner.command(args)
    }

    /// The same invocation, to be run somewhere other than the project.
    #[must_use]
    pub fn command_in(&self, cwd: &Path, args: &[&str]) -> Command {
        self.runner.command_in(cwd, args)
    }

    /// The same workspace, with one more variable in the environment of every command.
    ///
    /// This is how a suite gives its commands a home directory belonging to nobody, so
    /// that no test reads the start-up files of whoever is running it.
    #[must_use]
    pub fn with_env(
        mut self,
        name: impl Into<std::ffi::OsString>,
        value: impl Into<std::ffi::OsString>,
    ) -> Self {
        self.runner = self.runner.with_env(name, value);
        self
    }

    /// Make one unit and answer with its home.
    ///
    /// # Panics
    ///
    /// If the create failed, or left no home.
    #[must_use]
    pub fn unit(&self, slug: &str) -> PathBuf {
        drop(crate::text::stdout(&self.nodal(&["new", "--name", slug])));
        self.one_home()
    }

    /// The one unit this project has.
    ///
    /// # Panics
    ///
    /// If no command has registered the project, or no unit was made.
    #[must_use]
    pub fn one_unit(&self) -> Unit {
        let store = self.store();
        let project = projects::list(store.conn())
            .expect("the projects are readable")
            .pop()
            .expect("the project is known");
        units::list(store.conn(), project.id)
            .expect("the units are readable")
            .pop()
            .expect("a unit was made")
    }

    /// Every event the one unit this project has wrote.
    ///
    /// # Panics
    ///
    /// As [`Workspace::one_unit`].
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        let store = self.store();
        events::list_for_unit(store.conn(), self.one_unit().id).expect("the events are readable")
    }

    /// The one home this project has.
    ///
    /// # Panics
    ///
    /// If no unit has been materialised yet.
    #[must_use]
    pub fn one_home(&self) -> PathBuf {
        self.homes().pop().expect("the unit has a home")
    }
}

/// A path with every symbolic link on the way to it resolved, which is what a process
/// asked for its own working directory answers.
#[must_use]
pub fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Write a file, making the directories above it.
fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the directory above the file is made");
    }
    std::fs::write(path, contents).expect("the file is written");
}

/// Write `count` files into a directory, as an install writes a dependency tree.
fn write_bulk(directory: &Path, count: usize) {
    if count == 0 {
        return;
    }
    std::fs::create_dir_all(directory).expect("the directory is made");
    for index in 0..count {
        write(&directory.join(format!("{index}.js")), "module.exports = {};\n");
    }
}
