//! The commands a Cargo tree is driven by.
//!
//! Cargo has one vocabulary and every crate uses it, so `build` and `test` are read off
//! the manifest's existence rather than out of a script table: a project with a
//! `Cargo.toml` builds with `cargo build` and tests with `cargo test`, and one that does
//! it differently says so in `nodal.toml`.
//!
//! `lint` is not read that way, because `cargo clippy` is a component a toolchain may
//! not have. It is proposed only where the tree states that the project lints with
//! Clippy: a Clippy configuration file, or a lint table in the manifest. Inference
//! reads files; it never asks the host whether a tool is installed.

use crate::model::recipe::{CommandLine, PackageManager, Recipe};
use crate::recipe::infer::scripts::SetCommand;
use crate::recipe::infer::{Confidence, Project, Proposal};

/// The manifest every Cargo tree has.
const MANIFEST: &str = "Cargo.toml";

/// Where a project states that it lints with Clippy, as a file of its own.
const CLIPPY_FILES: &[&str] = &["clippy.toml", ".clippy.toml"];

/// Where a project states the same thing inside its manifest.
const LINT_TABLES: &[&str] = &["lints", "workspace.lints"];

/// The commands every Cargo tree has: the recipe key, the field it fills, and the line.
const ALWAYS: &[(&str, SetCommand, &str)] = &[
    ("build", |recipe, line| recipe.commands.build = Some(line), "cargo build"),
    ("test", |recipe, line| recipe.commands.test = Some(line), "cargo test"),
];

/// The command a tree has only where it states the tool.
const WHERE_STATED: (&str, SetCommand, &str) =
    ("lint", |recipe, line| recipe.commands.lint = Some(line), "cargo clippy");

/// Propose the commands a Cargo tree states.
#[must_use]
pub fn infer(project: &Project, so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    if !so_far.package_manager.contains(&PackageManager::Cargo) || !project.exists(MANIFEST) {
        return proposal;
    }
    let stated = lints_with_clippy(project).then_some(&WHERE_STATED);
    for (key, set, line) in ALWAYS.iter().chain(stated) {
        let Ok(line) = CommandLine::parse(*line) else { continue };
        set(&mut proposal.recipe, line);
        proposal = proposal.sure(&format!("commands.{key}"), Confidence::High);
    }
    proposal
}

/// Whether the tree states that this project lints with Clippy.
fn lints_with_clippy(project: &Project) -> bool {
    if project.first_existing(CLIPPY_FILES).is_some() {
        return true;
    }
    let Some(manifest) = project.read_toml(MANIFEST) else { return false };
    LINT_TABLES
        .iter()
        .any(|path| path.split('.').try_fold(&manifest, |table, key| table.get(key)).is_some())
}
