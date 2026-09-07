//! Which package manager the project installs with, and the version it pins.
//!
//! The lockfile is the evidence: it is committed, it is the file CI installs from, and
//! it names exactly one manager. The order below is the order of specificity, so a
//! repository that carries two lockfiles resolves to the one its own tooling would use.

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
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    if let Some((_, manager)) = LOCKFILES.iter().find(|(lockfile, _)| project.exists(lockfile)) {
        proposal.recipe.package_manager = Some(*manager);
        proposal = proposal.sure("package_manager", Confidence::High);
    }
    proposal.recipe.package_manager_pin = project
        .package_json()
        .get("packageManager")
        .and_then(serde_json::Value::as_str)
        .and_then(|pin| ToolVersion::parse(pin).ok());
    proposal
}

/// The command that runs a named script, in the package manager the recipe settled on.
#[must_use]
pub fn run_script(recipe: &Recipe, script: &str) -> String {
    let program = recipe.package_manager.unwrap_or(PackageManager::Npm).program();
    format!("{program} run {script}")
}
