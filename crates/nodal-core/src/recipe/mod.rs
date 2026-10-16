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

pub mod change;
pub mod gap;
pub mod infer;
pub mod merge;
pub mod parse;
pub mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::recipe::Recipe;
use crate::recipe::change::Change;
use crate::recipe::gap::Gap;
use crate::recipe::infer::{Confidence, Project};
use crate::recipe::merge::Merge;
use crate::services::ports;

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
    /// The text of the project's own `nodal.toml`, `None` when it has none.
    ///
    /// The bytes and not a flag saying there were some. `nodal init --force` says which
    /// lines it changes, and the only file that answers that is the one that is there
    /// now. Read once, here, so the command that reports the change and the command
    /// that parsed the recipe read one file one time.
    pub file: Option<String>,
}

/// Read `root`'s recipe: its file if it has one, over what its files imply.
///
/// # Errors
///
/// [`Error::Io`] if `nodal.toml` exists but cannot be read, and [`Error::Recipe`] if it
/// is not a recipe.
pub fn load(root: impl AsRef<Path>) -> Result<Effective> {
    loaded(root.as_ref()).map(|(effective, _)| effective)
}

/// [`load`], with the project it read, for a caller that reads the project once more.
fn loaded(root: &Path) -> Result<(Effective, Project)> {
    let path = root.join(FILE_NAME);
    let (explicit, file) = match std::fs::read_to_string(&path) {
        Ok(text) => (parse::parse(&text, &path)?, Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Recipe::default(), None),
        Err(error) => return Err(Error::io(&path)(error)),
    };

    let project = Project::open(root);
    let proposed = infer::infer(&project);
    let origin = if explicit.db.fixed_ports.is_empty() {
        PinSource::Inferred
    } else {
        PinSource::Written(path.clone())
    };
    let recipe = explicit.merge(proposed.recipe);
    refuse_a_pin_inside_the_granted_range(&recipe, origin)?;
    let gaps = proposed.gaps.into_iter().filter(|gap| !gap.key.is_answered_by(&recipe)).collect();
    Ok((Effective { recipe, gaps, confidence: proposed.confidence, file }, project))
}

/// Where a pinned port in the effective recipe came from.
///
/// `[db.fixed_ports]` merges whole: a table a person wrote replaces the inferred one
/// entirely, so one side or the other states every pin and a refusal can name which.
/// The operator's next action is not the same for the two. A written pin is a line in a
/// file that is there to edit. An inferred pin is in no file of the project's own, so
/// the file a message named would send the operator to a document nobody wrote; the
/// stack states that pin, and `nodal.toml` is where a person overrides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinSource {
    /// The project's own recipe, at this path, states the pin.
    Written(PathBuf),
    /// No recipe states the pin: it is read from the stack's own configuration.
    Inferred,
}

impl PinSource {
    /// What to do about a pin this source states, in the words a refusal carries.
    #[must_use]
    pub fn remedy(&self) -> String {
        match self {
            Self::Written(_) => String::from("pin it in db.fixed_ports outside that range"),
            Self::Inferred => {
                format!("change the stack, or state db.fixed_ports in {FILE_NAME} without it")
            }
        }
    }
}

impl std::fmt::Display for PinSource {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Written(path) => write!(out, "{}", path.display()),
            Self::Inferred => out.write_str("the stack's configuration"),
        }
    }
}

/// Refuse a pinned port that sits inside the range a unit's ports are granted from.
///
/// Nodal does not lease a pinned port and does not keep it out of a block: it is one
/// number for the whole project, and the only thing that keeps it apart from a granted
/// port is that the two ranges do not overlap. A pin inside the block range breaks that,
/// and the allocator will hand the same number to a unit of the same project. The merged
/// recipe is checked, not only the file, so `nodal init` refuses an inferred pin before
/// it writes one that the next command cannot read. `origin` is read before the merge,
/// which is the one place that knows which side the pin came from.
fn refuse_a_pin_inside_the_granted_range(recipe: &Recipe, origin: PinSource) -> Result<()> {
    for (name, port) in &recipe.db.fixed_ports {
        if (ports::RANGE_FIRST..=ports::RANGE_LAST).contains(port) {
            return Err(Error::RecipePinInGrantedRange {
                origin,
                name: name.clone(),
                port: *port,
                first: ports::RANGE_FIRST,
                last: ports::RANGE_LAST,
            });
        }
    }
    Ok(())
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
    /// The text of the recipe that is there now, `None` when the project has none. Its
    /// keys are in `contents`: init proposes around what a person wrote, it never drops
    /// them. Its comments are not, which is what [`InitPlan::changes`] reports.
    pub existing: Option<String>,
    /// The lifecycle hooks the effective recipe declares. `nodal init` approves them as
    /// a set when it writes the file, and they are here so that it does not have to
    /// read the recipe a second time to know what it is approving.
    pub hooks: crate::model::Hooks,
    /// Where the lockfile of the inferred manager disagrees with its manifest, so that
    /// `nodal init` can say so before the first `nodal new` installs from it.
    pub disagreement: Option<String>,
}

/// Work out what `nodal init` should write for the project at `root`. Reads only.
///
/// # Errors
///
/// Whatever [`load`] returns.
pub fn plan_init(root: impl AsRef<Path>) -> Result<InitPlan> {
    let root = root.as_ref();
    let (effective, project) = loaded(root)?;
    let disagreement = infer::package_manager::disagreement(&project, &effective.recipe);
    Ok(InitPlan {
        path: root.join(FILE_NAME),
        contents: render::render(&effective.recipe, &effective.gaps),
        gaps: effective.gaps,
        existing: effective.file,
        hooks: effective.recipe.hooks,
        disagreement,
    })
}

impl InitPlan {
    /// Which lines writing this plan would change in the file that is there now.
    ///
    /// Empty where the plan and the file hold the same lines, which is what says a
    /// rewrite would change nothing. A project with no recipe yet has every line of the
    /// plan added, because that is what writing it does.
    #[must_use]
    pub fn changes(&self) -> Vec<Change> {
        change::lines(self.existing.as_deref().unwrap_or_default(), &self.contents)
    }

    /// Whether a recipe is already there.
    #[must_use]
    pub const fn existed(&self) -> bool {
        self.existing.is_some()
    }
}

/// Write the planned file. Idempotent: writing the same plan twice leaves the same
/// bytes, and an existing recipe is only overwritten when `overwrite` says so.
///
/// Without `overwrite` the file is created exclusively rather than checked and then
/// written, so a recipe that appeared since the plan was made is still not clobbered.
///
/// # Errors
///
/// [`Error::RecipeExists`] when a recipe is already there and `overwrite` is false, and
/// [`Error::Io`] if the file cannot be written.
pub fn apply_init(plan: &InitPlan, overwrite: bool) -> Result<()> {
    use std::io::Write as _;

    if overwrite {
        return std::fs::write(&plan.path, &plan.contents).map_err(Error::io(&plan.path));
    }
    let mut file = match std::fs::File::create_new(&plan.path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(Error::RecipeExists { path: plan.path.clone() });
        }
        Err(error) => return Err(Error::io(&plan.path)(error)),
    };
    file.write_all(plan.contents.as_bytes()).map_err(Error::io(&plan.path))
}
