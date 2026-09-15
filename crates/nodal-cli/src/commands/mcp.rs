//! `nodal mcp`: answer an agent's tool calls on standard input and standard output.

use std::process::ExitCode;

use clap::Args;

use crate::cli::Cli;

/// Arguments of `nodal mcp`.
///
/// There are none. The transport is stdio, the directory is the one the server was
/// started in, and both are what the client's configuration decides.
#[derive(Debug, Args)]
pub struct Mcp {
    /// Print the tool listing and exit, which is what `tools/list` answers.
    ///
    /// The committed copy of this document is what `ci/schema-diff.sh` compares against,
    /// so a tool whose arguments changed cannot reach a release without the change being
    /// reviewed.
    #[arg(long)]
    pub tools: bool,
}

impl Mcp {
    /// Answer requests until standard input ends.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::Io`] when a line could not be read or an answer written.
    pub fn run(&self, cli: &Cli) -> nodal_core::Result<ExitCode> {
        if self.tools {
            crate::commands::emit(&crate::mcp::tool_listing())?;
            return Ok(ExitCode::SUCCESS);
        }
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        crate::mcp::serve(cli, &mut input, &mut output)?;
        Ok(ExitCode::SUCCESS)
    }
}
