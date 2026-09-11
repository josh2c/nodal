//! `nodal show`: one unit in full, and its memory written again.

use std::process::ExitCode;

use clap::Args;
use nodal_core::context::survey;
use nodal_core::lifecycle::states;
use nodal_core::model::Timestamp;
use nodal_core::output::{self, Format};
use nodal_core::runtime::{entry, lock, ls, processes, show};
use nodal_core::store::Store;

use crate::commands::context;

/// Arguments of `nodal show`.
#[derive(Debug, Args)]
pub struct Show {
    /// The unit's handle. Defaults to the unit the working directory is in.
    #[arg(value_name = "UNIT")]
    pub unit: Option<String>,

    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Show {
    /// Print everything known about the unit, and write its memory again.
    ///
    /// The memory is written here because this is the command a person or an agent runs
    /// to ask what a unit is: asking is the moment the answer has to be current, and
    /// `WORKUNIT.md` is the same answer in the form the next agent reads.
    ///
    /// It is the list's own reading, taken once: the survey answers the row and the
    /// memories both, and the flip to merged is recorded here rather than inside the
    /// reading, exactly as it is for `nodal ls`. What could not be settled is not shown,
    /// because this answer is about one unit and the notes are the list's.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::UnitNotFound`] when no unit has the handle, and whatever
    /// the registry or Git reported.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let cwd = std::env::current_dir().map_err(nodal_core::Error::io("."))?;
        let unit = entry::unit_named(store.conn(), self.unit.as_deref(), &cwd)?;
        let project = entry::project_of_unit(store.conn(), &unit)?;
        let now = Timestamp::now();
        let surveyed = survey::project(store.conn(), &project)?;
        let held = ls::Held::of(
            &lock::live(store.conn(), &project.root, now)?,
            lock::idle_hours(&project.root),
        );
        let mut listed = ls::rows(&surveyed, &processes::Live, &project, &held, now);
        states::settle(store.conn(), &mut listed.units, now);
        let answer = show::detail(store.conn(), listed, &unit)?;
        context::compile(&project, &surveyed);
        output::write(&answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
