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

use crate::git::git;
use crate::runner::Runner;
use crate::state::InState;
use crate::text::stderr;

/// The name of the binary the suite drives.
const BINARY: &str = "nodal";

/// The variable that names the binary, for a runner that has it somewhere of its own.
pub const BINARY_VAR: &str = "NODAL_TEST_BINARY";

/// What the stub package manager writes, so that a base holds a file no commit put there.
const INSTALLED: &str = "node_modules/installed.txt";

/// Where the stub records the directory it was asked to build in.
///
/// A warm build is the one step whose whole value is the path it ran at, so the stub
/// writes that path down and the suite reads it back. It goes under `node_modules`
/// because that directory is carried into a base and into every home, so the reading
/// survives the copy the property is about.
const BUILT_IN: &str = "node_modules/built-in.txt";

/// The stub package manager itself. It writes one file and succeeds.
///
/// It answers `--version` as well, with the version the fixture's manifest pins. Nodal
/// compares the two before it builds, and a stub that answered nothing would be a host
/// without a package manager at all.
///
/// `run` is the fixture's build command, which a warm base build runs after the
/// install. It records the directory it ran in and writes nothing else, so a property
/// about where the warm step happened reads one file and nothing else changes.
const STUB: &str = concat!(
    "#!/bin/sh\n",
    "case \"$1\" in --version) echo '@VERSION@'; exit 0;; esac\n",
    "mkdir -p node_modules || exit 1\n",
    "case \"$1\" in run) pwd -P > node_modules/built-in.txt; exit 0;; esac\n",
    "echo 'the safety suite installs nothing' >> node_modules/installed.txt\n",
);

/// A stub that installs and then refuses to build.
///
/// It writes its reason to standard output and a note to standard error, for the same
/// reason [`STUB_FAILS_ONCE`] does: which stream a tool puts its reason on is the
/// tool's choice.
const STUB_WARM_FAILS: &str = concat!(
    "#!/bin/sh\n",
    "case \"$1\" in --version) echo '@VERSION@'; exit 0;; esac\n",
    "mkdir -p node_modules || exit 1\n",
    "case \"$1\" in run)\n",
    "  echo 'ERR_BUILD_FAILED  the build script exited non-zero'\n",
    "  echo 'a note that is not the reason' >&2\n",
    "  exit 1;;\n",
    "esac\n",
    "echo 'the safety suite installs nothing' >> node_modules/installed.txt\n",
);

/// The path in a stub script that the fixture replaces with a real one.
const WITNESS: &str = "@WITNESS@";

/// The version in a stub script that the fixture replaces with the one to report.
const VERSION: &str = "@VERSION@";

/// A stub that fails the first time it is asked to install and succeeds after that.
///
/// It writes its reason to standard output and a note to standard error, because that
/// is how a package manager behaves and because an error that kept only the second
/// stream would carry the note and not the reason.
const STUB_FAILS_ONCE: &str = concat!(
    "#!/bin/sh\n",
    "case \"$1\" in --version) echo '@VERSION@'; exit 0;; esac\n",
    "if [ ! -f '@WITNESS@' ]; then\n",
    "  : > '@WITNESS@'\n",
    "  echo 'ERR_PNPM_OUTDATED_LOCKFILE  the lockfile does not match package.json'\n",
    "  echo 'a note that is not the reason' >&2\n",
    "  exit 1\n",
    "fi\n",
    "mkdir -p node_modules || exit 1\n",
    "case \"$1\" in run) pwd -P > node_modules/built-in.txt; exit 0;; esac\n",
    "echo 'the safety suite installs nothing' >> node_modules/installed.txt\n",
);

/// A stub that builds and takes the tree it was given with it.
///
/// A tool is free to remove what it was pointed at, and the last step of a build has to
/// answer for a tree that is not there rather than announce a base nobody can clone
/// from. It removes the mark beside the tree as well, which is why the pattern and not
/// the name: the suite does not spell what the mark is called.
const STUB_WARM_REMOVES_THE_TREE: &str = concat!(
    "#!/bin/sh\n",
    "case \"$1\" in --version) echo '@VERSION@'; exit 0;; esac\n",
    "mkdir -p node_modules || exit 1\n",
    "case \"$1\" in run)\n",
    "  here=$(pwd -P)\n",
    "  cd / || exit 1\n",
    "  rm -rf \"$here\" \"$here\".*\n",
    "  exit 0;;\n",
    "esac\n",
    "echo 'the safety suite installs nothing' >> node_modules/installed.txt\n",
);

/// A stub that refuses to build the first time and builds after that.
///
/// The retry is the property: a base build whose last step failed keeps its clone and
/// its install, and the attempt after it carries on at the same path.
const STUB_WARM_FAILS_ONCE: &str = concat!(
    "#!/bin/sh\n",
    "case \"$1\" in --version) echo '@VERSION@'; exit 0;; esac\n",
    "mkdir -p node_modules || exit 1\n",
    "case \"$1\" in run)\n",
    "  if [ ! -f '@WITNESS@' ]; then\n",
    "    : > '@WITNESS@'\n",
    "    echo 'ERR_BUILD_FAILED  the build script exited non-zero'\n",
    "    echo 'a note that is not the reason' >&2\n",
    "    exit 1\n",
    "  fi\n",
    "  pwd -P > node_modules/built-in.txt\n",
    "  exit 0;;\n",
    "esac\n",
    "echo 'the safety suite installs nothing' >> node_modules/installed.txt\n",
);

/// A stub that installs, and answers `--version` with a version the test chooses.
const STUB_VERSIONED: &str = concat!(
    "#!/bin/sh\n",
    "case \"$1\" in --version) echo '@VERSION@'; exit 0;; esac\n",
    "mkdir -p node_modules || exit 1\n",
    "echo 'the safety suite installs nothing' >> node_modules/installed.txt\n",
);

/// The programs a sealed machine keeps, beyond the stub package manager.
///
/// A machine that asserts what Nodal does when the host cannot satisfy a pin has to be
/// one where `corepack` and `mise` are certainly absent, and trimming the path to a
/// single directory is the only way to say that on a laptop where either may be
/// installed. Only the pin suite asks for this: a sealed machine cannot start a
/// tethered command, which most of the suite does.
///
/// `git` is linked in because Nodal is not a tool without it, and `mkdir` because the
/// stub package manager makes a directory.
const SEALED_TOOLS: [&str; 2] = ["git", "mkdir"];

/// A stub `corepack`, which every unsealed machine has in place of the real one.
///
/// The fixture pins its package manager and Nodal acts on the pin, so a build looks for
/// `corepack` before it looks at the host's own `pnpm`. The real one would fetch the
/// pinned version over the network and run the project's install for real, which is
/// what this suite exists not to do — and it would do it only on the machines that have
/// it, so the suite would behave one way on a laptop and another in CI. This one runs
/// the stub package manager instead, which is what the pin resolves to here.
///
/// It is invoked as `corepack <tool>@<version> <args>`, so it drops the first word.
const STUB_COREPACK: &str = concat!("#!/bin/sh\n", "shift\n", "exec pnpm \"$@\"\n");

/// A stub `mise`, in place of the real one, for the same reason.
///
/// It is invoked as `mise exec <tool>@<version> -- <argv>`, so it drops everything up
/// to the separator. A build reaches it only when there is no `corepack`.
const STUB_MISE: &str = concat!(
    "#!/bin/sh\n",
    "while [ \"$1\" != \"--\" ]; do shift || exit 1; done\n",
    "shift\n",
    "exec \"$@\"\n",
);

/// The stubs an unsealed machine puts in front of the host's own tools.
const SHADOWED: [(&str, &str); 2] = [("corepack", STUB_COREPACK), ("mise", STUB_MISE)];

/// The identity commits in this machine are made with. A home is a clone and carries no
/// identity of its own, as a person's checkout would from their global configuration.
const IDENTITY: [(&str, &str); 2] =
    [("user.email", "safety@nodal.invalid"), ("user.name", "Nodal safety suite")];

/// The fixture project, the state directory its units go in, and the command under test.
pub struct Machine {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// The binary under test, and where it writes.
    runner: Runner,
    /// The person's checkout: the fixture project, committed.
    pub source: PathBuf,
    /// Nodal's state directory: the registry, every base, every home and the trash.
    pub state: PathBuf,
}

impl InState for Machine {
    fn state_dir(&self) -> &Path {
        &self.state
    }
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
        Self::built(&Setup { forced, ..Setup::default() })
    }

    /// A machine whose package manager fails the first time it installs.
    ///
    /// The second `nodal new` finds the failed attempt and carries on with it, which is
    /// the behaviour `tests/base_retry.rs` is about.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`].
    #[must_use]
    pub fn failing_once() -> Self {
        Self::built(&Setup { stub: Some(STUB_FAILS_ONCE), sealed: true, ..Setup::default() })
    }

    /// A machine whose package manager installs and then refuses to build.
    ///
    /// `tests/base_warm.rs` is what this is for: a warm base build whose last step
    /// fails is the case where a directory that looks like a base holds a build that
    /// has no valid result.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`].
    #[must_use]
    pub fn failing_to_warm() -> Self {
        Self::built(&Setup { stub: Some(STUB_WARM_FAILS), ..Setup::default() })
    }

    /// A machine whose package manager refuses to build the first time only.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`].
    #[must_use]
    pub fn failing_to_warm_once() -> Self {
        Self::built(&Setup { stub: Some(STUB_WARM_FAILS_ONCE), ..Setup::default() })
    }

    /// A machine whose build command removes the tree it was given.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`].
    #[must_use]
    pub fn warming_into_nothing() -> Self {
        Self::built(&Setup { stub: Some(STUB_WARM_REMOVES_THE_TREE), ..Setup::default() })
    }

    /// A machine whose project pins `pin` and whose package manager reports `reports`.
    ///
    /// Sealed: nothing but the stub, `git` and `mkdir` is on its path, so `corepack`
    /// and `mise` are certainly not there and the pin is settled by comparing the two
    /// versions.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`].
    #[must_use]
    pub fn pinning(pin: &str, reports: &str) -> Self {
        Self::built(&Setup {
            stub: Some(STUB_VERSIONED),
            reports: Some(reports.to_owned()),
            pin: Some(pin.to_owned()),
            sealed: true,
            ..Setup::default()
        })
    }

    /// The same again, with `exclude` written into the project's own `nodal.toml`.
    ///
    /// A row here is one the person wrote, which is what tells the copy apart from a row
    /// of Nodal's default table: the person asked for it, so a tracked path under it is
    /// refused rather than kept.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`], and if the recipe could not be written.
    #[must_use]
    pub fn excluding(forced: &[&str], exclude: &[&str]) -> Self {
        Self::built(&Setup { forced, exclude, ..Setup::default() })
    }

    /// The fixture project as a repository, set up as `setup` asks.
    ///
    /// # Panics
    ///
    /// As [`Machine::tracking`].
    #[must_use]
    fn built(setup: &Setup<'_>) -> Self {
        let root = TempDir::new().expect("a temporary directory");
        let source = nodal_fixture::write(root.path().join("project"));
        let state = root.path().join("state");
        let tools = root.path().join("tools");
        let witness = tools.join("attempted").to_str().expect("a printable path").to_owned();
        let reports = setup.reports.as_deref().unwrap_or(nodal_fixture::PACKAGE_MANAGER_PIN);
        write_stub(&tools, setup.stub.unwrap_or(STUB), &witness, reports);
        if setup.sealed {
            link_tools(&tools);
        } else {
            shadow_tools(&tools);
        }

        if !setup.exclude.is_empty() {
            write_exclude(&source, setup.exclude);
        }
        if let Some(pin) = setup.pin.as_deref() {
            write_pin(&source, pin);
        }

        git(&source, &["init", "--quiet", "--initial-branch", "main"]);
        for (key, value) in IDENTITY {
            git(&source, &["config", "--local", key, value]);
        }
        git(&source, &["add", "--all"]);
        for path in setup.forced {
            git(&source, &["add", "--force", "--", path]);
        }
        git(&source, &["commit", "--quiet", "--message", "the fixture project"]);

        let search = if setup.sealed { tools.clone().into_os_string() } else { path(&tools) };
        let runner = Runner::new(binary(), &state, &source).with_env("PATH", search);
        Self { _root: root, runner, source, state }
    }

    /// `nodal` with this machine's state, run in the project.
    ///
    /// # Panics
    ///
    /// If the binary could not be started. A command that ran and failed is an answer,
    /// and several properties here are about a refusal.
    #[must_use]
    pub fn nodal(&self, args: &[&str]) -> Output {
        self.runner.nodal(args)
    }

    /// The same invocation, run somewhere other than the project.
    ///
    /// # Panics
    ///
    /// As [`Machine::nodal`].
    #[must_use]
    pub fn nodal_in(&self, cwd: &Path, args: &[&str]) -> Output {
        self.runner.nodal_in(cwd, args)
    }

    /// The invocation itself, not yet run.
    #[must_use]
    pub fn command(&self, args: &[&str]) -> Command {
        self.runner.command(args)
    }

    /// The same machine, with `named` set in the environment of every command it runs.
    ///
    /// One suite wants it. The refresh properties name Git's transport allow-list and
    /// its proxy command, so that a fetch which reached for anything but the filesystem
    /// would fail rather than quietly succeed on whichever host happened to be online.
    #[must_use]
    pub fn with_env(mut self, named: (&str, &str)) -> Self {
        self.runner = self.runner.with_env(named.0, named.1);
        self
    }

    /// Where this machine's commands keep the settings Claude Code reads.
    ///
    /// It is beside the state directory and it is not `$HOME/.claude`
    /// ([`crate::runner::Runner::config`]).
    #[must_use]
    pub fn config_dir(&self) -> &Path {
        self.runner.config()
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

    /// The file the stub package manager writes, relative to a tree's root.
    #[must_use]
    pub const fn installed() -> &'static str {
        INSTALLED
    }

    /// The file the stub build command writes, relative to a tree's root.
    #[must_use]
    pub const fn built_in() -> &'static str {
        BUILT_IN
    }
}

/// How one fixture machine differs from the plain one.
///
/// Every constructor above states only the field it needs another value for, so a
/// machine reads as the one thing the test is about.
#[derive(Default)]
struct Setup<'a> {
    /// Paths added to the first commit although the project ignores them.
    forced: &'a [&'a str],
    /// Paths written into the project's own `base.exclude`.
    exclude: &'a [&'a str],
    /// The stub package manager's script, when it is not the plain one.
    stub: Option<&'a str>,
    /// The version the stub answers `--version` with. The fixture's own pin unless the
    /// test is about a host that does not satisfy it.
    reports: Option<String>,
    /// Whether the path holds the stub package manager, `git` and `mkdir` and nothing
    /// else. Only the pin suite wants this; see [`SEALED_TOOLS`].
    sealed: bool,
    /// A package-manager pin written into the project's recipe.
    pin: Option<String>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

/// Add a `base.exclude` table to the fixture's own recipe, as a person would write it.
fn write_exclude(source: &Path, exclude: &[&str]) {
    let path = source.join(nodal_fixture::RECIPE);
    let recipe = std::fs::read_to_string(&path).expect("the fixture has a recipe");
    let rows: Vec<String> = exclude.iter().map(|path| format!("\"{path}\"")).collect();
    let written = format!("{recipe}\n[base]\nexclude = [{rows}]\n", rows = rows.join(", "));
    std::fs::write(&path, written).expect("the recipe is written");
}

/// Add a package-manager pin to the fixture's own recipe, as a manifest would carry it.
fn write_pin(source: &Path, pin: &str) {
    let path = source.join(nodal_fixture::RECIPE);
    let recipe = std::fs::read_to_string(&path).expect("the fixture has a recipe");
    let written = format!("package_manager = \"pnpm\"\npackage_manager_pin = \"{pin}\"\n{recipe}");
    std::fs::write(&path, written).expect("the recipe is written");
}

/// The search path a command gets: this machine's tools, then the real ones.
fn path(tools: &Path) -> std::ffi::OsString {
    let mut value = tools.to_path_buf().into_os_string();
    if let Some(inherited) = std::env::var_os("PATH") {
        value.push(":");
        value.push(inherited);
    }
    value
}

/// Put this suite's `corepack` and `mise` in front of whatever the host has.
fn shadow_tools(tools: &Path) {
    for (program, script) in SHADOWED {
        write_runnable(&tools.join(program), script);
    }
}

/// Link the programs a sealed machine still needs into its one tools directory.
fn link_tools(tools: &Path) {
    for program in SEALED_TOOLS {
        let Some(found) = which(program) else { continue };
        drop(std::fs::remove_file(tools.join(program)));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&found, tools.join(program))
            .unwrap_or_else(|_| panic!("{program} is linked into the sealed path"));
    }
}

/// Where a program is on the path of whoever is running the tests.
fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|directory| directory.join(program)).find(|p| p.is_file())
}

/// Write the stub package manager into `directory` and make it runnable.
fn write_stub(directory: &Path, script: &str, witness: &str, reports: &str) {
    std::fs::create_dir_all(directory).expect("the tools directory is created");
    let written = script.replace(WITNESS, witness).replace(VERSION, reports);
    write_runnable(&directory.join("pnpm"), &written);
}

/// Write one script and make it runnable.
fn write_runnable(path: &Path, script: &str) {
    std::fs::write(path, script).expect("the stub is written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is runnable");
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
