//! `nodal done`: push the unit's work and print where the change is opened.

use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::ops::done::{self, Request};
use nodal_core::output::{self, Format};
use nodal_core::store::Store;

/// Arguments of `nodal done`.
#[derive(Debug, Args)]
pub struct Done {
    /// The unit's handle. Defaults to the unit the working directory is in.
    #[arg(value_name = "UNIT")]
    pub unit: Option<String>,

    /// The remote to push to. Defaults to `origin`, then to the only remote there is.
    #[arg(long, value_name = "REMOTE")]
    pub remote: Option<String>,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Done {
    /// Push the branch and the work-in-progress ref, and put the unit up for review.
    ///
    /// # Errors
    ///
    /// Propagates a unit with no home, a repository that does not decide which remote a
    /// push goes to, a push the remote refused, and whatever Git or the registry
    /// reported.
    pub fn run(&self, store: &mut Store) -> nodal_core::Result<ExitCode> {
        let request = Request {
            target: self.unit.clone(),
            remote: self.remote.clone(),
            cwd: std::env::current_dir().map_err(nodal_core::Error::io("."))?,
        };
        let report = done::done(store, &request)?;
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
