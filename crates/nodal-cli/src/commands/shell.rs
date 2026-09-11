//! `nodal shell`: become a shell that carries the unit's environment.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::env::files;
use nodal_core::model::Timestamp;
use nodal_core::runtime::{lock, shell};
use nodal_core::store::Store;

/// Arguments of `nodal shell`.
#[derive(Debug, Args)]
pub struct Shell {
    /// A directory in the unit's home. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Take the write lock from the actor who holds it, and record the hand-off.
    #[arg(long)]
    pub take: bool,
}

impl Shell {
    /// Find the home, take the write on it, and replace this process with the shell.
    ///
    /// The lock is taken before the process is replaced, because after the replacement
    /// there is nothing left of this command to report with.
    ///
    /// # Errors
    ///
    /// Propagates a directory that is not a unit home, a shell that cannot start, and
    /// [`nodal_core::Error::UnitLocked`] when another actor holds the unit.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let start = self.path.clone().unwrap_or_else(|| PathBuf::from("."));
        let home = files::find_home(&start)?;
        lock::claim(store.conn(), &home, self.take, Timestamp::now())?;
        let entry = shell::plan(&home)?;
        shell::enter(&entry).map(|never| match never {})
    }
}
