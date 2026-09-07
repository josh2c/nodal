//! `nodal doctor`: report what this machine has left behind, and remove nothing.

use std::process::ExitCode;

use clap::Args;
use nodal_core::doctor;
use nodal_core::model::Timestamp;
use nodal_core::output::{self, Format};
use nodal_core::services::docker;
use nodal_core::store::Store;
use nodal_core::workspace::home;

/// Arguments of `nodal doctor`.
#[derive(Debug, Args)]
pub struct Doctor {
    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Doctor {
    /// Read this machine and print what it holds.
    ///
    /// The report writes nothing. It reads the registry, the checkout it was run in,
    /// Nodal's state directory and, where a daemon answers, Docker, and it leaves every
    /// one of them as it found them.
    ///
    /// One thing happens before the report that is not the report. Every command that
    /// opens the registry finishes or rolls back an operation an earlier run was killed
    /// in the middle of ([`crate::cli::Cli`]), and that preamble writes. It says what it
    /// did on standard error, and it is the same preamble `nodal ls` and `nodal ps` run.
    /// Nothing doctor itself does writes anything.
    ///
    /// # Errors
    ///
    /// Propagates a registry that cannot be read, and a state directory that nothing
    /// says the place of. A Docker daemon that is not there is a note in the answer, not
    /// a failure.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let cwd = std::env::current_dir().map_err(nodal_core::Error::io("<cwd>"))?;
        let state_dir = home::directory()?;
        let sessions = doctor::intent::config_directory();
        let machine = doctor::Machine::here(&cwd, &state_dir, sessions.as_deref());
        let answer = doctor::survey(store.conn(), &docker::Cli, &machine, Timestamp::now())?;
        output::write(&answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
