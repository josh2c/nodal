//! The package-manager pin: what a project asks for, and what this host can run.
//!
//! A project pins its package manager in the manifest — `packageManager` in a
//! `package.json`, an `engines` table, a `mise.toml` row — and Nodal reads all three
//! into the recipe ([`Recipe::package_manager_pin`] and [`Recipe::toolchain`]). Until
//! this module existed the pin was read and then ignored: a base build ran whatever
//! `pnpm` the path happened to hold.
//!
//! That is the failure this module is here to stop, and it is worse than a crash. A
//! package manager one major version away from the pinned one does not refuse the
//! lockfile. It installs, exits zero, and leaves a tree that is subtly not the tree the
//! lockfile describes. Every unit cloned from that base inherits it, and the person who
//! finds the difference is debugging their own code, not their toolchain. cal.com and
//! formbricks both failed this way.
//!
//! So a pin is acted on, in this order:
//!
//! 1. `corepack` on the path runs the install at the pinned version, for the package
//!    managers Corepack owns. It fetches the version it is asked for.
//! 2. `mise` on the path does the same for every package manager, through `mise exec`.
//! 3. Neither, and the host's own version is compared to the pin. A different major
//!    series is refused, before the clone, with both versions named.
//!
//! Refusing before the clone is the point of the third case. A refusal that arrives
//! after a base has been cloned and half-installed has cost the person minutes and left
//! them a directory to think about.
//!
//! # An install whose output no home receives runs in the home
//!
//! `base.exclude` names paths a clone leaves out. A project that names an install output
//! there — `node_modules` is the one that was measured — used to get a base that ran the
//! install and a home that never received it: minutes of work, thrown away once per
//! project, and every home then reported `not ready: dependencies` truthfully.
//!
//! So the exclusion decides where the install runs ([`Site`]). The base skips it, because
//! nothing would ever read what it wrote; the home runs it, because the home is the tree
//! that needs it. Same install, same pin, same argument list — one directory later.
//!
//! # Which manager a pin belongs to
//!
//! A pin belongs to the manager whose manifest carries it. The `packageManager` field
//! lives in `package.json`. [`pinned`] gives it to the Node manager in the list,
//! whichever position that manager holds. A field that names its program belongs to
//! the manager it names. A field that names a version and no program still belongs to
//! the Node manager.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::recipe::{PackageManager, Recipe, VENV, Venv};
use crate::workspace::Excludes;
use crate::{Error, Result};

/// The variable that stops Corepack asking a person to confirm a download.
///
/// A base build has no terminal of its own: it runs under `nodal new`, under `nodal
/// base build` and under the resolver that finishes an interrupted one. Corepack
/// waiting for an answer nobody can give would hang the build rather than fail it.
const NO_DOWNLOAD_PROMPT: (&str, &str) = ("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0");

/// Poetry's own configuration file, and the table that moves its environment into the
/// project directory.
const POETRY_CONFIG: &str = "poetry.toml";

/// What a host can be asked about a tool.
///
/// A trait, so that the decision below is a pure function of the recipe and the
/// answers. The implementation that spawns a process lives in [`super::build`], which
/// is the one module in `substrate` that starts anything that is not `git`.
pub trait Host {
    /// Whether this program is on the path.
    fn on_path(&self, program: &str) -> bool;

    /// What `<program> --version` says, or `None` when it is not there or said
    /// nothing a version could be read out of.
    fn version(&self, program: &str) -> Option<String>;
}

/// Where one install has to run for its output to be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "site")]
pub enum Site {
    /// In the base, which every home receives a copy-on-write copy of.
    Base,
    /// In each home, because no home receives the base's copy of what this install
    /// writes.
    Home {
        /// The excluded path the install writes, which is why it moved.
        output: PathBuf,
    },
}

impl Site {
    /// Whether a base build runs this install.
    #[must_use]
    pub const fn in_base(&self) -> bool {
        matches!(self, Self::Base)
    }

    /// The excluded output that moved the install, when one did.
    #[must_use]
    pub const fn excluded(&self) -> Option<&PathBuf> {
        match self {
            Self::Base => None,
            Self::Home { output } => Some(output),
        }
    }
}

/// How a base build runs one package manager's install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Install {
    /// Whose install this is. The step it becomes is named after it, so a build of a
    /// repository of three ecosystems has three named steps rather than three called
    /// the same thing.
    pub manager: PackageManager,
    /// What makes the environment this manager installs into, for a manager that does
    /// not make its own. Empty for every other one, which is all but `pip`.
    ///
    /// Its own step, before the install, so that a build killed between the two resumes
    /// at the install rather than making the environment again.
    #[serde(default)]
    pub prepare: Vec<String>,
    /// The program and its arguments.
    pub argv: Vec<String>,
    /// Variables the install needs, on top of the ones it inherits.
    pub env: Vec<(String, String)>,
    /// Where it runs. An install the base would write and no home would receive runs in
    /// the home instead ([`Site`]).
    ///
    /// Defaulted when absent, so a plan rebuilt from a journal an earlier release wrote
    /// still rebuilds: that release ran every install in the base.
    #[serde(default = "in_the_base")]
    pub at: Site,
    /// The committed file the install is held to ([`PackageManager::lockfiles`]), or
    /// `None` for a project that carries none, whose install is the plain one and may
    /// write one. The name and not a flag, so the line a person reads can say which.
    ///
    /// Defaulted when absent: a journal an earlier release wrote recorded the plain
    /// argument list, and the plan rebuilt from it runs what it recorded.
    #[serde(default)]
    pub lockfile: Option<PathBuf>,
}

/// Where an install recorded before [`Site`] existed ran.
fn in_the_base() -> Site {
    Site::Base
}

impl Install {
    /// The same install, sited.
    ///
    /// A builder rather than a field on every construction, because where an install runs
    /// is a fact about the project's exclusions and every other field is a fact about the
    /// manager and the host. One place decides it ([`site`]), so a route through Corepack
    /// and a route through the bare host cannot be sited differently.
    #[must_use]
    fn at(mut self, site: Site) -> Self {
        self.at = site;
        self
    }
}

/// The interpreters a virtual environment is made with, in the order they are tried.
///
/// `python3` first, because a host that has both means the second one by the first. A
/// host with neither is refused rather than left to `pip`, which would install into
/// whatever the path holds.
const INTERPRETERS: [&str; 2] = ["python3", "python"];

/// Every install a base build runs, in the recipe's order, with each pin acted on.
///
/// One per package manager the recipe names, because a repository of three ecosystems
/// whose base installed one of them is a base that is warm for a third of its tree.
/// They run one after another; the price of "warm" being true is a longer build.
///
/// # Errors
/// [`Error::ToolPin`] when the project pins a version this host cannot run and nothing
/// on the path can fetch it. The caller has not made a directory yet, which is why this
/// is called where it is.
pub fn installs(recipe: &Recipe, host: &dyn Host, tree: &Path) -> Result<Vec<Install>> {
    let sites = sites(recipe, tree);
    sites
        .into_iter()
        .map(|(manager, site)| Ok(install(recipe, manager, host, tree)?.at(site)))
        .collect()
}

/// Where each of the recipe's managers has to install, in the recipe's order.
///
/// One walk of the managers and one reading of `tree`, which is what both callers need:
/// the whole list for a base build, and the moved part of it for a create.
fn sites(recipe: &Recipe, tree: &Path) -> Vec<(PackageManager, Site)> {
    let excludes = Excludes::with_recipe(&recipe.base.exclude);
    let venv = venv_of(tree);
    recipe
        .package_manager
        .iter()
        .map(|manager| (*manager, where_it_runs(*manager, &excludes, venv)))
        .collect()
}

/// Whether the project told Poetry to keep its environment beside the code.
///
/// The one reading of `poetry.toml`, here because this module owns the siting rule and
/// [`super::warmth`] already asks this module where an install runs. A file that is not
/// there, that will not parse, or that says nothing about the key answers
/// [`Venv::Outside`], which is Poetry's own default.
#[must_use]
pub fn venv_of(tree: &Path) -> Venv {
    let Ok(text) = std::fs::read_to_string(tree.join(POETRY_CONFIG)) else { return Venv::Outside };
    let asked = toml::from_str::<toml::Value>(&text)
        .ok()
        .and_then(|config| config.get("virtualenvs")?.get("in-project")?.as_bool())
        .unwrap_or(false);
    if asked { Venv::InProject } else { Venv::Outside }
}

/// Where one manager's install has to run for a home to read what it wrote.
///
/// Public because [`super::warmth`] says the same thing about the same manager: one rule,
/// so a base that skipped an install and a line that explains the skip cannot disagree.
///
/// The exclusion list is the same one a create builds ([`Excludes::with_recipe`]), so the
/// base and the home cannot disagree about which paths a clone leaves out. A manager that
/// writes nothing into the tree is never moved: nothing a clone does reaches the Cargo
/// home, and nothing reaches the environment Poetry keeps outside the project
/// ([`Venv`]).
#[must_use]
pub fn where_it_runs(manager: PackageManager, excludes: &Excludes, venv: Venv) -> Site {
    let Some(output) = manager.install_output(venv) else { return Site::Base };
    let output = Path::new(output);
    if excludes.excludes(output) { Site::Home { output: output.to_path_buf() } } else { Site::Base }
}

/// Every install the base does not run, because no home receives what it writes.
///
/// Empty for every project that excludes no install output. One walk of the managers,
/// and the host is asked about a tool only where there is an install to move: a manager
/// that installs in the base is not resolved here at all, so a pin it could not satisfy
/// is raised by the base build rather than by this call.
///
/// # Errors
/// As [`installs`].
pub fn in_the_home(recipe: &Recipe, host: &dyn Host, tree: &Path) -> Result<Vec<Install>> {
    sites(recipe, tree)
        .into_iter()
        .filter(|(_, site)| !site.in_base())
        .map(|(manager, site)| Ok(install(recipe, manager, host, tree)?.at(site)))
        .collect()
}

/// The install one manager runs, frozen to the lockfile `tree` carries and with its
/// own pin acted on.
fn install(
    recipe: &Recipe,
    manager: PackageManager,
    host: &dyn Host,
    tree: &Path,
) -> Result<Install> {
    let lockfile = lockfile_of(manager, tree);
    let pin = pinned(recipe, manager);
    let series = pin.as_deref().and_then(|pin| major(&version_of(pin, manager.program())));
    let argv = super::build::install_argv(manager, lockfile.as_deref(), series);
    let prepare = environment_for(manager, host)?;
    let plain = |argv: Vec<String>| Install {
        manager,
        prepare: prepare.clone(),
        argv,
        env: Vec::new(),
        at: Site::Base,
        lockfile: lockfile.clone(),
    };
    let (Some(pin), Some((program, rest))) = (pin, argv.split_first()) else {
        return Ok(plain(argv));
    };
    let wanted = version_of(&pin, program);

    if corepack_owns(manager) && host.on_path("corepack") {
        let mut through = vec![String::from("corepack"), format!("{program}@{wanted}")];
        through.extend_from_slice(rest);
        let (name, value) = NO_DOWNLOAD_PROMPT;
        let env = vec![(name.to_owned(), value.to_owned())];
        return Ok(Install { env, ..plain(through) });
    }

    if host.on_path("mise") {
        let mut through = vec![
            String::from("mise"),
            String::from("exec"),
            format!("{program}@{wanted}"),
            String::from("--"),
        ];
        through.extend_from_slice(&argv);
        return Ok(plain(through));
    }

    match satisfied(host, program, &wanted) {
        Verdict::Runnable => Ok(plain(argv)),
        Verdict::Refused { found } => {
            Err(Error::ToolPin { tool: program.clone(), wanted: stated(&pin, program), found })
        }
    }
}

/// The committed file `manager` is held to, when `tree` carries one.
///
/// The one reading of the lockfile's presence, here with the other readings of a working
/// copy; a refusal in [`super::build`] names the file by the same reading. The first
/// name of [`PackageManager::lockfiles`] that is there, so a repository carrying Bun's
/// two forms is held to the newer one.
pub(crate) fn lockfile_of(manager: PackageManager, tree: &Path) -> Option<PathBuf> {
    manager.lockfiles().iter().map(PathBuf::from).find(|name| tree.join(name).is_file())
}

/// What makes the environment `manager` installs into, when it does not make its own.
///
/// `uv` and Poetry make their own; every Node manager and Cargo install into a
/// directory rather than an interpreter. `pip` is the one that does not: handed no
/// environment it installs into whichever interpreter the path holds, which writes into
/// the host and not into the base. So the base makes one, and a host that cannot is
/// refused here, before anything is cloned.
///
/// # Errors
/// [`Error::NoInterpreter`] when the manager needs an environment and this host has no
/// interpreter to make one with.
fn environment_for(manager: PackageManager, host: &dyn Host) -> Result<Vec<String>> {
    if manager != PackageManager::Pip {
        return Ok(Vec::new());
    }
    let Some(interpreter) = INTERPRETERS.into_iter().find(|program| host.on_path(program)) else {
        return Err(Error::NoInterpreter {
            manager: manager.program(),
            tried: INTERPRETERS.join(" or "),
        });
    };
    Ok(vec![interpreter.to_owned(), String::from("-m"), String::from("venv"), String::from(VENV)])
}

/// Whether the host may run the install as it stands.
enum Verdict {
    /// It may: the host's version agrees with the pin, or one of the two cannot be
    /// read as a version and this module does not guess.
    Runnable,
    /// It may not, and here is what the host answered.
    Refused {
        /// The host's major series, or `nothing` when the tool is absent.
        found: String,
    },
}

/// Compare the host's version of a tool to the pinned one, by major series.
///
/// The major series and no finer. A pin is written in every notation npm accepts —
/// `9.1.0`, `^9`, `>=9.1`, `pnpm@9.1.0` — and a comparison that tried to honour all of
/// them would refuse builds it should allow. A major version is the part every one of
/// those notations agrees on, and a difference in it is the one that silently changes
/// what an install writes.
fn satisfied(host: &dyn Host, program: &str, wanted: &str) -> Verdict {
    let Some(found) = host.version(program) else {
        return Verdict::Refused { found: String::from("nothing") };
    };
    let (Some(want), Some(have)) = (major(wanted), major(&found)) else {
        return Verdict::Runnable;
    };
    if want == have {
        return Verdict::Runnable;
    }
    Verdict::Refused { found: format!("{have}.x") }
}

/// The package managers Corepack knows how to fetch.
const fn corepack_owns(manager: PackageManager) -> bool {
    matches!(manager, PackageManager::Pnpm | PackageManager::Yarn | PackageManager::Npm)
}

/// The version a project pins one of its package managers to, as the manifest writes it.
///
/// The dedicated key first, then the toolchain table under the tool's own name and
/// under the `engines` name a `package.json` gives it. One of the three, whichever the
/// project's manifests supplied.
///
/// `package_manager_pin` comes from a `packageManager` field, so it belongs to the
/// Node manager in the list. A field that names its program belongs to that manager.
/// A field that names no program still belongs to the Node manager, not the primary.
fn pinned(recipe: &Recipe, manager: PackageManager) -> Option<String> {
    let program = manager.program();
    if let Some(pin) = recipe.package_manager_pin.as_ref() {
        let text = pin.as_str();
        let named = text.starts_with(&format!("{program}@"));
        let bare = !text.contains('@') && recipe.script_manager() == Some(manager);
        if named || bare {
            return Some(text.to_owned());
        }
    }
    for key in [program.to_owned(), format!("engines.{program}")] {
        if let Some((_, version)) = recipe.toolchain.iter().find(|(name, _)| name.as_str() == key) {
            return Some(version.as_str().to_owned());
        }
    }
    None
}

/// The pin as a message should name it, beside the tool's own name.
///
/// The manifest's own words, less the tool name a `packageManager` field repeats:
/// "needs pnpm pnpm@9.12.3" names the tool twice. A range keeps its operator, because
/// `^11` and `11.0.0` are different things to ask for and the person reading the
/// refusal is going to go and look at the manifest.
fn stated(pin: &str, program: &str) -> String {
    let text = pin.trim();
    text.strip_prefix(&format!("{program}@")).unwrap_or(text).to_owned()
}

/// The version out of a pin, for a tool that is asked to fetch it.
///
/// `pnpm@9.1.0` and `9.1.0` are the same pin written twice, and a range operator is
/// dropped: `corepack pnpm@^9 install` is not a thing Corepack accepts.
fn version_of(pin: &str, program: &str) -> String {
    let text = pin.trim();
    let text = text.strip_prefix(&format!("{program}@")).unwrap_or(text);
    text.trim_start_matches(['^', '~', '>', '=', '<', 'v', ' ']).trim().to_owned()
}

/// The major component of a version, when the text starts with one.
fn major(text: &str) -> Option<u32> {
    let digits: String = text
        .trim()
        .trim_start_matches(['^', '~', '>', '=', '<', 'v', ' '])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Whether a program is on the path, by looking for it there.
///
/// A read and not a spawn, so it stays here rather than in the module that starts
/// processes: asking a tool whether it exists by running it costs a process for every
/// build that has no pin to act on.
#[must_use]
pub fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|directory| runnable(&directory.join(program)))
}

/// Whether this path names a file this host can run.
#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .is_ok_and(|data| data.is_file() && data.permissions().mode() & 0o111 != 0)
}

/// Whether this path names a file this host can run.
#[cfg(not(unix))]
fn runnable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Host, Install, Site, in_the_home, installs, venv_of};
    use crate::model::recipe::Venv;
    use crate::model::recipe::{PackageManager, Recipe, ToolName, ToolVersion};
    use crate::{Error, Result};

    /// A host that answers whatever the test says, and starts nothing.
    struct Fake {
        /// What is on its path.
        tools: Vec<&'static str>,
        /// What its package manager answers to `--version`.
        version: Option<&'static str>,
    }

    impl Host for Fake {
        fn on_path(&self, program: &str) -> bool {
            self.tools.contains(&program)
        }

        fn version(&self, _program: &str) -> Option<String> {
            self.version.map(str::to_owned)
        }
    }

    fn bare_host() -> Fake {
        Fake { tools: Vec::new(), version: Some("10.4.1") }
    }

    /// A tree that holds no `poetry.toml`, which is every project here but the Poetry
    /// one. Reading a path that is not there answers Poetry's own default,
    /// [`Venv::Outside`], which is what a project that never configured it has.
    fn bare_tree() -> &'static Path {
        Path::new("/nodal-tests/no-such-tree")
    }

    /// The one install a recipe of a single manager produces.
    fn only(recipe: &Recipe, host: &dyn Host) -> Result<Install> {
        let mut resolved = installs(recipe, host, bare_tree())?;
        assert_eq!(resolved.len(), 1, "this helper is for a recipe of one manager");
        Ok(resolved.remove(0))
    }

    fn pinned_recipe(pin: &str) -> Recipe {
        Recipe {
            package_manager: vec![PackageManager::Pnpm],
            package_manager_pin: Some(ToolVersion::parse(pin.to_owned()).unwrap()),
            ..Recipe::default()
        }
    }

    #[test]
    fn a_project_with_no_pin_installs_as_it_always_did() {
        let recipe = Recipe { package_manager: vec![PackageManager::Pnpm], ..Recipe::default() };
        let resolved = only(&recipe, &bare_host()).unwrap();
        assert_eq!(resolved.argv, ["pnpm", "install"]);
        assert!(resolved.env.is_empty());
    }

    /// The blocker this shape exists for: `pip` with no environment installs into
    /// whichever interpreter the path holds, which writes into the host and not into the
    /// base. The build makes one first, and the install runs out of it.
    #[test]
    fn pip_is_given_an_environment_to_install_into() {
        let recipe = Recipe { package_manager: vec![PackageManager::Pip], ..Recipe::default() };
        let host = Fake { tools: vec!["python3"], version: None };
        let resolved = only(&recipe, &host).unwrap();

        assert_eq!(
            resolved.prepare,
            ["python3", "-m", "venv", ".venv"],
            "the environment is made before the install runs"
        );
        assert_eq!(resolved.argv, [".venv/bin/pip", "install", "-r", "requirements.txt"]);
    }

    /// A host with no `python3` is asked for `python`, and the two are tried in that
    /// order.
    #[test]
    fn the_second_interpreter_is_tried_when_the_first_is_not_there() {
        let recipe = Recipe { package_manager: vec![PackageManager::Pip], ..Recipe::default() };
        let host = Fake { tools: vec!["python"], version: None };
        assert_eq!(only(&recipe, &host).unwrap().prepare, ["python", "-m", "venv", ".venv"]);
    }

    /// Refused before the clone, for the reason a pin is: the alternative is a build
    /// that writes into the host.
    #[test]
    fn a_host_with_no_interpreter_is_refused_rather_than_left_to_the_hosts_pip() {
        let recipe = Recipe { package_manager: vec![PackageManager::Pip], ..Recipe::default() };
        let refused = only(&recipe, &Fake { tools: Vec::new(), version: None }).unwrap_err();
        let told = refused.to_string();
        assert!(matches!(refused, Error::NoInterpreter { .. }), "the wrong error: {told}");
        assert!(told.contains("python3 or python"), "{told}");
        assert!(told.contains("virtual environment"), "{told}");
    }

    /// A recipe of one manager, with the project excluding these paths from every home.
    fn excluding(manager: PackageManager, paths: &[&str]) -> Recipe {
        let mut recipe = Recipe { package_manager: vec![manager], ..Recipe::default() };
        recipe.base.exclude = paths.iter().map(PathBuf::from).collect();
        recipe
    }

    /// The measured case. The base installed `node_modules`, every home was cloned
    /// without it, and the minutes bought nothing.
    #[test]
    fn an_install_whose_output_no_home_receives_runs_in_the_home() {
        let recipe = excluding(PackageManager::Pnpm, &["node_modules"]);
        let host = Fake { tools: Vec::new(), version: None };

        let resolved = only(&recipe, &host).unwrap();
        assert_eq!(resolved.at, Site::Home { output: PathBuf::from("node_modules") });
        assert!(!resolved.at.in_base(), "the base would write what no home reads");

        let moved = in_the_home(&recipe, &host, bare_tree()).unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].argv, resolved.argv, "the same install, one directory later");
    }

    /// Poetry keeps its environment outside the tree unless the project moved it, so an
    /// exclusion of `.venv` names a directory Poetry was never going to write.
    ///
    /// The siting rule read the same table the warmth reading did, and that table
    /// answered `.venv` for every Python manager. The warmth reading gated Poetry a
    /// second time on `poetry.toml`; the siting rule had no such gate, so it moved
    /// Poetry's install into every home over a path nothing would ever put there. The
    /// fact is now in the table, so one answer serves both.
    #[test]
    fn poetry_that_keeps_its_environment_outside_the_tree_is_never_moved() {
        let recipe = excluding(PackageManager::Poetry, &[".venv"]);
        let host = Fake { tools: Vec::new(), version: None };
        let tree = tempfile::tempdir().unwrap();
        std::fs::write(tree.path().join("poetry.toml"), "[virtualenvs]\nin-project = false\n")
            .unwrap();

        assert_eq!(venv_of(tree.path()), Venv::Outside, "the project said false");
        let mut resolved = installs(&recipe, &host, tree.path()).unwrap();
        assert_eq!(resolved.remove(0).at, Site::Base, "nothing would read a .venv in a home");
        assert!(
            in_the_home(&recipe, &host, tree.path()).unwrap().is_empty(),
            "a create must not run an install for a directory poetry does not write"
        );
    }

    /// The same project, having asked Poetry for the environment beside the code. Now the
    /// exclusion names a directory Poetry really writes, so the install moves.
    #[test]
    fn poetry_that_keeps_its_environment_in_the_project_moves_like_any_other() {
        let recipe = excluding(PackageManager::Poetry, &[".venv"]);
        let host = Fake { tools: Vec::new(), version: None };
        let tree = tempfile::tempdir().unwrap();
        std::fs::write(tree.path().join("poetry.toml"), "[virtualenvs]\nin-project = true\n")
            .unwrap();

        assert_eq!(venv_of(tree.path()), Venv::InProject);
        let moved = in_the_home(&recipe, &host, tree.path()).unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].at, Site::Home { output: PathBuf::from(".venv") });
    }

    /// The ordinary project. Nothing moves, and the create asks the host about nothing.
    #[test]
    fn an_install_the_homes_receive_stays_in_the_base() {
        let recipe = Recipe { package_manager: vec![PackageManager::Pnpm], ..Recipe::default() };
        let host = Fake { tools: Vec::new(), version: None };
        assert_eq!(only(&recipe, &host).unwrap().at, Site::Base);
        assert!(in_the_home(&recipe, &host, bare_tree()).unwrap().is_empty());
    }

    /// Cargo writes into the Cargo home, which no exclusion list reaches, so an
    /// exclusion of `target` moves nothing.
    #[test]
    fn a_manager_that_installs_outside_the_tree_is_never_moved() {
        let recipe = excluding(PackageManager::Cargo, &["target", "node_modules"]);
        let host = Fake { tools: Vec::new(), version: None };
        assert_eq!(only(&recipe, &host).unwrap().at, Site::Base);
        assert!(in_the_home(&recipe, &host, bare_tree()).unwrap().is_empty());
    }

    /// One half of a repository moves and the other does not. A base of three ecosystems
    /// that excluded one of them still installs the other two.
    #[test]
    fn only_the_excluded_half_of_a_repository_moves() {
        let mut recipe = excluding(PackageManager::Pnpm, &[".venv"]);
        recipe.package_manager.push(PackageManager::Uv);
        let host = Fake { tools: Vec::new(), version: None };

        let resolved = installs(&recipe, &host, bare_tree()).unwrap();
        let node = resolved.iter().find(|one| one.manager == PackageManager::Pnpm).unwrap();
        let python = resolved.iter().find(|one| one.manager == PackageManager::Uv).unwrap();
        assert_eq!(node.at, Site::Base);
        assert_eq!(python.at, Site::Home { output: PathBuf::from(".venv") });

        let moved = in_the_home(&recipe, &host, bare_tree()).unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].manager, PackageManager::Uv);
    }

    /// A moved install keeps the pin it was resolved with. The route through Corepack is
    /// the one a second siting rule would have got wrong.
    #[test]
    fn a_moved_install_still_runs_at_the_version_the_project_pinned() {
        let mut recipe = excluding(PackageManager::Pnpm, &["node_modules"]);
        recipe.package_manager_pin = Some(ToolVersion::parse("pnpm@9.1.0").unwrap());
        let host = Fake { tools: vec!["corepack"], version: None };

        let moved = in_the_home(&recipe, &host, bare_tree()).unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].at, Site::Home { output: PathBuf::from("node_modules") });
        assert_eq!(moved[0].argv[..2], [String::from("corepack"), String::from("pnpm@9.1.0")]);
    }

    /// Every manager that makes its own environment, or needs none, is unchanged.
    #[test]
    fn a_manager_that_makes_its_own_environment_is_given_none() {
        for manager in [PackageManager::Uv, PackageManager::Poetry, PackageManager::Cargo] {
            let recipe = Recipe { package_manager: vec![manager], ..Recipe::default() };
            let resolved = only(&recipe, &Fake { tools: Vec::new(), version: None }).unwrap();
            assert!(resolved.prepare.is_empty(), "{manager:?}: {resolved:?}");
        }
    }

    #[test]
    fn corepack_runs_the_install_at_the_pinned_version() {
        let host = Fake { tools: vec!["corepack", "mise"], version: Some("10.4.1") };
        let resolved = only(&pinned_recipe("11.7.0"), &host).unwrap();
        assert_eq!(resolved.argv, ["corepack", "pnpm@11.7.0", "install"]);
        assert_eq!(resolved.env, [(String::from("COREPACK_ENABLE_DOWNLOAD_PROMPT"), "0".into())]);
    }

    #[test]
    fn mise_runs_the_install_when_there_is_no_corepack() {
        let host = Fake { tools: vec!["mise"], version: Some("10.4.1") };
        let resolved = only(&pinned_recipe("11.7.0"), &host).unwrap();
        assert_eq!(resolved.argv, ["mise", "exec", "pnpm@11.7.0", "--", "pnpm", "install"]);
    }

    #[test]
    fn a_host_of_the_pinned_major_series_installs_directly() {
        let host = Fake { tools: Vec::new(), version: Some("11.9.2") };
        let resolved = only(&pinned_recipe("11.7.0"), &host).unwrap();
        assert_eq!(resolved.argv, ["pnpm", "install"]);
    }

    #[test]
    fn a_host_of_another_major_series_is_refused_by_name_and_version() {
        let refused = only(&pinned_recipe("11.7.0"), &bare_host()).unwrap_err();
        let told = refused.to_string();
        assert!(matches!(refused, Error::ToolPin { .. }), "the wrong error: {told}");
        assert_eq!(told, "needs pnpm 11.7.0; host has 10.x; install it or run `corepack enable`");
    }

    #[test]
    fn a_host_without_the_tool_at_all_is_refused_too() {
        let host = Fake { tools: Vec::new(), version: None };
        let refused = only(&pinned_recipe("11.7.0"), &host).unwrap_err();
        assert!(refused.to_string().contains("host has nothing"), "{refused}");
    }

    #[test]
    fn a_pin_written_as_the_manifest_writes_it_is_read_the_same_way() {
        let host = Fake { tools: vec!["corepack"], version: Some("10.4.1") };
        let resolved = only(&pinned_recipe("pnpm@11.7.0"), &host).unwrap();
        assert_eq!(resolved.argv, ["corepack", "pnpm@11.7.0", "install"]);
    }

    #[test]
    fn a_range_is_compared_by_its_major_and_refused_in_the_words_it_was_written_in() {
        let refused = only(&pinned_recipe("^11.0.0"), &bare_host()).unwrap_err();
        assert!(refused.to_string().contains("needs pnpm ^11.0.0"), "{refused}");
    }

    #[test]
    fn a_refusal_names_the_tool_once_when_the_pin_names_it_too() {
        let refused = only(&pinned_recipe("pnpm@11.7.0"), &bare_host()).unwrap_err();
        assert_eq!(
            refused.to_string(),
            "needs pnpm 11.7.0; host has 10.x; install it or run `corepack enable`"
        );
    }

    #[test]
    fn a_toolchain_row_pins_the_package_manager_when_no_dedicated_key_does() {
        let mut recipe =
            Recipe { package_manager: vec![PackageManager::Pnpm], ..Recipe::default() };
        recipe.toolchain.insert(
            ToolName::parse(String::from("engines.pnpm")).unwrap(),
            ToolVersion::parse(String::from("11.7.0")).unwrap(),
        );
        let refused = only(&recipe, &bare_host()).unwrap_err();
        assert!(refused.to_string().contains("needs pnpm 11.7.0"), "{refused}");
    }

    #[test]
    fn a_version_neither_side_can_be_read_from_is_left_alone() {
        let host = Fake { tools: Vec::new(), version: Some("a nightly build") };
        let resolved = only(&pinned_recipe("11.7.0"), &host).unwrap();
        assert_eq!(resolved.argv, ["pnpm", "install"]);
    }

    /// A repository of three ecosystems installs three times, in the recipe's order.
    #[test]
    fn every_manager_the_recipe_names_gets_its_own_install() {
        let recipe = Recipe {
            package_manager: vec![PackageManager::Cargo, PackageManager::Pnpm, PackageManager::Uv],
            ..Recipe::default()
        };
        let resolved = installs(&recipe, &bare_host(), bare_tree()).unwrap();
        let argvs: Vec<Vec<String>> = resolved.iter().map(|one| one.argv.clone()).collect();
        assert_eq!(argvs, [["cargo", "fetch"], ["pnpm", "install"], ["uv", "sync"]]);
        assert_eq!(resolved[1].manager, PackageManager::Pnpm);
    }

    /// A `packageManager` field names its own program, so the pin it states belongs to
    /// the manager it names and to no other.
    #[test]
    fn a_pin_that_names_its_program_is_not_read_as_another_managers() {
        let recipe = Recipe {
            package_manager: vec![PackageManager::Cargo, PackageManager::Pnpm],
            package_manager_pin: Some(ToolVersion::parse(String::from("pnpm@11.7.0")).unwrap()),
            ..Recipe::default()
        };
        let refused = installs(&recipe, &bare_host(), bare_tree()).unwrap_err();
        assert!(refused.to_string().contains("needs pnpm 11.7.0"), "{refused}");

        let cargo_only = Recipe { package_manager: vec![PackageManager::Cargo], ..recipe.clone() };
        let resolved = installs(&cargo_only, &bare_host(), bare_tree()).unwrap();
        assert_eq!(resolved[0].argv, ["cargo", "fetch"], "pnpm's pin was read as cargo's");
    }

    #[test]
    fn corepack_is_not_asked_to_fetch_a_package_manager_it_does_not_own() {
        let host = Fake { tools: vec!["corepack"], version: Some("0.5.1") };
        let recipe = Recipe {
            package_manager: vec![PackageManager::Uv],
            package_manager_pin: Some(ToolVersion::parse(String::from("uv@1.2.3")).unwrap()),
            ..Recipe::default()
        };
        let refused = only(&recipe, &host).unwrap_err();
        assert!(refused.to_string().contains("needs uv 1.2.3"), "{refused}");
    }

    /// A `packageManager` field lives in `package.json`, so a version with no program
    /// belongs to the Node manager, not the primary. A Rust-led tree therefore installs
    /// cargo unpinned and pnpm at the pinned version.
    #[test]
    fn a_rust_led_tree_with_a_bare_node_pin_installs_cargo_unpinned() {
        let host = Fake { tools: vec!["corepack"], version: Some("10.4.1") };
        let pin = Some(ToolVersion::parse(String::from("11.7.0")).unwrap());
        let cargo = vec![String::from("cargo"), String::from("fetch")];
        let pnpm =
            vec![String::from("corepack"), String::from("pnpm@11.7.0"), String::from("install")];

        let rust_led = Recipe {
            package_manager: vec![PackageManager::Cargo, PackageManager::Pnpm],
            package_manager_pin: pin.clone(),
            ..Recipe::default()
        };
        let rust_argvs: Vec<Vec<String>> = installs(&rust_led, &host, bare_tree())
            .unwrap()
            .into_iter()
            .map(|one| one.argv)
            .collect();
        assert_eq!(rust_argvs, [cargo.clone(), pnpm.clone()]);

        let node_led = Recipe {
            package_manager: vec![PackageManager::Pnpm, PackageManager::Cargo],
            package_manager_pin: pin,
            ..Recipe::default()
        };
        let node_argvs: Vec<Vec<String>> = installs(&node_led, &host, bare_tree())
            .unwrap()
            .into_iter()
            .map(|one| one.argv)
            .collect();
        assert_eq!(node_argvs, [pnpm, cargo]);
    }

    /// A manager the pin does not belong to is installed, not refused, when the Node
    /// manager's pin is satisfied.
    #[test]
    fn a_manager_the_pin_does_not_belong_to_is_not_refused() {
        let recipe = Recipe {
            package_manager: vec![PackageManager::Pnpm, PackageManager::Cargo],
            package_manager_pin: Some(ToolVersion::parse(String::from("10.4.1")).unwrap()),
            ..Recipe::default()
        };
        let resolved = installs(&recipe, &bare_host(), bare_tree()).unwrap();
        let argvs: Vec<Vec<String>> = resolved.iter().map(|one| one.argv.clone()).collect();
        assert_eq!(argvs, [["pnpm", "install"], ["cargo", "fetch"]]);
    }
}
