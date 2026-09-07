//! `nodal cd`: print the home a target names, and ask a waiting shell to enter it.

use std::process::ExitCode;

use clap::Args;
use nodal_core::runtime::entry;
use nodal_core::store::Store;

/// Arguments of `nodal cd`.
#[derive(Debug, Args)]
pub struct Cd {
    /// A unit's slug, or a directory. Defaults to the home the shell is in.
    #[arg(value_name = "UNIT")]
    pub target: Option<String>,
}

impl Cd {
    /// Resolve the target, tell the shell function about it, and print it.
    ///
    /// # Errors
    ///
    /// Propagates a target that names no home, and a registry that cannot be read.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let cwd = std::env::current_dir().map_err(nodal_core::Error::io("."))?;
        let home = entry::home(self.target.as_deref(), &cwd, store.conn())?;
        entry::ask_to_enter(&home)?;
        println!("{}", home.display());
        Ok(ExitCode::SUCCESS)
    }
}
