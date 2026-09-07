//! One machine to assert a property on: the fixture project, a state directory, and the
//! `nodal` a person would type.
//!
//! Every path here is temporary and is removed when the test ends. Three environment
//! variables move what Nodal would otherwise share with the person running the tests —
//! the state directory, the per-machine secrets file and the hook approvals — so a
//! safety test can never read or write any of them.
//!
//! ## The package manager
//!
//! The fixture is a pnpm project, so building its base runs `pnpm install`. The suite
//! puts a stub `pnpm` on the path of every command it runs, which writes one file into
//! `node_modules` and returns. That keeps the suite free of a network, of a toolchain
//! and of minutes of waiting on both platforms, and it costs nothing here: not one
//! property in this crate is about installing. `ci/acceptance-fixture.sh` is where the
//! fixture is installed and built for real.
//!
//! The one file the stub writes is deliberate. `node_modules` is a row of the exclusion
//! table Nodal keeps rather than drops, so that file is carried into every home, and the
//! base then holds something that is not a Git object for base immutability to be about.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_core::store::{Store, projects, units};
use tempfile::TempDir;

/// The name of the binary the suite drives.
const BINARY: &str = "nodal";

/// The variable that names the binary, for a runner that has it somewhere of its own.
pub const BINARY_VAR: &str = "NODAL_TEST_BINARY";

/// What the stub package manager writes, so that a base holds a file no commit put there.
const INSTALLED: &str = "node_modules/installed.txt";

/// The stub package manager itself. It writes one file and succeeds.
const STUB: &str = "#!/bin/sh\nmkdir -p node_modules || exit 1\necho 'the safety suite installs nothing' > node_modules/installed.txt\n";

/// The identity commits in this machine are made with. A home is a clone and carries no
/// identity of its own, as a person's checkout would from their global configuration.
const IDENTITY: [(&str, &str); 2] =
    [("user.email", "safety@nodal.invalid"), ("user.name", "Nodal safety suite")];

/// The fixture project, the state directory its units go in, and the command under test.
pub struct Machine {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// The person's checkout: the fixture project, committed.
    pub source: PathBuf,
    /// Nodal's state directory: the registry, every base, every home and the trash.
    pub state: PathBuf,
    /// Where the stub package manager is, which every command gets on its path.
    tools: PathBuf,
}

impl Machine {
    /// The fixture project as a repository with one commit, and an empty state directory.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`].
    #[must_use]
    pub fn new() -> Self {
        Self::tracking(&[])
    }

    /// The same, with `forced` added to the first commit although the project ignores it.
    ///
    /// This is what a project does when it commits a baseline report into a directory
    /// whose name says the content is generated. The name still says generated; the
    /// commit says the content is the project's own, and the commit wins.
    ///
    /// # Panics
    ///
    /// If the temporary directory, the fixture, the stub or the repository could not be
    /// made, which is a machine no property can be asserted on.
    #[must_use]
    pub fn tracking(forced: &[&str]) -> Self {
        let root = TempDir::new().expect("a temporary directory");
        let source = nodal_fixture::write(root.path().join("project"));
        let state = root.path().join("state");
        let tools = root.path().join("tools");
        write_stub(&tools);

        git(&source, &["init", "--quiet", "--initial-branch", "main"]);
        for (key, value) in IDENTITY {
            git(&source, &["config", "--local", key, value]);
        }
        git(&source, &["add", "--all"]);
        for path in forced {
            git(&source, &["add", "--force", "--", path]);
        }
        git(&source, &["commit", "--quiet", "--message", "the fixture project"]);

        Self { _root: root, source, state, tools }
    }

    /// `nodal` with this machine's state, run in the project.
    ///
    /// # Panics
    ///
    /// If the binary could not be started. A command that ran and failed is an answer,
    /// and several properties here are about a refusal.
    #[must_use]
    pub fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the binary runs")
    }

    /// The same invocation, run somewhere other than the project.
    ///
    /// # Panics
    ///
    /// As [`Machine::nodal`].
    #[must_use]
    pub fn nodal_in(&self, cwd: &Path, args: &[&str]) -> Output {
        let mut command = self.command(args);
        command.current_dir(cwd);
        command.output().expect("the binary runs")
    }

    /// The invocation itself, not yet run.
    #[must_use]
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(binary());
        command.args(args).current_dir(&self.source);
        command.env("NODAL_HOME", &self.state);
        // All three are shared by every unit on a machine, and a safety test must never
        // read or create the ones belonging to whoever is running it.
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command.env("NODAL_HOOKS_FILE", self.state.join("hooks.toml"));
        command.env("PATH", self.path());
        command
    }

    /// Make one unit and answer with its home, with an identity Git will commit under.
    ///
    /// # Panics
    ///
    /// If the create failed, or left no home, which no property here can go on without.
    #[allow(
        clippy::must_use_candidate,
        reason = "several properties only need the unit to exist, not its home"
    )]
    pub fn unit(&self, slug: &str) -> PathBuf {
        let made = self.nodal(&["new", "--name", slug]);
        assert!(made.status.success(), "nodal new {slug}: {}", stderr(&made));
        let home = self.home_of(slug);
        for (key, value) in IDENTITY {
            git(&home, &["config", "--local", key, value]);
        }
        home
    }

    /// Build the base every unit of this project will be cloned from, before any unit
    /// exists.
    ///
    /// A property about what a create does to its base cannot start from a base a create
    /// made: a create that wrote the same file into the base every time would then be
    /// invisible. So the base is built by the command whose job that is, and the reading
    /// is taken before the first create.
    ///
    /// # Panics
    ///
    /// If the build failed, or made no base.
    #[must_use]
    pub fn build_base(&self) -> PathBuf {
        let built = self.nodal(&["base", "build"]);
        assert!(built.status.success(), "nodal base build: {}", stderr(&built));
        self.base()
    }

    /// The home of the unit with this handle.
    ///
    /// # Panics
    ///
    /// If the registry holds no such unit, or it has no home.
    #[must_use]
    pub fn home_of(&self, slug: &str) -> PathBuf {
        let store = self.store();
        let project = self.project(&store);
        let units = units::list(store.conn(), project.id).expect("the units are readable");
        let unit = units
            .into_iter()
            .find(|unit| unit.slug.as_str() == slug)
            .unwrap_or_else(|| panic!("no unit called {slug}"));
        nodal_core::store::environments::list_for_unit(store.conn(), unit.id)
            .expect("the materialisations are readable")
            .pop()
            .unwrap_or_else(|| panic!("{slug} has no home"))
            .home
    }

    /// The registry, opened for reading what a command wrote.
    ///
    /// # Panics
    ///
    /// If it could not be opened.
    #[must_use]
    pub fn store(&self) -> Store {
        Store::open(self.registry()).expect("the registry opens")
    }

    /// Where the registry file is.
    #[must_use]
    pub fn registry(&self) -> PathBuf {
        self.state.join("registry.db")
    }

    /// The one project this machine has.
    ///
    /// # Panics
    ///
    /// If no command has registered it yet.
    #[must_use]
    pub fn project(&self, store: &Store) -> nodal_core::model::Project {
        projects::list(store.conn())
            .expect("the projects are readable")
            .pop()
            .expect("no command has registered the project yet")
    }

    /// Every live home of this project, in name order.
    #[must_use]
    pub fn homes(&self) -> Vec<PathBuf> {
        entries(self.segment("e"))
    }

    /// Every base of this project, in name order. One still being assembled carries a
    /// suffix and is not a base yet.
    #[must_use]
    pub fn bases(&self) -> Vec<PathBuf> {
        entries(self.segment("b"))
            .into_iter()
            .filter(|path| !path.to_string_lossy().ends_with(".partial"))
            .collect()
    }

    /// The one base every unit of this project is cloned from.
    ///
    /// # Panics
    ///
    /// If there is not exactly one, which every property here depends on.
    #[must_use]
    pub fn base(&self) -> PathBuf {
        let mut built = self.bases();
        assert_eq!(built.len(), 1, "one project, one base: {built:?}");
        built.pop().unwrap_or_default()
    }

    /// Everything this project's trash holds, in name order.
    #[must_use]
    pub fn trashed(&self) -> Vec<PathBuf> {
        entries(self.segment("trash"))
    }

    /// The file the stub package manager writes, relative to a tree's root.
    #[must_use]
    pub const fn installed() -> &'static str {
        INSTALLED
    }

    /// One segment of this project's directory, and nothing until something made it.
    fn segment(&self, name: &str) -> Option<PathBuf> {
        std::fs::read_dir(&self.state)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join(name))
            .find(|path| path.is_dir())
    }

    /// The search path a command gets: the stub package manager, then the real one.
    fn path(&self) -> std::ffi::OsString {
        let mut value = self.tools.clone().into_os_string();
        if let Some(inherited) = std::env::var_os("PATH") {
            value.push(":");
            value.push(inherited);
        }
        value
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

/// Write the stub package manager into `directory` and make it runnable.
fn write_stub(directory: &Path) {
    std::fs::create_dir_all(directory).expect("the tools directory is created");
    let path = directory.join("pnpm");
    std::fs::write(&path, STUB).expect("the stub package manager is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("the stub package manager is runnable");
    }
}

/// The binary under test.
///
/// A test in this crate cannot be given the path the way a test inside `nodal-cli` is,
/// because Cargo names a binary to the package that declares it and this is another
/// package. So it is found beside the test executable, which is where Cargo puts it, and
/// [`BINARY_VAR`] overrides that for a runner that keeps it somewhere else.
///
/// # Panics
///
/// If it is not there, with the command that builds it. A safety suite that quietly
/// skipped itself because a build step was missing would be worse than one that fails.
#[must_use]
pub fn binary() -> PathBuf {
    if let Some(named) = std::env::var_os(BINARY_VAR).filter(|value| !value.is_empty()) {
        return PathBuf::from(named);
    }
    let test = std::env::current_exe().expect("this test has a path");
    for directory in test.ancestors().skip(1).take(2) {
        let candidate = directory.join(BINARY);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!(
        "no `{BINARY}` beside {}: build it with `cargo build -p nodal-cli`, \
         or name it with {BINARY_VAR}",
        test.display()
    );
}

/// What is directly inside a directory, in name order, and nothing when there is no such
/// directory yet.
fn entries(directory: Option<PathBuf>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = directory
        .into_iter()
        .flat_map(|path| std::fs::read_dir(path).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

/// Standard output as text, with the command insisted upon.
///
/// # Panics
///
/// If the command failed, printing what it said about that.
#[must_use]
pub fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{}", stderr(output));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Standard error as text.
#[must_use]
pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// One `git` command in a directory, as trimmed text, with the call insisted upon.
///
/// The global and system configuration are shut out, so that a property holds whatever
/// the person running the tests has in theirs.
///
/// # Panics
///
/// If `git` is not there or refused the command.
pub fn git(directory: impl AsRef<Path>, args: &[&str]) -> String {
    let output = try_git(directory.as_ref(), args);
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        directory.as_ref().display(),
        stderr(&output)
    );
    String::from_utf8_lossy(&output.stdout).trim_end().to_owned()
}

/// The same, for a command that is asked because it may fail.
///
/// # Panics
///
/// If `git` is not there at all.
#[must_use]
pub fn try_git(directory: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs")
}
