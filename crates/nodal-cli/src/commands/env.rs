//! `nodal env`: what the home you are in is activated with.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::env::files;
use nodal_core::model::Timestamp;
use nodal_core::output::view::EnvReport;
use nodal_core::output::{self, Format};
use nodal_core::runtime::{Shell, lock, shells};
use nodal_core::store::Store;

/// Arguments of `nodal env`.
#[derive(Debug, Args)]
pub struct Env {
    /// A directory in the unit's home. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the assignments for a shell to evaluate.
    #[arg(long)]
    pub export: bool,

    /// Which shell the assignments are for. Defaults to bash and zsh.
    #[arg(long, value_name = "SHELL", requires = "export")]
    pub shell: Option<String>,

    /// Print the report as JSON. Names and origins only, never a value.
    #[arg(long, conflicts_with = "export")]
    pub json: bool,
}

impl Env {
    /// Find the home, then report it or print its variables.
    ///
    /// An export refreshes the unit's write lock, because the export is what the prompt
    /// hook runs when a shell enters a home and is therefore the signal that somebody is
    /// still here. It refuses nobody. A home another actor holds still exports its
    /// variables and its ports: the lock informs a second actor and does not stop them,
    /// and a shell with no environment would be stopping them.
    ///
    /// A machine with no registry exports just as it did. The lock is a record on a
    /// registry, and `nodal env` has never needed one.
    ///
    /// # Errors
    ///
    /// Propagates a directory that is not a unit home, and files that cannot be read.
    pub fn run(&self, store: Option<&Store>) -> nodal_core::Result<ExitCode> {
        let start = self.path.clone().unwrap_or_else(|| PathBuf::from("."));
        let home = files::find_home(&start)?;
        if self.export {
            if let Some(store) = store {
                lock::touch(store.conn(), &home, Timestamp::now())?;
            }
            let shell = match &self.shell {
                Some(name) => Shell::parse(name)?,
                None => Shell::default(),
            };
            print!("{}", shells::assignments(shell, &nodal_core::env::entering(&home)?));
            return Ok(ExitCode::SUCCESS);
        }
        let report = EnvReport::from_manifest(&files::read_manifest(&home)?, Timestamp::now());
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
