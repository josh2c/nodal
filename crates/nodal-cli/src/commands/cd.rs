//! `nodal cd`: print the home a target names, and ask a waiting shell to enter it.

use std::process::ExitCode;

use clap::Args;
use nodal_core::model::Timestamp;
use nodal_core::runtime::{entry, lock};
use nodal_core::store::Store;

/// Arguments of `nodal cd`.
#[derive(Debug, Args)]
pub struct Cd {
    /// A unit's slug, or a directory. Defaults to the home the shell is in.
    #[arg(value_name = "UNIT")]
    pub target: Option<String>,

    /// Take the write lock from the actor who holds it, and record the hand-off.
    #[arg(long)]
    pub take: bool,
}

impl Cd {
    /// Resolve the target, take the write on it, and print it.
    ///
    /// The lock is taken before the shell is told anything. A person refused the home
    /// must not be moved into it first and told afterwards.
    ///
    /// # Errors
    ///
    /// Propagates a target that names no home, a registry that cannot be read, and
    /// [`nodal_core::Error::UnitLocked`] when another actor holds the unit.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let cwd = std::env::current_dir().map_err(nodal_core::Error::io("."))?;
        let home = entry::home(self.target.as_deref(), &cwd, store.conn())?;
        lock::claim(store.conn(), &home, self.take, Timestamp::now())?;
        entry::ask_to_enter(&home)?;
        println!("{}", home.display());
        Ok(ExitCode::SUCCESS)
    }
}
