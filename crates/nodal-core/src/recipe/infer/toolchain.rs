//! Which tool versions the project pins.
//!
//! Two kinds of evidence, kept apart on purpose. A pin file is what a version manager
//! reads, so it is what a shell in the home will actually select. An `engines` field is
//! what the package manager checks at install time; it is recorded under an `engines.`
//! prefix so that a mismatch between the two is visible rather than silently merged.

use crate::model::recipe::{Recipe, ToolName, ToolVersion};
use crate::recipe::gap::{Gap, GapKey};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Pin files, and the tool each one pins.
const PIN_FILES: &[(&str, &str)] = &[
    (".nvmrc", "node"),
    (".node-version", "node"),
    (".tool-versions", "asdf"),
    ("mise.toml", "mise"),
    (".mise.toml", "mise"),
    ("rust-toolchain.toml", "rust"),
    (".python-version", "python"),
];

/// Propose `toolchain`, or raise [`GapKey::Toolchain`] when the project pins nothing.
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    for (file, tool) in PIN_FILES {
        let Some(version) = project.read_pin(file) else { continue };
        if let Some(pin) = pin(tool, &version) {
            proposal.recipe.toolchain.insert(pin.0, pin.1);
            proposal = proposal.sure("toolchain", Confidence::High);
        }
    }
    for (tool, version) in project.engines() {
        if let Some(pin) = pin(&format!("engines.{tool}"), &version) {
            proposal.recipe.toolchain.insert(pin.0, pin.1);
            proposal.confidence.entry(String::from("toolchain")).or_insert(Confidence::Medium);
        }
    }
    if proposal.recipe.toolchain.is_empty() {
        proposal = proposal.gap(Gap::new(GapKey::Toolchain));
    }
    proposal
}

/// A pin, when both halves have the shape their types require.
fn pin(tool: &str, version: &str) -> Option<(ToolName, ToolVersion)> {
    Some((ToolName::parse(tool).ok()?, ToolVersion::parse(version).ok()?))
}
