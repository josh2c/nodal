//! `nodal run`: run a command in the unit's environment and record that it ran.

use std::process::ExitCode;

use clap::Args;
use nodal_core::Error;
use nodal_core::env::files;
use nodal_core::runtime::{run, sessions};
use nodal_core::store::Store;

/// Arguments of `nodal run`.
#[derive(Debug, Args)]
pub struct Run {
    /// The command and its arguments.
    #[arg(
        value_name = "COMMAND",
        required = true,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub argv: Vec<String>,
}

impl Run {
    /// Run the command, then bring the unit's sessions up to date.
    ///
    /// # Errors
    ///
    /// Propagates a directory that is not a unit home, and a command that cannot start.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let cwd = std::env::current_dir().map_err(Error::io("."))?;
        let home = files::find_home(&cwd)?;
        let ran = run::execute(&home, &cwd, &self.argv, Some(store.conn()))?;
        sessions::observe_quietly(store.conn());
        Ok(ExitCode::from(u8::try_from(ran.code.unwrap_or(1)).unwrap_or(1)))
    }
}
