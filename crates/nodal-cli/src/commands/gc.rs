//! `nodal gc`: give back the merged homes whose retention has run out, remove the
//! reclaimed ones, and report what has gone quiet.

use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::ops::gc::{self, Options};
use nodal_core::model::Timestamp;
use nodal_core::output::{self, Format};
use nodal_core::store::Store;

/// How many days of quiet make a live unit worth reporting, when `--idle` is given no
/// number of its own.
const DEFAULT_IDLE_DAYS: &str = "7";

/// Arguments of `nodal gc`.
#[derive(Debug, Args)]
pub struct Gc {
    /// Also report the live units nothing has touched for this many days. Nothing of
    /// theirs is stopped.
    #[arg(long, value_name = "DAYS", num_args = 0..=1, default_missing_value = DEFAULT_IDLE_DAYS)]
    pub idle: Option<u32>,

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
    /// go, and a merged unit the uniqueness check refuses, are lines of the report
    /// rather than failures, so one home nobody can remove does not stop the sweep.
    pub fn run(&self, store: &mut Store, hooks: bool) -> nodal_core::Result<ExitCode> {
        let options = Options { idle: self.idle, hooks };
        let swept = gc::collect(store, Timestamp::now(), &options)?;
        output::write(&swept, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
