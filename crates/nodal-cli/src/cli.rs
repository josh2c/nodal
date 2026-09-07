//! The clap tree: global options that every command shares, and dispatch.
//!
//! Subcommands from the CLI surface in `docs/contracts.md` are added here by the task
//! that implements each one, one file per command under `commands/`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use nodal_core::logging::Verbosity;

use crate::commands::init::Init;

/// The subcommands implemented so far. The rest arrive with their own tasks.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write `nodal.toml` for this project, with a line for every gap.
    Init(Init),
}

/// One list for every coding agent on your project.
#[derive(Debug, Parser)]
#[command(name = "nodal", version, about, long_about = None)]
pub struct Cli {
    /// Path to the registry, for tests and for a second machine's store.
    #[arg(long, global = true, value_name = "PATH", env = "NODAL_STORE")]
    pub store: Option<PathBuf>,

    /// Skip recipe hooks for this invocation.
    #[arg(long, global = true)]
    pub no_hooks: bool,

    /// Print progress to stderr; repeat for debug traces.
    #[arg(short = 'v', long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// What to do.
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// The logging level these arguments ask for.
    #[must_use]
    pub fn verbosity(&self) -> Verbosity {
        Verbosity::from_occurrences(self.verbose)
    }

    /// Run the parsed command.
    ///
    /// # Errors
    ///
    /// Propagates whatever the invoked command returns from `nodal-core`.
    pub fn dispatch(&self) -> nodal_core::Result<ExitCode> {
        match &self.command {
            Some(Command::Init(init)) => init.run(),
            None => {
                let (store, no_hooks) = (&self.store, self.no_hooks);
                tracing::debug!(?store, no_hooks, "no subcommand given");
                // A bare invocation prints the surface it has, rather than nothing.
                Self::command().print_help().map_err(nodal_core::Error::io("<stdout>"))?;
                Ok(ExitCode::SUCCESS)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn clap_tree_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn verbose_flags_count() {
        use clap::Parser;
        use nodal_core::logging::Verbosity;

        let cli = Cli::try_parse_from(["nodal", "-vv"]).unwrap();
        assert_eq!(cli.verbosity(), Verbosity::Trace);
    }
}
