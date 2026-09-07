//! Where the project's commands run.
//!
//! One answer, chosen from the definition file the project ships. A project with no
//! such file runs natively, which is a statement rather than a fallback: it is what
//! `nodal shell` will do, and it is the case every other backend is measured against.

use crate::model::recipe::{Backend, Recipe};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Definition files, and the backend each implies.
const DEFINITIONS: &[(&str, Backend)] = &[
    (".devcontainer", Backend::Devcontainer),
    ("flake.nix", Backend::Nix),
    ("shell.nix", Backend::Nix),
];

/// Propose `backend`.
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let backend = DEFINITIONS
        .iter()
        .find(|(file, _)| project.exists(file))
        .map_or(Backend::Native, |(_, backend)| *backend);
    let mut proposal = Proposal::default();
    proposal.recipe.backend = Some(backend);
    proposal.sure("backend", Confidence::High)
}
