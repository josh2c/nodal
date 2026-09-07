//! The recipe: read `nodal.toml`, infer what it does not say, and name what is left.
//!
//! Three steps, in one order, everywhere. [`infer`] proposes a recipe from the
//! project's own files. [`parse`] reads the file a person wrote. [`merge`] puts the
//! second over the first, key by key. What inference could not answer and the file did
//! not either is a [`gap::Gap`], and `nodal init` writes each gap into the file as the
//! question it is.
//!
//! `nodal init` is split the way every operation is: [`plan_init`] is pure
//! over what it read and returns the exact bytes, and [`apply_init`] is the only part
//! that writes. The plan is what `--print` shows, so what a person reviews is what
//! lands.

pub mod gap;
pub mod infer;
pub mod merge;
pub mod parse;
pub mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::recipe::Recipe;
use crate::recipe::gap::Gap;
use crate::recipe::infer::{Confidence, Project};
use crate::recipe::merge::Merge;

/// The file a project's recipe lives in, at the project root.
pub const FILE_NAME: &str = "nodal.toml";

/// A project's recipe as it actually applies: the file over the inference, with
/// whatever neither of them answered still named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// The merged recipe.
    pub recipe: Recipe,
    /// What no source could answer, in a stable order.
    pub gaps: Vec<Gap>,
    /// How sure inference was, by key path. A key the file states is not listed: it is
    /// not a guess.
    pub confidence: BTreeMap<String, Confidence>,
    /// Whether the project has a `nodal.toml` at all.
    pub written: bool,
}

/// Read `root`'s recipe: its file if it has one, over what its files imply.
///
/// # Errors
///
/// [`Error::Io`] if `nodal.toml` exists but cannot be read, and [`Error::Recipe`] if it
/// is not a recipe.
pub fn load(root: impl AsRef<Path>) -> Result<Effective> {
    let root = root.as_ref();
    let path = root.join(FILE_NAME);
    let (explicit, written) = match std::fs::read_to_string(&path) {
        Ok(text) => (parse::parse(&text, &path)?, true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Recipe::default(), false),
        Err(error) => return Err(Error::io(&path)(error)),
    };

    let proposed = infer::infer(&Project::open(root));
    let recipe = explicit.merge(proposed.recipe);
    let gaps = proposed.gaps.into_iter().filter(|gap| !gap.key.is_answered_by(&recipe)).collect();
    Ok(Effective { recipe, gaps, confidence: proposed.confidence, written })
}

/// What `nodal init` would write, and what it would leave for a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPlan {
    /// The file that would be written.
    pub path: PathBuf,
    /// Its exact contents.
    pub contents: String,
    /// The questions the file will carry.
    pub gaps: Vec<Gap>,
    /// Whether a recipe was already there. Its keys are in `contents`: init proposes
    /// around what a person wrote, it never drops it.
    pub existed: bool,
}

/// Work out what `nodal init` should write for the project at `root`. Reads only.
///
/// # Errors
///
/// Whatever [`load`] returns.
pub fn plan_init(root: impl AsRef<Path>) -> Result<InitPlan> {
    let root = root.as_ref();
    let effective = load(root)?;
    Ok(InitPlan {
        path: root.join(FILE_NAME),
        contents: render::render(&effective.recipe, &effective.gaps),
        gaps: effective.gaps,
        existed: effective.written,
    })
}

/// Write the planned file. Idempotent: writing the same plan twice leaves the same
/// bytes, and an existing recipe is only overwritten when `overwrite` says so.
///
/// # Errors
///
/// [`Error::RecipeExists`] when a recipe is already there and `overwrite` is false, and
/// [`Error::Io`] if the file cannot be written.
pub fn apply_init(plan: &InitPlan, overwrite: bool) -> Result<()> {
    if plan.existed && !overwrite {
        return Err(Error::RecipeExists { path: plan.path.clone() });
    }
    std::fs::write(&plan.path, &plan.contents).map_err(Error::io(&plan.path))
}
