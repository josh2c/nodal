//! Inference: read what a project already says about itself and propose a recipe.
//!
//! Measured against real projects before it was written: a Node project with a local
//! Supabase stack needs a recipe of a few lines, most of them confirmations rather than
//! decisions. The engine keeps that shape: every source proposes keys it can defend
//! from a file it read, records how sure it is, and raises a [`Gap`] instead of
//! guessing when the answer is not in the repository.
//!
//! Each source is one file and one function, `infer(project, so_far) -> Proposal`. They
//! run in the order of [`SOURCES`], and a later source may read what earlier ones
//! proposed — the migration commands need the package manager, for example. Nothing
//! else couples them, so adding a stack means adding a file and a row.

pub mod backend;
pub mod env_names;
pub mod layout;
pub mod migrations;
pub mod package_manager;
pub mod scripts;
pub mod services;
pub mod source;
pub mod toolchain;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::recipe::Recipe;
use crate::recipe::gap::Gap;
use crate::recipe::merge::Merge;

pub use crate::recipe::infer::source::Project;

/// How much a proposed key can be relied on.
///
/// `High` means a file states it: a lockfile names the package manager, a script exists
/// under the name it is used by. `Medium` means it follows from a convention Nodal
/// chose, and is the level at which `nodal init` asks for a confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Follows from a convention rather than from the project's own files.
    Medium,
    /// Stated by a file in the project.
    High,
}

/// What one source, or the whole engine, has to propose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// The keys this source could fill in.
    pub recipe: Recipe,
    /// How sure it is, by `nodal.toml` key path.
    pub confidence: BTreeMap<String, Confidence>,
    /// What it could not answer.
    pub gaps: Vec<Gap>,
}

impl Proposal {
    /// Record a confidence for one key path.
    #[must_use]
    pub fn sure(mut self, key: &str, confidence: Confidence) -> Self {
        self.confidence.insert(key.to_owned(), confidence);
        self
    }

    /// Record a gap.
    #[must_use]
    pub fn gap(mut self, gap: Gap) -> Self {
        self.gaps.push(gap);
        self
    }

    /// Fold a later source into this one. Earlier sources win, because the table runs
    /// from the most direct evidence to the least.
    fn absorb(&mut self, later: Self) {
        let earlier = std::mem::take(&mut self.recipe);
        self.recipe = earlier.merge(later.recipe);
        for (key, confidence) in later.confidence {
            self.confidence.entry(key).or_insert(confidence);
        }
        self.gaps.extend(later.gaps);
    }
}

/// One inference source: what it is called, and the function that runs it.
type Source = (&'static str, fn(&Project, &Recipe) -> Proposal);

/// Every source, in the order they run. A later source may read what earlier ones
/// proposed; nothing may read what a later one will.
const SOURCES: &[Source] = &[
    ("package manager", package_manager::infer),
    ("toolchain", toolchain::infer),
    ("scripts", scripts::infer),
    ("layout", layout::infer),
    ("migrations", migrations::infer),
    ("services", services::infer),
    ("backend", backend::infer),
    ("environment names", env_names::infer),
];

/// Run every source over `project` and return the recipe they propose together.
#[must_use]
pub fn infer(project: &Project) -> Proposal {
    let mut proposal = Proposal::default();
    for (name, source) in SOURCES {
        let found = source(project, &proposal.recipe);
        tracing::debug!(source = name, gaps = found.gaps.len(), "recipe inference: source ran");
        proposal.absorb(found);
    }
    proposal.gaps.sort_by_key(|gap| gap.key);
    proposal.gaps.dedup_by_key(|gap| gap.key);
    proposal
}
