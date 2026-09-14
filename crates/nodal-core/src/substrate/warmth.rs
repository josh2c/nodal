//! Whether a tree holds what a base build was to put in it.
//!
//! One question, asked of files: for each package manager the recipe names, and for the
//! build command it names, is the output there. Nothing here starts a process, reads a
//! lockfile or asks a tool what it thinks. A mark saying a build finished is not
//! evidence, because a build that was undone leaves the directory it wrote in.
//!
//! Every ecosystem answers differently, and one of them cannot answer at all. Cargo's
//! install writes into the Cargo home, which is outside the tree and shared by every
//! base on the host, so no file under a Cargo base says whether its dependencies are
//! fetched. Poetry is the same unless the project asked for the environment in the
//! project directory. Those are [`State::Unknown`] with the reason, never a guess.

use std::path::Path;

use crate::model::readiness::{Readiness, State};
use crate::model::recipe::{PackageManager, Recipe};

/// Where a Node install puts what it installed.
const NODE_MODULES: &str = "node_modules";

/// Where a Python install puts its environment, when it puts it in the project.
const VENV: &str = ".venv";

/// Poetry's own configuration file, and the key that moves its environment into the
/// project directory.
const POETRY_CONFIG: &str = "poetry.toml";

/// Cargo's build output directory, which every profile writes a subdirectory of.
const TARGET: &str = "target";

/// Whether `tree` holds what a build of `recipe` was to produce.
#[must_use]
pub fn of(recipe: &Recipe, tree: &Path) -> Readiness {
    Readiness { dependencies: dependencies(recipe, tree), build: build(recipe, tree) }
}

/// Whether every manager the recipe names has left its dependencies in the tree.
fn dependencies(recipe: &Recipe, tree: &Path) -> State {
    if recipe.package_manager.is_empty() {
        return State::Unknown { why: String::from("the project names no package manager") };
    }
    recipe
        .package_manager
        .iter()
        .map(|manager| installed(*manager, tree))
        .fold(State::Ready, State::worse)
}

/// Whether one manager has left its dependencies in the tree.
fn installed(manager: PackageManager, tree: &Path) -> State {
    let program = manager.program();
    match manager {
        PackageManager::Pnpm | PackageManager::Yarn | PackageManager::Npm | PackageManager::Bun => {
            present(tree, NODE_MODULES, program)
        }
        PackageManager::Uv => present(tree, VENV, program),
        PackageManager::Poetry if in_project(tree) => present(tree, VENV, program),
        PackageManager::Poetry => State::Unknown {
            why: String::from(
                "poetry keeps its environment outside the tree unless virtualenvs.in-project is set",
            ),
        },
        PackageManager::Cargo => {
            State::Unknown { why: String::from("cargo keeps its download cache outside the tree") }
        }
    }
}

/// Whether the project asked Poetry to keep its environment beside the code.
fn in_project(tree: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(tree.join(POETRY_CONFIG)) else { return false };
    toml::from_str::<toml::Value>(&text)
        .ok()
        .and_then(|config| config.get("virtualenvs")?.get("in-project")?.as_bool())
        .unwrap_or(false)
}

/// Whether the build command's own output directory is in the tree.
///
/// Only Cargo names one that can be checked, and only for the two profiles whose
/// directory is part of the command: `cargo build` writes `target/debug` and
/// `--release` writes `target/release`. A `--profile` names a directory of its own,
/// which this does not read from the manifest, so that is unknown rather than reported
/// cold at a path the build never wrote. A Node build writes wherever the project's own
/// configuration sends it, so there is no directory to look for either.
fn build(recipe: &Recipe, tree: &Path) -> State {
    let Some(command) = recipe.commands.build.as_ref() else {
        return State::Unknown { why: String::from("the project states no build command") };
    };
    let words: Vec<&str> = command.as_str().split_whitespace().collect();
    let Some(program) = words.first() else {
        return State::Unknown { why: String::from("the build command is empty") };
    };
    if *program != PackageManager::Cargo.program() {
        return State::Unknown {
            why: format!("a `{program}` build names no output directory this can check"),
        };
    }
    if words.iter().any(|word| word.starts_with("--profile")) {
        return State::Unknown {
            why: String::from("the build names a profile whose output directory is not read here"),
        };
    }
    let profile = if words.contains(&"--release") { "release" } else { "debug" };
    present(tree, &format!("{TARGET}/{profile}"), program)
}

/// Whether `relative` is a directory of `tree`, named in the words a report uses.
fn present(tree: &Path, relative: &str, program: &str) -> State {
    if tree.join(relative).is_dir() {
        return State::Ready;
    }
    State::Cold { why: format!("{relative} is not there; `{program}` has not run here") }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;

    use super::of;
    use crate::model::readiness::State;
    use crate::model::recipe::{CommandLine, PackageManager, Recipe};

    fn recipe(managers: &[PackageManager], build: Option<&str>) -> Recipe {
        let mut recipe = Recipe { package_manager: managers.to_vec(), ..Recipe::default() };
        recipe.commands.build = build.map(|line| CommandLine::parse(line).unwrap());
        recipe
    }

    fn tree(directories: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for relative in directories {
            std::fs::create_dir_all(root.path().join(relative)).unwrap();
        }
        root
    }

    #[test]
    fn a_node_tree_is_ready_when_its_dependencies_are_there_and_cold_when_they_are_not() {
        let warm = tree(&["node_modules"]);
        let recipe = recipe(&[PackageManager::Pnpm], None);
        assert_eq!(of(&recipe, warm.path()).dependencies, State::Ready);

        let cold = tree(&[]);
        let state = of(&recipe, cold.path()).dependencies;
        assert!(matches!(state, State::Cold { .. }), "{state:?}");
        assert!(state.why().unwrap().contains("node_modules"));
    }

    /// The one a file cannot answer. Reporting it cold would send a person to rebuild
    /// what is already fetched.
    #[test]
    fn a_cargo_tree_cannot_prove_its_dependencies_either_way() {
        let state = of(&recipe(&[PackageManager::Cargo], None), tree(&[]).path()).dependencies;
        assert!(matches!(state, State::Unknown { .. }), "{state:?}");
        assert!(state.why().unwrap().contains("outside the tree"));
    }

    /// A repository of three ecosystems is as ready as its least ready manager.
    #[test]
    fn a_polyglot_tree_reports_the_manager_that_has_not_run() {
        let managers = [PackageManager::Pnpm, PackageManager::Cargo, PackageManager::Uv];
        let half = tree(&["node_modules"]);
        let state = of(&recipe(&managers, None), half.path()).dependencies;
        assert!(matches!(state, State::Cold { .. }), "uv left no environment: {state:?}");
        assert!(state.why().unwrap().contains(".venv"));

        let whole = tree(&["node_modules", ".venv"]);
        let state = of(&recipe(&managers, None), whole.path()).dependencies;
        assert!(matches!(state, State::Unknown { .. }), "cargo cannot answer: {state:?}");
    }

    #[test]
    fn poetry_answers_only_when_the_project_keeps_its_environment_in_the_tree() {
        let root = tree(&[]);
        let recipe = recipe(&[PackageManager::Poetry], None);
        assert!(matches!(of(&recipe, root.path()).dependencies, State::Unknown { .. }));

        std::fs::write(root.path().join("poetry.toml"), "[virtualenvs]\nin-project = true\n")
            .unwrap();
        let state = of(&recipe, root.path()).dependencies;
        assert!(matches!(state, State::Cold { .. }), "{state:?}");

        std::fs::create_dir_all(root.path().join(".venv")).unwrap();
        assert_eq!(of(&recipe, root.path()).dependencies, State::Ready);
    }

    #[test]
    fn a_cargo_build_is_checked_at_the_profile_the_command_asked_for() {
        let debug = tree(&["target/debug"]);
        assert_eq!(of(&recipe(&[], Some("cargo build")), debug.path()).build, State::Ready);

        let state = of(&recipe(&[], Some("cargo build --release")), debug.path()).build;
        assert!(matches!(state, State::Cold { .. }), "{state:?}");
        assert!(state.why().unwrap().contains("target/release"));

        let release = tree(&["target/release"]);
        let ready = of(&recipe(&[], Some("cargo build --release")), release.path()).build;
        assert_eq!(ready, State::Ready);
    }

    /// A named profile writes a directory of its own, which nothing here reads, so it
    /// is unknown rather than reported cold at `target/debug`.
    #[test]
    fn a_build_under_a_named_profile_is_unknown_rather_than_cold_at_the_wrong_path() {
        let root = tree(&["target/debug"]);
        for line in ["cargo build --profile fast", "cargo build --profile=fast"] {
            let state = of(&recipe(&[], Some(line)), root.path()).build;
            assert!(matches!(state, State::Unknown { .. }), "{line}: {state:?}");
        }
    }

    #[test]
    fn a_build_with_no_output_this_can_name_is_unknown_rather_than_guessed() {
        let root = tree(&[]);
        let stated = of(&recipe(&[], Some("pnpm run build")), root.path()).build;
        assert!(matches!(stated, State::Unknown { .. }), "{stated:?}");

        let none = of(&recipe(&[], None), root.path()).build;
        assert_eq!(none.why(), Some("the project states no build command"));
    }

    #[test]
    fn a_project_with_no_package_manager_is_not_reported_as_installed() {
        let state = of(&Recipe::default(), Path::new("/nonexistent")).dependencies;
        assert!(matches!(state, State::Unknown { .. }), "{state:?}");
    }
}
