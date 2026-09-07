//! `nodal init`: write the project's `nodal.toml`, with a line for every gap.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::output::view::InitReport;
use nodal_core::output::{self, Format};
use nodal_core::recipe;

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
        let report = InitReport::from_plan(&plan);
        if self.json {
            return write(&report, Format::Json);
        }
        if self.print {
            print!("{}", plan.contents);
            return Ok(ExitCode::SUCCESS);
        }
        recipe::apply_init(&plan, self.force)?;
        write(&report, Format::Human)
    }
}

/// Print a read type in the format asked for. Every command ends this way, so the two
/// renderings stay two views of one value (`nodal_core::output`).
fn write(report: &InitReport, format: Format) -> nodal_core::Result<ExitCode> {
    output::write(report, format, &mut std::io::stdout())?;
    Ok(ExitCode::SUCCESS)
}
