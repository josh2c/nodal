//! `nodal shell-init`: print the shell integration for a shell, or install it.

use std::process::ExitCode;

use clap::Args;
use nodal_core::output::{self, Format};
use nodal_core::runtime::{Shell, init};
use nodal_core::setup::plan;
use nodal_core::workspace::home;

/// Arguments of `nodal shell-init`.
#[derive(Debug, Args)]
pub struct ShellInit {
    /// bash, zsh or fish. Defaults to the shell `$SHELL` names.
    #[arg(value_name = "SHELL")]
    pub shell: Option<String>,

    /// Write the script into Nodal's state directory and load it from the shell's
    /// start-up file. `nodal uninstall` removes both again.
    #[arg(long)]
    pub install: bool,

    /// Print the result of `--install` as JSON.
    #[arg(long, requires = "install")]
    pub json: bool,
}

impl ShellInit {
    /// Print the integration, or install it.
    ///
    /// Printing is the default and is unchanged: the text goes to standard output, and
    /// `eval "$(nodal shell-init bash)"` in a start-up file still works. `--install`
    /// writes that same text to a file and puts one line in the start-up file that
    /// sources it. The line evaluates nothing and starts no process, which is what a
    /// hook that runs in every shell a person opens has to be able to say.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::UnknownShell`] when the shell is named and not supported,
    /// or when nothing says which shell this is;
    /// [`nodal_core::Error::NoHomeDirectory`] when nothing says where the state
    /// directory or the start-up file is; and whatever writing either reported.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        let shell = match &self.shell {
            Some(name) => Shell::parse(name)?,
            None => Shell::detect()
                .ok_or_else(|| nodal_core::Error::UnknownShell { name: String::new() })?,
        };
        let binary = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("nodal"));
        if !self.install {
            print!("{}", init::script(shell, &binary));
            return Ok(ExitCode::SUCCESS);
        }
        let done = plan::install(&home::directory()?, &home::user()?, shell, &binary)?;
        output::write(&done, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
