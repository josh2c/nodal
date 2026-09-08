//! Which tool versions the project pins.
//!
//! Two kinds of evidence, kept apart on purpose. A pin file is what a version manager
//! reads, so it is what a shell in the home will actually select. A manifest field is
//! what a package manager checks at install time; `engines` in `package.json` and
//! `rust-version` in `Cargo.toml` are both of that kind, and each one is recorded under
//! a prefix naming the file it came from, so that a mismatch between a manifest and a
//! pin file is visible rather than silently merged.

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
    for (name, version) in manifest_pins(project) {
        if let Some(pin) = pin(&name, &version) {
            proposal.recipe.toolchain.insert(pin.0, pin.1);
            proposal.confidence.entry(String::from("toolchain")).or_insert(Confidence::Medium);
        }
    }
    if proposal.recipe.toolchain.is_empty() {
        proposal = proposal.gap(Gap::new(GapKey::Toolchain));
    }
    proposal
}

/// The version constraints the project's manifests state, by the name each is recorded
/// under.
///
/// A Cargo `rust-version` is the minimum the crate builds with, which is the same kind
/// of claim `engines` makes: what the tool checks, not what a version manager selects.
/// A workspace states it once in `workspace.package`, and a member crate in `package`,
/// so both are read and the crate's own answer wins.
fn manifest_pins(project: &Project) -> Vec<(String, String)> {
    let mut pins: Vec<(String, String)> = project
        .engines()
        .into_iter()
        .map(|(tool, version)| (format!("engines.{tool}"), version))
        .collect();
    if let Some(version) = rust_version(project) {
        pins.push((String::from(CARGO_RUST), version));
    }
    pins
}

/// The name a Cargo `rust-version` is recorded under.
const CARGO_RUST: &str = "cargo.rust";

/// `rust-version` from `Cargo.toml`, from the crate's own table or the workspace's.
fn rust_version(project: &Project) -> Option<String> {
    let manifest = project.read_toml("Cargo.toml")?;
    let read = |table: &str| {
        manifest
            .get(table)
            .and_then(|value| if table == "workspace" { value.get("package") } else { Some(value) })
            .and_then(|package| package.get("rust-version"))
            .and_then(|version| version.as_str())
            .map(str::to_owned)
    };
    read("package").or_else(|| read("workspace"))
}

/// A pin, when both halves have the shape their types require.
fn pin(tool: &str, version: &str) -> Option<(ToolName, ToolVersion)> {
    Some((ToolName::parse(tool).ok()?, ToolVersion::parse(version).ok()?))
}
