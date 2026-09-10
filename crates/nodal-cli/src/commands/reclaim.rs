//! `nodal reclaim`: end a unit, move its home to the trash, and say what is left.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::ops::reclaim::{self, Request};
use nodal_core::output::{self, Format};
use nodal_core::store::Store;

use crate::commands::context;

/// Arguments of `nodal reclaim`.
#[derive(Debug, Args)]
pub struct Reclaim {
    /// The unit's handle. Defaults to the unit the working directory is in.
    #[arg(value_name = "UNIT")]
    pub unit: Option<String>,

    /// Reclaim a unit whose home holds work that exists nowhere else, or whose home
    /// something Nodal did not start is standing in. The work is committed to a
    /// snapshot ref inside the home first, and the home goes to the trash rather than
    /// being deleted, so nothing here is a way to lose a commit.
    #[arg(long)]
    pub force: bool,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Reclaim {
    /// Reclaim the unit and print what was done, including what was left.
    ///
    /// The exit code says whether the verification found anything. A reclaim that left
    /// something behind is not a failure — it did what it could and reported the rest —
    /// but a script that reclaims a hundred units needs to know which of them to look
    /// at, and the report on standard output is where it reads what.
    ///
    /// # Errors
    ///
    /// Propagates a home that holds work that is only there, a home something Nodal
    /// did not start is standing in, a unit that was reclaimed already, a hook this
    /// machine has not approved, and whatever Git, the filesystem or the registry
    /// reported.
    pub fn run(&self, store: &mut Store, hooks: bool) -> nodal_core::Result<ExitCode> {
        let request = Request {
            target: self.unit.clone(),
            force: self.force,
            hooks,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };
        let project = context::project_of(store, self.unit.as_deref(), &request.cwd);
        let report = reclaim::reclaim(store, &request)?;
        if let Some(project) = &project {
            context::refresh(store, project);
        }
        let left = !report.leftovers.is_empty();
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(if left { ExitCode::FAILURE } else { ExitCode::SUCCESS })
    }
}
