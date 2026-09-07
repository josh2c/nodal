//! `nodal gc`: remove the reclaimed homes whose retention has run out.

use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::ops::gc;
use nodal_core::model::Timestamp;
use nodal_core::output::{self, Format};
use nodal_core::store::Store;

/// Arguments of `nodal gc`.
#[derive(Debug, Args)]
pub struct Gc {
    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Gc {
    /// Sweep the trash and print what went.
    ///
    /// # Errors
    ///
    /// Propagates a registry that cannot be read or written. A directory that will not
    /// go is a line of the report rather than a failure, so one home nobody can remove
    /// does not stop the rest of the sweep.
    pub fn run(&self, store: &Store) -> nodal_core::Result<ExitCode> {
        let swept = gc::collect(store, Timestamp::now())?;
        output::write(&swept, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
