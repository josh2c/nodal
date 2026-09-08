//! `nodal explain`: why one unit is the way it is.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::model::Timestamp;
use nodal_core::output::{self, Format};
use nodal_core::runtime::{entry, explain};
use nodal_core::store::{Store, projects};

/// Arguments of `nodal explain`.
#[derive(Debug, Args)]
pub struct Explain {
    /// The unit's handle, or a directory in it. Defaults to the unit the working
    /// directory is in.
    #[arg(value_name = "UNIT")]
    pub unit: Option<String>,

    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Explain {
    /// Print where the unit's home came from, what it did not receive, what was removed
    /// from it and where its ports came from.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::UnitNotFound`] when no unit has that handle,
    /// [`nodal_core::Error::UnitNotMaterialized`] when it has no home on any host, and
    /// whatever the registry or the recipe reader reported.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let unit = entry::unit_named(store.conn(), self.unit.as_deref(), &cwd)?;
        let project = projects::get(store.conn(), unit.project_id)?.ok_or_else(|| {
            nodal_core::Error::StoreMissingRow { table: "project", id: unit.project_id.to_string() }
        })?;
        let answer = explain::explain(store.conn(), &project, &unit, Timestamp::now())?;
        output::write(&answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
