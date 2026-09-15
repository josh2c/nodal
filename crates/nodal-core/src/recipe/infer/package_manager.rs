//! Which package managers the project installs with, and the version it pins.
//!
//! The lockfile is the evidence: it is committed, it is the file CI installs from, and
//! it names exactly one manager. A repository of one ecosystem carries one; a repository
//! of a Rust binary, a Node CLI and a Python tool carries three, and every one of them
//! has to be installed or the base is warm for a third of the tree.
//!
//! So every lockfile present is proposed, in the order below. The order is the order of
//! specificity inside an ecosystem, so a repository that carries both a `pnpm-lock.yaml`
//! and a `package-lock.json` still installs with the one its own tooling would use.
//!
//! **Which of them is the primary is a second question.** The primary is the manager a
//! bare script name resolves against, so it is the manager that leads the repository,
//! and the file that says which one that is, is the root manifest that declares the
//! build the recipe takes: a Rust-led repository with a `package.json` beside its
//! `Cargo.toml` builds with `cargo build`, and its primary is Cargo however many Node
//! files it carries. Where two root manifests declare a build, and where none does, the
//! tie falls to the manager the root manifest names — a `packageManager` field names its
//! program whether or not it also pins a version. A repository that states neither keeps
//! the table's own order.

use crate::model::recipe::{Ecosystem, PackageManager, Recipe, ToolVersion};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// The lockfile each package manager writes, most specific first.
const LOCKFILES: &[(&str, PackageManager)] = &[
    ("pnpm-lock.yaml", PackageManager::Pnpm),
    ("yarn.lock", PackageManager::Yarn),
    ("package-lock.json", PackageManager::Npm),
    ("bun.lockb", PackageManager::Bun),
    ("Cargo.lock", PackageManager::Cargo),
    ("uv.lock", PackageManager::Uv),
    ("poetry.lock", PackageManager::Poetry),
];

/// The `package.json` field that names the manager the repository is driven by.
const PIN: &str = "packageManager";

/// Propose `package_manager` and `package_manager_pin`.
///
/// One manager per ecosystem. Two lockfiles of one ecosystem are a repository mid-way
/// through changing manager, and installing with both would write two dependency trees
/// over each other, so the more specific one wins and the other is not proposed.
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    let mut ecosystems: Vec<Ecosystem> = Vec::new();
    for (lockfile, manager) in LOCKFILES {
        if !project.exists(lockfile) || ecosystems.contains(&manager.ecosystem()) {
            continue;
        }
        ecosystems.push(manager.ecosystem());
        proposal.recipe.package_manager.push(*manager);
    }
    lead(project, &mut proposal.recipe.package_manager);
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

/// Move the primary to the front of `proposed`, and leave the rest in their order.
fn lead(project: &Project, proposed: &mut [PackageManager]) {
    let Some(at) = primary(project, proposed) else { return };
    proposed[..=at].rotate_right(1);
}

/// Where the manager that leads this repository is, `None` where the project states no
/// more than the lockfiles it carries.
fn primary(project: &Project, proposed: &[PackageManager]) -> Option<usize> {
    let mut builders =
        proposed.iter().enumerate().filter(|(_, manager)| builds(project, **manager));
    match (builders.next(), builders.next()) {
        (Some((only, _)), None) => Some(only),
        _ => named(project, proposed),
    }
}

/// Whether this manager's root manifest declares the build the recipe takes.
///
/// The same reading as the source that proposes the build command, so "which manager
/// builds this repository" and "which command builds it" cannot disagree. Every Cargo
/// tree builds with `cargo build` ([`super::cargo`]); a Node tree builds where its
/// script table declares a build ([`super::scripts`]); a Python project declares no
/// build at all ([`super::python`]).
fn builds(project: &Project, manager: PackageManager) -> bool {
    match manager.ecosystem() {
        Ecosystem::Rust => project.exists(super::cargo::MANIFEST),
        Ecosystem::Node => project.scripts().contains_key(super::scripts::BUILD),
        Ecosystem::Python => false,
    }
}

/// The proposed manager the root manifest names.
///
/// The program out of the `packageManager` field, which names it whether or not it
/// carries a version: `"packageManager": "pnpm"` names pnpm exactly as `"pnpm@9.12.3"`
/// does. A field that names a manager the project carries no lockfile for names nothing
/// here, because a manager that is not proposed cannot lead the list.
fn named(project: &Project, proposed: &[PackageManager]) -> Option<usize> {
    let pin = project.package_json().get(PIN)?.as_str()?;
    let program = pin.split('@').next()?;
    proposed.iter().position(|manager| manager.program() == program)
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
