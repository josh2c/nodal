//! `nodal new`: make a unit, its branch and a home to work in.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::ops::new::{self, Request};
use nodal_core::model::{BranchName, Objective, Slug};
use nodal_core::output::{self, Format};
use nodal_core::store::Store;

/// Arguments of `nodal new`.
#[derive(Debug, Args)]
pub struct New {
    /// What the unit is for. The handle is derived from it unless `--name` says.
    #[arg(value_name = "OBJECTIVE")]
    pub objective: Option<String>,

    /// The handle to give the unit, instead of deriving one.
    #[arg(long, value_name = "SLUG")]
    pub name: Option<String>,

    /// The branch the work starts from.
    #[arg(long, value_name = "BRANCH")]
    pub from: Option<String>,

    /// A directory in the project. Defaults to the working directory.
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl New {
    /// Create the unit and report what it was given.
    ///
    /// # Errors
    ///
    /// Propagates a branch another open unit holds, a home that would overlap a tree
    /// Nodal knows, and whatever Git, the filesystem or the registry reported.
    pub fn run(&self, store: &mut Store) -> nodal_core::Result<ExitCode> {
        let report = new::create(store, &self.request()?)?;
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }

    /// The arguments as the values the operation takes.
    fn request(&self) -> nodal_core::Result<Request> {
        Ok(Request {
            source: self.path.clone().unwrap_or_else(|| PathBuf::from(".")),
            objective: self.objective.as_deref().map(Objective::parse).transpose()?,
            name: self.name.as_deref().map(Slug::parse).transpose()?,
            parent_branch: self.from.as_deref().map(BranchName::parse).transpose()?,
        })
    }
}
