//! `nodal run`: run a command in the unit's environment and record that it ran.
//!
//! `--tether` starts the command in a process group of its own and writes that group
//! into the registry. The group then belongs to the unit and is stopped when the unit
//! is reclaimed, whatever became of this process ([`nodal_core::runtime::run`]).

use std::process::ExitCode;

use clap::Args;
use nodal_core::Error;
use nodal_core::env::files;
use nodal_core::model::Timestamp;
use nodal_core::runtime::run::Mode;
use nodal_core::runtime::{lock, run, sessions};
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

    /// Take the write lock from the actor who holds it, and record the hand-off.
    #[arg(long)]
    pub take: bool,
}

impl Run {
    /// Take the write on the home, run the command, then bring the sessions up to date.
    ///
    /// The lock is taken before the command starts. A run refused the home must not
    /// have run anything.
    ///
    /// # Errors
    ///
    /// Propagates a directory that is not a unit home, a command that cannot start, and
    /// [`nodal_core::Error::UnitLocked`] when another actor holds the unit.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let cwd = std::env::current_dir().map_err(Error::io("."))?;
        let home = files::find_home(&cwd)?;
        lock::claim(store.conn(), &home, self.take, Timestamp::now())?;
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
