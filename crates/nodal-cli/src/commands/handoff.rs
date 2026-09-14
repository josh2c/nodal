//! `nodal handoff`: leave a note for whoever continues a unit.

use std::process::ExitCode;

use clap::Args;
use nodal_core::output::{self, Format};
use nodal_core::runtime::{entry, handoff};
use nodal_core::store::Store;

/// Arguments of `nodal handoff`.
#[derive(Debug, Args)]
pub struct Handoff {
    /// The unit's handle. Defaults to the unit the working directory is in.
    #[arg(long, value_name = "UNIT")]
    pub unit: Option<String>,

    /// What to leave for whoever continues the unit.
    #[arg(value_name = "TEXT")]
    pub text: String,

    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Handoff {
    /// Record the handoff and print what was recorded.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::UnitNotFound`] when no unit has the handle,
    /// [`nodal_core::Error::EmptyHandoff`] when the text says nothing, and whatever the
    /// registry reported.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let text = self.rendered(store, Format::from_json_flag(self.json))?;
        print!("{text}");
        Ok(ExitCode::SUCCESS)
    }

    /// What this command writes, in the format asked for.
    ///
    /// One reading and one rendering, so the text a person sees and the text a tool
    /// reads come from one value ([`output::render`]).
    ///
    /// # Errors
    ///
    /// Whatever the registry reported, and the refusal of a handoff that says nothing.
    pub fn rendered(&self, store: &Store, format: Format) -> nodal_core::Result<String> {
        let cwd = std::env::current_dir().map_err(nodal_core::Error::io("."))?;
        let unit = entry::unit_named(store.conn(), self.unit.as_deref(), &cwd)?;
        let log = handoff::state(store.conn(), &unit, &self.text)?;
        output::render(&log, format)
    }
}
