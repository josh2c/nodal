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
//! # Which manager a pin belongs to
//!
//! A `packageManager` field that names its program belongs to the manager it names, and
//! [`pinned`] reads it that way. A field that names a version and no program belongs to
//! the primary manager, because the primary is the manager the repository is driven by.
//!
//! That second rule is now stated against a primary the build command decides
//! ([`crate::recipe::infer::package_manager::lead`]). So in a Rust-led repository — one
//! built by `cargo build`, with a `package.json` beside its `Cargo.toml` — a bare
//! `"packageManager": "9.12.3"` binds to Cargo, and a base build refuses the host's
//! `cargo` over a version the Node half asked for. The field is a `package.json` field,
//! so the manager whose manifest carries it is the one that meant it.
//!
//! It is stated rather than fixed here. The fix is to give the pin to the manager whose
//! manifest carries it, which is a change to what the recipe records rather than to how
//! this module reads it, and it is a follow-up of its own. A field that names its
//! program, which is what a manifest written by any of these tools carries, is
//! unaffected.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::recipe::{PackageManager, Recipe};
use crate::{Error, Result};

/// The variable that stops Corepack asking a person to confirm a download.
///
/// A base build has no terminal of its own: it runs under `nodal new`, under `nodal
/// base build` and under the resolver that finishes an interrupted one. Corepack
/// waiting for an answer nobody can give would hang the build rather than fail it.
const NO_DOWNLOAD_PROMPT: (&str, &str) = ("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0");

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

/// How a base build runs one package manager's install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Install {
    /// Whose install this is. The step it becomes is named after it, so a build of a
    /// repository of three ecosystems has three named steps rather than three called
    /// the same thing.
    pub manager: PackageManager,
    /// The program and its arguments.
    pub argv: Vec<String>,
    /// Variables the install needs, on top of the ones it inherits.
    pub env: Vec<(String, String)>,
}

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
pub fn installs(recipe: &Recipe, host: &dyn Host) -> Result<Vec<Install>> {
    recipe.package_manager.iter().map(|manager| install(recipe, *manager, host)).collect()
}

/// The install one manager runs, with its own pin acted on.
fn install(recipe: &Recipe, manager: PackageManager, host: &dyn Host) -> Result<Install> {
    let argv = super::build::install_argv(manager);
    let plain = |argv: Vec<String>| Install { manager, argv, env: Vec::new() };
    let (Some(pin), Some((program, rest))) = (pinned(recipe, manager), argv.split_first()) else {
        return Ok(plain(argv));
    };
    let wanted = version_of(&pin, program);

    if corepack_owns(manager) && host.on_path("corepack") {
        let mut through = vec![String::from("corepack"), format!("{program}@{wanted}")];
        through.extend_from_slice(rest);
        let (name, value) = NO_DOWNLOAD_PROMPT;
        let env = vec![(name.to_owned(), value.to_owned())];
        return Ok(Install { manager, argv: through, env });
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
/// `package_manager_pin` comes from a `packageManager` field, which names its own
/// program, so it belongs to the manager it names. A repository whose primary is Cargo
/// and whose `package.json` pins pnpm must not have that pin read as Cargo's. A pin
/// that names no program is the primary manager's, for the reason the module doc gives.
fn pinned(recipe: &Recipe, manager: PackageManager) -> Option<String> {
    let program = manager.program();
    if let Some(pin) = recipe.package_manager_pin.as_ref() {
        let text = pin.as_str();
        let named = text.starts_with(&format!("{program}@"));
        let bare = !text.contains('@') && recipe.package_manager.first() == Some(&manager);
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
    use super::{Host, Install, installs};
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

    /// The one install a recipe of a single manager produces.
    fn only(recipe: &Recipe, host: &dyn Host) -> Result<Install> {
        let mut resolved = installs(recipe, host)?;
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
        let resolved = installs(&recipe, &bare_host()).unwrap();
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
        let refused = installs(&recipe, &bare_host()).unwrap_err();
        assert!(refused.to_string().contains("needs pnpm 11.7.0"), "{refused}");

        let cargo_only = Recipe { package_manager: vec![PackageManager::Cargo], ..recipe.clone() };
        let resolved = installs(&cargo_only, &bare_host()).unwrap();
        assert_eq!(resolved[0].argv, ["cargo", "fetch"], "pnpm's pin was read as cargo's");
    }

    #[test]
    fn corepack_is_not_asked_to_fetch_a_package_manager_it_does_not_own() {
        let host = Fake { tools: vec!["corepack"], version: Some("0.5.1") };
        let recipe = Recipe {
            package_manager: vec![PackageManager::Uv],
            package_manager_pin: Some(ToolVersion::parse(String::from("1.2.3")).unwrap()),
            ..Recipe::default()
        };
        let refused = only(&recipe, &host).unwrap_err();
        assert!(refused.to_string().contains("needs uv 1.2.3"), "{refused}");
    }

    /// A pin that names no program says nothing about the managers behind the primary.
    /// Read as every manager's, this recipe would refuse the whole build because the
    /// host's Cargo is not version 11.
    #[test]
    fn a_pin_that_names_no_program_binds_to_the_primary_manager_only() {
        let recipe = Recipe {
            package_manager: vec![PackageManager::Pnpm, PackageManager::Cargo],
            package_manager_pin: Some(ToolVersion::parse(String::from("11.7.0")).unwrap()),
            ..Recipe::default()
        };
        let refused = installs(&recipe, &bare_host()).unwrap_err();
        assert!(refused.to_string().contains("needs pnpm 11.7.0"), "the primary: {refused}");

        let cargo_first = Recipe {
            package_manager: vec![PackageManager::Cargo, PackageManager::Pnpm],
            ..recipe.clone()
        };
        let resolved = installs(&cargo_first, &bare_host()).unwrap_err();
        assert!(resolved.to_string().contains("needs cargo 11.7.0"), "{resolved}");
    }

    /// The same recipe with the pin removed installs both managers and refuses neither.
    #[test]
    fn a_manager_behind_the_primary_is_not_refused_over_the_primarys_bare_pin() {
        let recipe = Recipe {
            package_manager: vec![PackageManager::Pnpm, PackageManager::Cargo],
            package_manager_pin: Some(ToolVersion::parse(String::from("10.4.1")).unwrap()),
            ..Recipe::default()
        };
        // The host answers 10.4.1, so the primary's pin is satisfied and cargo, which
        // the pin says nothing about, is installed rather than refused.
        let resolved = installs(&recipe, &bare_host()).unwrap();
        let argvs: Vec<Vec<String>> = resolved.iter().map(|one| one.argv.clone()).collect();
        assert_eq!(argvs, [["pnpm", "install"], ["cargo", "fetch"]]);
    }
}
