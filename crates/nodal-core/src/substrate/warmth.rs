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
//! project directory. That is [`State::Unknown`] with the reason, never a guess.
//!
//! `pip` answers, because a base build makes the environment it installs into and puts
//! it in the tree ([`crate::substrate::pin::Install::prepare`]). A tree with no `.venv`
//! is one `pip` has not run in, which is cold.
//!
//! # An install output the project excluded
//!
//! `base.exclude` names paths no unit home receives. A project that names an install
//! output there gets a base that does not run that install at all ([`super::pin::Site`]),
//! because nothing would ever read what it wrote. The base's line says that, rather than
//! reporting a directory missing that the base was never going to have.
//!
//! The home's line does not change. A home runs that install itself, so the home's tree
//! answers for it exactly as any other tree does.
//!
//! # An ecosystem the recipe has no manager for
//!
//! A repository may carry a half that the recipe names no manager for: a
//! `pyproject.toml` with no lockfile, a `package.json` with no lockfile. Nothing
//! installs that half, so nothing in the tree will ever prove it warm, and a report that
//! read only the managers the recipe names said nothing about it at all. The base then
//! handed over a tree that was cold in a way no line named, and a build that needed that
//! half failed with the tool's own error.
//!
//! So the manifests are read as well as the managers ([`Ecosystem::manifests`]), and an
//! ecosystem a repository has and a recipe has no manager for is reported as what it is.
//!
//! The question is put only where the recipe names at least one manager. A recipe that
//! names none already answers it, in one line about the whole project, and asking again
//! per ecosystem would put a second line on every project that has not been set up
//! yet.

use std::path::Path;

use crate::model::readiness::{Readiness, State};
use crate::model::recipe::{Ecosystem, PackageManager, Recipe, Venv};
use crate::substrate::{build_output, pin};
use crate::workspace::Excludes;

/// Cargo's build output directory, which every profile writes a subdirectory of.
const TARGET: &str = "target";

/// Which copy a reading is about, which decides one answer and no other.
///
/// A base and a home hold the same tree and do not have the same job. An install whose
/// output the project excluded is the home's to run, so the base is not cold for lacking
/// it and the home is. Nothing else in this module reads this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tree {
    /// The tree homes are cloned from.
    Base,
    /// A unit home.
    Home,
}

/// Whether `tree` holds what a build of `recipe` was to produce.
#[must_use]
pub fn of(recipe: &Recipe, tree: &Path, kind: Tree) -> Readiness {
    Readiness { dependencies: dependencies(recipe, tree, kind), build: build(recipe, tree) }
}

/// Whether every manager the recipe names has left its dependencies in the tree, and
/// whether the tree holds an ecosystem no manager of it covers.
fn dependencies(recipe: &Recipe, tree: &Path, kind: Tree) -> State {
    if recipe.package_manager.is_empty() {
        return State::Unknown { why: String::from("the project names no package manager") };
    }
    let excludes = Excludes::with_recipe(&recipe.base.exclude);
    let venv = pin::venv_of(tree);
    recipe
        .package_manager
        .iter()
        .map(|manager| match sited(*manager, &excludes, venv, kind) {
            Some(elsewhere) => elsewhere,
            None => installed(*manager, tree, venv),
        })
        .chain(unmanaged(recipe, tree).into_iter().map(no_manager))
        .fold(State::Ready, State::worse)
}

/// What a base says about an install it does not run, and nothing for every other case.
///
/// Only a base answers here. A home runs that install itself, so its own tree is the
/// evidence and [`installed`] is what reads it. The state is unknown rather than cold
/// because the base is not missing anything: a person sent to rebuild it would wait for
/// an install and get the same tree back.
fn sited(manager: PackageManager, excludes: &Excludes, venv: Venv, kind: Tree) -> Option<State> {
    if kind != Tree::Base {
        return None;
    }
    let output = pin::where_it_runs(manager, excludes, venv).excluded()?.clone();
    Some(State::Unknown {
        why: format!(
            "{} is in base.exclude, so no home receives it; `{}` installs in each home \
             instead of here",
            output.display(),
            manager.program()
        ),
    })
}

/// Every ecosystem the tree holds a manifest for that no manager of the recipe covers.
///
/// The manifest is read from the tree and not from the recipe, because the question is
/// what this working copy has in it.
fn unmanaged(recipe: &Recipe, tree: &Path) -> Vec<(Ecosystem, &'static str)> {
    Ecosystem::ALL
        .iter()
        .filter(|ecosystem| {
            !recipe.package_manager.iter().any(|manager| manager.ecosystem() == **ecosystem)
        })
        .filter_map(|ecosystem| {
            let manifest = ecosystem.manifests().iter().find(|name| tree.join(name).exists())?;
            Some((*ecosystem, *manifest))
        })
        .collect()
}

/// What a report says about an ecosystem nothing would install.
///
/// Cold and not unknown: the tree is not ready, and unlike Cargo's cache there is no
/// reading anywhere that could say otherwise. What a person does about it is add the
/// manager to `package_manager`, so the line names the key rather than a directory.
fn no_manager((ecosystem, manifest): (Ecosystem, &'static str)) -> State {
    State::Cold {
        why: format!(
            "{manifest} is there and no manager for this ecosystem is known; \
             name a {} manager in package_manager",
            ecosystem.name()
        ),
    }
}

/// Whether one manager has left its dependencies in the tree.
///
/// The directory comes from the one table ([`PackageManager::install_output`]) rather
/// than from a second list of names here, and `venv` is the project's answer about
/// Poetry that the table needs. A manager the table gives no directory for writes
/// nothing a tree can be asked about, and [`outside`] says which one it is.
fn installed(manager: PackageManager, tree: &Path, venv: Venv) -> State {
    match manager.install_output(venv) {
        Some(output) => present(tree, output, manager.program()),
        None => outside(manager),
    }
}

/// Why a manager's install leaves nothing in this tree to read.
///
/// Both are [`State::Unknown`] and not cold: the tree is missing nothing, because
/// nothing was ever going to put a directory there. Reporting cold would send a person
/// to rebuild something that is already where its tool keeps it.
fn outside(manager: PackageManager) -> State {
    State::Unknown {
        why: String::from(match manager {
            PackageManager::Poetry => {
                "poetry keeps its environment outside the tree unless \
                 virtualenvs.in-project is set"
            }
            _ => "cargo keeps its download cache outside the tree",
        }),
    }
}

/// Whether the build command's own output directory is in the tree.
///
/// Cargo names it by profile, and only for the two profiles whose directory is part of
/// the command: `cargo build` writes `target/debug` and `--release` writes
/// `target/release`. A `--profile` names a directory of its own, which this does not
/// read from the manifest, so that is unknown rather than reported cold at a path the
/// build never wrote. Every other build writes where the project tells its tool to,
/// and that is read from the command, the script it runs and the task cache
/// ([`super::build_output`]); a build that names none has no directory to look for.
fn build(recipe: &Recipe, tree: &Path) -> State {
    let Some(command) = recipe.commands.build.as_ref() else {
        return State::Unknown { why: String::from("the project states no build command") };
    };
    let words: Vec<&str> = command.as_str().split_whitespace().collect();
    let Some(program) = words.first() else {
        return State::Unknown { why: String::from("the build command is empty") };
    };
    if *program != PackageManager::Cargo.program() {
        return match build_output::named(recipe, tree, &words) {
            Some(directory) => present(tree, &directory, program),
            None => State::Unknown { why: String::from("the build names no output directory") },
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

    use super::Tree;
    use crate::model::readiness::{Readiness, State};
    use crate::model::recipe::{CommandLine, PackageManager, Recipe};

    /// A reading of a home, which is what every test here asks unless it says otherwise.
    fn of(recipe: &Recipe, tree: &Path) -> Readiness {
        super::of(recipe, tree, Tree::Home)
    }

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

    /// The base's line says where the install went, rather than reporting a directory
    /// missing that the base was never going to write.
    #[test]
    fn a_base_says_that_an_excluded_install_output_is_installed_in_each_home() {
        let mut spec = recipe(&[PackageManager::Pnpm], None);
        spec.base.exclude = vec![std::path::PathBuf::from("node_modules")];
        let cold = tree(&[]);

        let base = super::of(&spec, cold.path(), Tree::Base).dependencies;
        assert!(matches!(base, State::Unknown { .. }), "the base lacks nothing: {base:?}");
        let why = base.why().unwrap();
        assert!(why.contains("base.exclude"), "{why}");
        assert!(why.contains("installs in each home"), "{why}");
    }

    /// The home's line does not change. The home runs the install, so the home's own tree
    /// is the evidence.
    #[test]
    fn a_home_answers_for_an_excluded_install_output_out_of_its_own_tree() {
        let mut spec = recipe(&[PackageManager::Pnpm], None);
        spec.base.exclude = vec![std::path::PathBuf::from("node_modules")];

        let cold = of(&spec, tree(&[]).path()).dependencies;
        assert!(matches!(cold, State::Cold { .. }), "{cold:?}");
        assert!(cold.why().unwrap().contains("node_modules"));

        assert_eq!(of(&spec, tree(&["node_modules"]).path()).dependencies, State::Ready);
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

    /// A Node build is read at the directory the project says it writes, and a project
    /// whose build names none is not read at a guessed path.
    #[test]
    fn a_node_build_is_checked_at_the_directory_its_script_names() {
        let root = tree(&[]);
        std::fs::write(root.path().join("package.json"), r#"{"scripts":{"build":"vite build"}}"#)
            .unwrap();
        let npm = recipe(&[PackageManager::Npm], Some("npm run build"));
        let cold = of(&npm, root.path()).build;
        assert_eq!(cold.why(), Some("dist is not there; `npm` has not run here"));

        std::fs::create_dir_all(root.path().join("dist")).unwrap();
        assert_eq!(of(&npm, root.path()).build, State::Ready);
    }

    #[test]
    fn a_build_with_no_output_this_can_name_is_unknown_rather_than_guessed() {
        let root = tree(&[]);
        let stated = of(&recipe(&[], Some("pnpm run build")), root.path()).build;
        assert_eq!(
            stated,
            State::Unknown { why: String::from("the build names no output directory") }
        );

        let none = of(&recipe(&[], None), root.path()).build;
        assert_eq!(none.why(), Some("the project states no build command"));
    }

    #[test]
    fn a_project_with_no_package_manager_is_not_reported_as_installed() {
        let state = of(&Recipe::default(), Path::new("/nonexistent")).dependencies;
        assert!(matches!(state, State::Unknown { .. }), "{state:?}");
    }

    /// A project that names no manager at all keeps its one line about the project, and
    /// does not get a second one per ecosystem.
    #[test]
    fn a_project_that_names_no_manager_is_not_asked_about_each_ecosystem() {
        let root = tree(&[]);
        std::fs::write(root.path().join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
        let state = of(&Recipe::default(), root.path()).dependencies;
        assert_eq!(state.why(), Some("the project names no package manager"));
    }

    /// A `requirements.txt` half once had no manager at all, so nothing installed it
    /// and nothing said so. `pip` installs it into an environment the base build makes
    /// in the tree, so the tree can answer for it.
    #[test]
    fn a_pip_tree_is_ready_when_it_carries_the_environment_and_cold_when_it_does_not() {
        let root = tree(&[]);
        std::fs::write(root.path().join("requirements.txt"), "flask\n").unwrap();
        let recipe = recipe(&[PackageManager::Pip], None);

        let state = of(&recipe, root.path()).dependencies;
        assert!(matches!(state, State::Cold { .. }), "{state:?}");
        assert!(state.why().unwrap().contains(".venv"), "{state:?}");

        std::fs::create_dir_all(root.path().join(".venv")).unwrap();
        assert_eq!(of(&recipe, root.path()).dependencies, State::Ready);
    }

    /// A half of the repository that no manager covers is named, rather than left out of
    /// the answer.
    #[test]
    fn an_ecosystem_with_a_manifest_and_no_manager_is_named() {
        let root = tree(&["node_modules"]);
        std::fs::write(root.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();

        let state = of(&recipe(&[PackageManager::Pnpm], None), root.path()).dependencies;
        assert!(matches!(state, State::Cold { .. }), "the node half is warm: {state:?}");
        let why = state.why().unwrap();
        assert!(why.contains("pyproject.toml"), "{why}");
        assert!(why.contains("no manager for this ecosystem"), "{why}");
        assert!(why.contains("python"), "{why}");
    }

    /// A manifest an ecosystem's own manager covers is not reported twice.
    #[test]
    fn an_ecosystem_a_manager_covers_is_read_once_and_by_that_manager() {
        let root = tree(&[".venv"]);
        std::fs::write(root.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        assert_eq!(
            of(&recipe(&[PackageManager::Uv], None), root.path()).dependencies,
            State::Ready
        );
    }

    /// A repository that names no manager and has no manifest either keeps the answer it
    /// always had.
    #[test]
    fn a_tree_with_no_manifest_and_no_manager_still_says_the_project_names_none() {
        let state = of(&Recipe::default(), tree(&[]).path()).dependencies;
        assert_eq!(state.why(), Some("the project names no package manager"));
    }
}
