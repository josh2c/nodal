//! `nodal shell`: become a shell that carries the unit's environment.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::env::files;
use nodal_core::runtime::shell;

/// Arguments of `nodal shell`.
#[derive(Debug, Args)]
pub struct Shell {
    /// A directory in the unit's home. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,
}

impl Shell {
    /// Find the home and replace this process with the shell.
    ///
    /// # Errors
    ///
    /// Propagates a directory that is not a unit home, and a shell that cannot start.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        let start = self.path.clone().unwrap_or_else(|| PathBuf::from("."));
        let home = files::find_home(&start)?;
        let entry = shell::plan(&home)?;
        shell::enter(&entry).map(|never| match never {})
    }
}
