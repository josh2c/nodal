//! The clap tree: global options that every command shares, and dispatch.
//!
//! Subcommands from the CLI surface in `docs/contracts.md` are added here by the task
//! that implements each one, one file per command under `commands/`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use nodal_core::lifecycle::{self, Resolution, ops};
use nodal_core::logging::Verbosity;
use nodal_core::store::Store;
use nodal_core::workspace::home;

use crate::commands::base::Base;
use crate::commands::cd::Cd;
use crate::commands::env::Env;
use crate::commands::init::Init;
use crate::commands::new::New;
use crate::commands::run::Run;
use crate::commands::shell::Shell;
use crate::commands::shell_init::ShellInit;

/// The subcommands implemented so far. The rest arrive with their own tasks.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write `nodal.toml` for this project, with a line for every gap.
    Init(Init),
    /// Report what the unit home you are in is activated with.
    Env(Env),
    /// Make a unit: a branch, a home cloned from the project, and the rows for both.
    New(New),
    /// Print the home of a unit, and enter it when the shell function is installed.
    Cd(Cd),
    /// Print the shell integration for bash, zsh or fish.
    ShellInit(ShellInit),
    /// Become a shell that carries a unit's environment.
    Shell(Shell),
    /// Run a command in a unit's environment and record it in the unit's log.
    Run(Run),
    /// List, build and collect the warm bases unit homes are cloned from.
    #[command(subcommand_required = true, arg_required_else_help = true)]
    Base(Base),
}

/// One list for every coding agent on your project.
#[derive(Debug, Parser)]
#[command(name = "nodal", version, about, long_about = None)]
pub struct Cli {
    /// Path to the registry. Defaults to `registry.db` in Nodal's state directory.
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
            Some(Command::Env(env)) => env.run(),
            Some(Command::New(new)) => new.run(&mut self.registry()?),
            Some(Command::Cd(cd)) => cd.run(&self.registry()?),
            Some(Command::Run(run)) => run.run(&self.registry()?),
            Some(Command::ShellInit(init)) => init.run(),
            Some(Command::Shell(shell)) => shell.run(),
            Some(Command::Base(base)) => base.run(&mut self.registry()?),
            None => {
                let (store, no_hooks) = (&self.store, self.no_hooks);
                tracing::debug!(?store, no_hooks, "no subcommand given");
                // A bare invocation prints the surface it has, rather than nothing.
                Self::command().print_help().map_err(nodal_core::Error::io("<stdout>"))?;
                Ok(ExitCode::SUCCESS)
            }
        }
    }

    /// The registry this invocation works on, with every interrupted operation dealt
    /// with before the command runs.
    ///
    /// This is the preamble: a `nodal` killed between two steps left work behind, and
    /// the next `nodal` is what finishes or undoes it. What was done is printed on
    /// standard error rather than standard output, so a command's `--json` answer stays
    /// one document.
    ///
    /// Every command that reads or writes the registry goes through here, and only
    /// those do. `init` writes a file into a project Nodal may never have heard of, and
    /// `env` reads a home's own files, so neither creates a registry as a side effect of
    /// being run.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::NoHomeDirectory`] when no override and no home directory
    /// say where the registry belongs, and whatever opening or reading it reported.
    fn registry(&self) -> nodal_core::Result<Store> {
        let path = match &self.store {
            Some(chosen) => chosen.clone(),
            None => home::registry()?,
        };
        let mut store = Store::open(path)?;
        report(&lifecycle::resolve(&mut store, &ops::rebuilders())?);
        Ok(store)
    }
}

/// Say what became of every operation an earlier run did not finish.
fn report(resolutions: &[Resolution]) {
    for resolution in resolutions {
        eprintln!("nodal: {resolution}");
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
