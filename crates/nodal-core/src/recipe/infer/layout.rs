//! How the repository is laid out: one package or many, what caches its tasks, and the
//! directories a base clone should not carry.
//!
//! The exclusions are the ones that cost the most and are worth the least in a fresh
//! home: build output, test output, and the run directories other tools keep. They are
//! a starting list a project edits, not a policy — `base.exclude` in `nodal.toml` wins.

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
    proposal.recipe.base.exclude =
        project.all_existing(HEAVY_DIRECTORIES).iter().map(Into::into).collect();
    if proposal.recipe.task_cache.is_some() {
        proposal = proposal.sure("task_cache", Confidence::High);
    }
    if proposal.recipe.base.exclude.is_empty() {
        return proposal;
    }
    proposal.sure("base.exclude", Confidence::High)
}
