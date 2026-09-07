//! How the repository is laid out: one package or many, what caches its tasks, and the
//! directories a base clone should not carry.
//!
//! The exclusions are the ones that cost the most and are worth the least in a fresh
//! home: build output, test output, and the run directories other tools keep. They are
//! a starting list a project edits, not a policy — `base.exclude` in `nodal.toml` wins.
//!
//! A heavy directory the project tracks is never proposed. The name says the content is
//! generated, and the commit says it is the project's own; the commit wins. A copy that
//! left it out would report a deletion for every file under it the moment it was made.
//! [`crate::workspace::tracked`] refuses such a list at the copy, and this source is
//! why an inferred recipe never carries one.

use std::path::PathBuf;

use crate::model::recipe::{Recipe, TaskCache};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Files that mark a repository as holding more than one package.
const MONOREPO_MARKERS: &[&str] =
    &["pnpm-workspace.yaml", "turbo.json", "nx.json", "lerna.json", "rush.json"];

/// Task-cache configuration files, and the runner each belongs to.
const TASK_CACHES: &[(&str, TaskCache)] =
    &[("turbo.json", TaskCache::Turborepo), ("nx.json", TaskCache::Nx)];

/// Directories that are large, regenerated on demand, and not worth cloning.
const HEAVY_DIRECTORIES: &[&str] = &[
    ".claude/worktrees",
    ".playbook/runs",
    "test-results",
    "playwright-report",
    ".next",
    "coverage",
];

/// Propose `monorepo`, `task_cache` and `base.exclude`.
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    proposal.recipe.monorepo = Some(MONOREPO_MARKERS.iter().any(|marker| project.exists(marker)));
    proposal.recipe.task_cache =
        TASK_CACHES.iter().find(|(file, _)| project.exists(file)).map(|(_, cache)| *cache);
    proposal.recipe.base.exclude = excludable(project);
    if proposal.recipe.task_cache.is_some() {
        proposal = proposal.sure("task_cache", Confidence::High);
    }
    if proposal.recipe.base.exclude.is_empty() {
        return proposal;
    }
    proposal.sure("base.exclude", Confidence::High)
}

/// The heavy directories the project has and does not track.
///
/// The order of [`HEAVY_DIRECTORIES`] is kept, so two runs over one project propose the
/// same list in the same order.
fn excludable(project: &Project) -> Vec<PathBuf> {
    let present = project.all_existing(HEAVY_DIRECTORIES);
    let tracked = project.tracked(&present);
    for path in &tracked {
        tracing::debug!(path, "recipe inference: a heavy directory is tracked, so it is kept");
    }
    present.iter().filter(|path| !tracked.contains(path)).map(|path| PathBuf::from(*path)).collect()
}
