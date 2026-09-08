//! `nodal ls`: every unit of the project, and what each one needs next.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::model::Timestamp;
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

impl Ls {
    /// Print every unit of the project the directory is in.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::ProjectNotFound`] when the directory is in no project Nodal
    /// records, and whatever the registry or Git reported.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let path = self.directory()?;
        let answer = self.answer(store)?.ok_or(nodal_core::Error::ProjectNotFound { path })?;
        self.refresh(store);
        self.print(&answer)
    }

    /// Write every unit's memory again.
    ///
    /// The list is the command a person types most, so it is the one that keeps the
    /// memories current for the units nobody has touched today. The reading itself
    /// stays a reading: `runtime::ls` writes nothing, and the compile that does happens
    /// here, after the answer is in hand.
    pub fn refresh(&self, store: &Store) {
        let Ok(path) = self.directory() else { return };
        context::refresh_at(store, &path);
    }

    /// The list, or `None` when the directory is in no project Nodal records.
    ///
    /// A bare `nodal` is both the list and the first command a person ever types, so it
    /// falls back to the help rather than to an error. The fallback is the caller's,
    /// because the help belongs to the argument parser.
    ///
    /// # Errors
    ///
    /// Whatever the registry or Git reported.
    pub fn answer(&self, store: &Store) -> nodal_core::Result<Option<UnitList>> {
        let path = self.directory()?;
        let Some(project) = entry::project_at(store.conn(), &path)? else {
            return Ok(None);
        };
        let now = Timestamp::now();
        Ok(Some(ls::list(store.conn(), &processes::Live, &project, now)?))
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
