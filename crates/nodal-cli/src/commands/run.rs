//! `nodal run`: run a command in the unit's environment and record that it ran.
//!
//! `--tether` starts the command in a process group of its own and writes that group
//! into the registry. The group then belongs to the unit and is stopped when the unit
//! is reclaimed, whatever became of this process ([`nodal_core::runtime::run`]).

use std::process::ExitCode;

use clap::Args;
use nodal_core::Error;
use nodal_core::env::files;
use nodal_core::runtime::run::Mode;
use nodal_core::runtime::{run, sessions};
use nodal_core::store::Store;

use crate::commands::context;

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

    /// Give the command a process group of its own, owned by the unit. `nodal reclaim`
    /// stops the whole group.
    #[arg(long)]
    pub tether: bool,
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
        let ran = run::execute(&home, &cwd, &self.argv, Some(store.conn()), self.mode())?;
        sessions::observe_quietly(store.conn());
        context::refresh_at(store, &home);
        if let Some(pgid) = ran.tether {
            eprintln!("nodal: the tether is still running as process group {pgid}");
        }
        Ok(ExitCode::from(u8::try_from(ran.code.unwrap_or(1)).unwrap_or(1)))
    }

    /// Which of the two ways to start the command these arguments ask for.
    const fn mode(&self) -> Mode {
        if self.tether { Mode::Tether } else { Mode::Plain }
    }
}
