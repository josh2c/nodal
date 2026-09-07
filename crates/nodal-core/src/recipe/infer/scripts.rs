//! The commands the project is driven by, taken from its own script names.
//!
//! Only the conventional names are read, and each is proposed as the package manager
//! would run it rather than as the script body, so the recipe stays true when the body
//! changes. A project whose test command is not called `test` says so in `nodal.toml`;
//! that is one of the lines a recipe exists for.

use crate::model::recipe::{CommandLine, Recipe};
use crate::recipe::infer::package_manager::run_script;
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Where one inferred command line is written on the recipe.
type SetCommand = fn(&mut Recipe, CommandLine);

/// Script names that map straight onto a recipe command, and the field each fills.
const SCRIPTS: &[(&str, SetCommand)] = &[
    ("dev", |recipe, line| recipe.commands.dev = Some(line)),
    ("build", |recipe, line| recipe.commands.build = Some(line)),
    ("test", |recipe, line| recipe.commands.test = Some(line)),
    ("lint", |recipe, line| recipe.commands.lint = Some(line)),
    ("typecheck", |recipe, line| recipe.commands.typecheck = Some(line)),
];

/// Propose `commands.*` for every conventional script the project declares.
#[must_use]
pub fn infer(project: &Project, so_far: &Recipe) -> Proposal {
    let declared = project.scripts();
    let mut proposal = Proposal::default();
    for (name, set) in SCRIPTS {
        if !declared.contains_key(*name) {
            continue;
        }
        let Ok(line) = CommandLine::parse(run_script(so_far, name)) else { continue };
        set(&mut proposal.recipe, line);
        proposal = proposal.sure(&format!("commands.{name}"), Confidence::High);
    }
    proposal
}
