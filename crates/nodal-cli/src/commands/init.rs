//! `nodal init`: write the project's `nodal.toml`, with a line for every gap.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::recipe::{self, InitPlan};

/// Arguments of `nodal init`.
#[derive(Debug, Args)]
pub struct Init {
    /// The project root. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Rewrite an existing `nodal.toml`, keeping the keys it already sets.
    #[arg(long)]
    pub force: bool,

    /// Print what would be written and write nothing.
    #[arg(long)]
    pub print: bool,

    /// Print the plan as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Init {
    /// Infer the recipe, then print it or write it.
    ///
    /// # Errors
    ///
    /// Propagates a recipe that cannot be read, and a file that cannot be written.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        let root = self.path.clone().unwrap_or_else(|| PathBuf::from("."));
        let plan = recipe::plan_init(&root)?;
        if self.json {
            println!("{}", json(&plan));
            return Ok(ExitCode::SUCCESS);
        }
        if self.print {
            print!("{}", plan.contents);
            return Ok(ExitCode::SUCCESS);
        }
        recipe::apply_init(&plan, self.force)?;
        report(&plan);
        Ok(ExitCode::SUCCESS)
    }
}

/// What was written, and what is left for a person.
fn report(plan: &InitPlan) {
    println!("{}", plan.path.display());
    for gap in &plan.gaps {
        println!("  gap  {}: {}", gap.key.toml_key(), gap.key.question());
    }
    if plan.gaps.is_empty() {
        println!("  no gaps: every key was inferred");
    }
}

/// The plan as JSON, for a tool that wants the gaps without the file.
fn json(plan: &InitPlan) -> String {
    serde_json::json!({
        "path": plan.path,
        "existed": plan.existed,
        "contents": plan.contents,
        "gaps": plan.gaps,
    })
    .to_string()
}
