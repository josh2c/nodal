//! `nodal shell-init`: print the shell integration for a shell.

use std::process::ExitCode;

use clap::Args;
use nodal_core::runtime::{Shell, init};

/// Arguments of `nodal shell-init`.
#[derive(Debug, Args)]
pub struct ShellInit {
    /// bash, zsh or fish. Defaults to the shell `$SHELL` names.
    #[arg(value_name = "SHELL")]
    pub shell: Option<String>,
}

impl ShellInit {
    /// Print the integration.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::UnknownShell`] when the shell is named and not supported,
    /// or when nothing says which shell this is.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        let shell = match &self.shell {
            Some(name) => Shell::parse(name)?,
            None => Shell::detect()
                .ok_or_else(|| nodal_core::Error::UnknownShell { name: String::new() })?,
        };
        let binary = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("nodal"));
        print!("{}", init::script(shell, &binary));
        Ok(ExitCode::SUCCESS)
    }
}
