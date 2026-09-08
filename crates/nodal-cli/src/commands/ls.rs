//! `nodal ls`: every unit of the project, and what each one needs next.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::context::survey::{self, Snapshot};
use nodal_core::lifecycle::states;
use nodal_core::model::{Project, Timestamp};
use nodal_core::output::view::UnitList;
use nodal_core::output::{self, Format};
use nodal_core::runtime::{entry, ls, processes};
use nodal_core::store::Store;

use crate::commands::context;

/// Arguments of `nodal ls`.
#[derive(Debug, Default, Args)]
pub struct Ls {
    /// A directory in the project. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

/// One reading of a project: the survey every home was asked for, and the list built
/// from it.
///
/// The two are kept together because they are one pass. The rows are what the person
/// asked for; the survey is what the memories are compiled from, and compiling from a
/// second reading would ask every home the same questions again.
pub struct Listing {
    /// The project the units belong to.
    pub project: Project,
    /// What every unit's home answered, once.
    pub surveyed: Vec<Snapshot>,
    /// The rows, ordered as the list prints them.
    pub list: UnitList,
}

impl Listing {
    /// Record the units the reading found merged, and show them so.
    ///
    /// The first of the two things the command layer records. It is here rather than in
    /// the reading because `docs/contracts.md` puts it here, and because a reading that
    /// writes is a reading nothing else can safely reuse.
    pub fn settle(&mut self, store: &Store) {
        let notes = states::settle(store.conn(), &mut self.list.units, self.list.now);
        self.list.notes.extend(notes);
    }

    /// Write every unit's memory again, from the survey the rows were built from.
    ///
    /// The second. The list is the command a person types most, so it is the one that
    /// keeps the memories current for the units nobody has touched today.
    pub fn compile(&self) {
        context::compile(&self.project, &self.surveyed);
    }
}

impl Ls {
    /// Print every unit of the project the directory is in.
    ///
    /// Three steps, and the order is the contract: the reading, then the two things the
    /// command layer records from it, then the answer. `runtime::ls` writes nothing, so
    /// both writes are here — the flip to merged, which the reading found, and every
    /// unit's memory, which is compiled from the same survey the rows were built from.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::ProjectNotFound`] when the directory is in no project Nodal
    /// records, and whatever the registry or Git reported.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let path = self.directory()?;
        let mut listing = self.read(store)?.ok_or(nodal_core::Error::ProjectNotFound { path })?;
        listing.settle(store);
        listing.compile();
        self.print(&listing.list)
    }

    /// The reading, or `None` when the directory is in no project Nodal records.
    ///
    /// A bare `nodal` is both the list and the first command a person ever types, so it
    /// falls back to the help rather than to an error. The fallback is the caller's,
    /// because the help belongs to the argument parser.
    ///
    /// # Errors
    ///
    /// Whatever the registry or Git reported.
    pub fn read(&self, store: &Store) -> nodal_core::Result<Option<Listing>> {
        let path = self.directory()?;
        let Some(project) = entry::project_at(store.conn(), &path)? else {
            return Ok(None);
        };
        let now = Timestamp::now();
        let surveyed = survey::project(store.conn(), &project)?;
        let list = ls::rows(&surveyed, &processes::Live, &project, now);
        Ok(Some(Listing { project, surveyed, list }))
    }

    /// Render the list in the format the arguments asked for.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::Render`] when the list cannot be encoded as JSON.
    pub fn print(&self, answer: &UnitList) -> nodal_core::Result<ExitCode> {
        output::write(answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }

    /// The directory the project is read from.
    fn directory(&self) -> nodal_core::Result<PathBuf> {
        match &self.path {
            Some(given) => Ok(given.clone()),
            None => std::env::current_dir().map_err(nodal_core::Error::io(".")),
        }
    }
}
