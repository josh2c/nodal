//! `nodal upgrade` and `nodal update`: how to get a newer Nodal.

use std::process::ExitCode;

use clap::Args;
use nodal_core::output::view::Upgrade as Report;
use nodal_core::output::{self, Format};
use nodal_core::setup::channel;

/// Arguments of `nodal upgrade`.
#[derive(Debug, Args)]
pub struct Upgrade {
    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Upgrade {
    /// Print where this binary came from and the one command that upgrades it.
    ///
    /// Nodal has no self-updater and makes no network call of its own (DL-034). This
    /// command reads the path of the running executable and one local file. It fetches
    /// nothing, compares no version, and runs nothing.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::Render`] when the answer cannot be encoded as JSON.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        let report =
            Report { version: String::from(env!("CARGO_PKG_VERSION")), install: channel::read() };
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
