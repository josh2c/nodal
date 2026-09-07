//! `nodal ps`: what is running on this machine, and which unit it belongs to.

use std::process::ExitCode;

use clap::Args;
use nodal_core::model::Timestamp;
use nodal_core::output::{self, Format};
use nodal_core::runtime::{processes, ps};
use nodal_core::services::docker;
use nodal_core::store::Store;

/// Arguments of `nodal ps`.
#[derive(Debug, Args)]
pub struct Ps {
    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Ps {
    /// Read every signal this host has and print what each one attributed.
    ///
    /// # Errors
    ///
    /// Propagates a registry that cannot be read. A signal that cannot run is a note in
    /// the answer, not a failure: a host with no Docker daemon still reports its
    /// processes and its ports.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let host = nodal_core::lifecycle::owner::current_host();
        let answer =
            ps::observe(store.conn(), &processes::Live, &docker::Cli, &host, Timestamp::now())?;
        output::write(&answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
