//! Which package managers the project installs with, and the version it pins.
//!
//! The file a manager installs from is the evidence: it is committed, it is the file CI
//! installs from, and it names exactly one manager. A repository of one ecosystem
//! carries one; a repository of a Rust binary, a Node CLI and a Python tool carries
//! three, and every one of them has to be installed or the base is warm for a third of
//! the tree.
//!
//! So every such file present is proposed, in the order below. The order is the order of
//! specificity inside an ecosystem, so a repository that carries both a `pnpm-lock.yaml`
//! and a `package-lock.json` still installs with the one its own tooling would use. A
//! `requirements.txt` is last of the Python three, because it is the file a repository
//! that pins nothing still has.
//!
//! **Which of them is the primary is a second question, and the recipe answers it.** The
//! primary is the manager a bare script name resolves against, so it is the manager that
//! leads the repository, and what says which one that is, is the build command the recipe
//! ends up with: a repository built by `cargo build` is led by Cargo however many Node
//! files it carries. A repository with no build command is led by the manager its
//! `packageManager` field names, which names its program whether or not it also pins a
//! version. A repository that states neither keeps the table's own order.
//!
//! [`lead`] is what applies that, and it runs after every source rather than inside this
//! one, because the build command is what the sources together decide.

use crate::model::recipe::{Ecosystem, PackageManager, Recipe, ToolVersion};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Every package manager, most specific first inside its ecosystem.
///
/// The file each one is proposed on is [`PackageManager::lockfiles`]: the same table a
/// base build reads to hold the install to, so the file that names a manager is the
/// file its install never changes.
const INSTALLS_FROM: [PackageManager; 8] = [
    PackageManager::Pnpm,
    PackageManager::Yarn,
    PackageManager::Npm,
    PackageManager::Bun,
    PackageManager::Cargo,
    PackageManager::Uv,
    PackageManager::Poetry,
    PackageManager::Pip,
];

/// The `package.json` field that names the manager the repository is driven by.
const PIN: &str = "packageManager";

/// Propose `package_manager` and `package_manager_pin`.
///
/// One manager per ecosystem. Two such files of one ecosystem are a repository mid-way
/// through changing manager, and installing with both would write two dependency trees
/// over each other, so the more specific one wins and the other is not proposed. A
/// `uv.lock` beside a `requirements.txt` is that case: uv is what the repository pins
/// with, and the requirements file is what it exports.
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    let mut ecosystems: Vec<Ecosystem> = Vec::new();
    for manager in INSTALLS_FROM {
        let carried = manager.lockfiles().iter().any(|installs_from| project.exists(installs_from));
        if !carried || ecosystems.contains(&manager.ecosystem()) {
            continue;
        }
        ecosystems.push(manager.ecosystem());
        proposal.recipe.package_manager.push(manager);
    }
    if !proposal.recipe.package_manager.is_empty() {
        proposal = proposal.sure("package_manager", Confidence::High);
    }
    proposal.recipe.package_manager_pin = project
        .package_json()
        .get(PIN)
        .and_then(serde_json::Value::as_str)
        .and_then(|pin| ToolVersion::parse(pin).ok());
    proposal
}

/// Put the manager that leads the repository at the front of its list.
///
/// Run once, after every source, because what decides the primary is the build command
/// the sources together arrived at. A repository built by `cargo build` is led by Cargo
/// and one built by `pnpm run build` is led by pnpm, whatever else either of them
/// carries: the manager that runs the build is the manager a bare script name belongs
/// to, and reading the recipe's own command is one reading rather than a second one kept
/// in step with the first.
///
/// With no build command the `packageManager` field names the primary. That field names
/// its program whether or not it also pins a version, so `pnpm` names pnpm exactly as
/// `pnpm@9.12.3` does.
///
/// Nothing moves where the program named is not one of the managers the project carries
/// a lockfile for, and nothing moves where the recipe states neither. The managers
/// behind the primary keep the order the table gave them.
pub fn lead(recipe: &mut Recipe) {
    let program = match (&recipe.commands.build, &recipe.package_manager_pin) {
        (Some(build), _) => build.as_str().split_whitespace().next(),
        (None, Some(pin)) => pin.as_str().split('@').next(),
        (None, None) => None,
    };
    let Some(at) = program
        .and_then(|program| recipe.package_manager.iter().position(|m| m.program() == program))
    else {
        return;
    };
    recipe.package_manager[..=at].rotate_right(1);
}

/// The command that runs a script a `package.json` declares.
///
/// The manager that reads that file, not the primary: a repository whose primary is
/// Cargo still runs its `package.json` scripts with its Node manager.
#[must_use]
pub fn run_script(recipe: &Recipe, script: &str) -> String {
    let program = recipe.script_manager().unwrap_or(PackageManager::Npm).program();
    format!("{program} run {script}")
}
