//! Which package managers the project installs with, and the version it pins.
//!
//! The lockfile is the evidence: it is committed, it is the file CI installs from, and
//! it names exactly one manager. A repository of one ecosystem carries one; the three
//! day proof's project carries three, and every one of them has to be installed or the
//! base is warm for a third of the tree.
//!
//! So every lockfile present is proposed, in the order below, and the first is the
//! primary: the manager a bare script name resolves against. The order is the order of
//! specificity inside an ecosystem, so a repository that carries both a `pnpm-lock.yaml`
//! and a `package-lock.json` still installs with the one its own tooling would use.

use crate::model::recipe::{PackageManager, Recipe, ToolVersion};
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
        if !project.exists(lockfile) || ecosystems.contains(&ecosystem(*manager)) {
            continue;
        }
        ecosystems.push(ecosystem(*manager));
        proposal.recipe.package_manager.push(*manager);
    }
    if !proposal.recipe.package_manager.is_empty() {
        proposal = proposal.sure("package_manager", Confidence::High);
    }
    proposal.recipe.package_manager_pin = project
        .package_json()
        .get("packageManager")
        .and_then(serde_json::Value::as_str)
        .and_then(|pin| ToolVersion::parse(pin).ok());
    proposal
}

/// The dependency tree a manager writes. Two managers of one ecosystem write the same
/// one, so a project installs with at most one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ecosystem {
    /// `node_modules`.
    Node,
    /// The Cargo registry cache.
    Rust,
    /// A virtual environment.
    Python,
}

/// Which dependency tree a manager writes.
const fn ecosystem(manager: PackageManager) -> Ecosystem {
    match manager {
        PackageManager::Pnpm | PackageManager::Yarn | PackageManager::Npm | PackageManager::Bun => {
            Ecosystem::Node
        }
        PackageManager::Cargo => Ecosystem::Rust,
        PackageManager::Uv | PackageManager::Poetry => Ecosystem::Python,
    }
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
