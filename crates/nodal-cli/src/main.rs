//! The `nodal` binary: parse arguments, dispatch, print. No behaviour lives here.

mod cli;
mod commands;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("nodal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> nodal_core::Result<ExitCode> {
    nodal_core::logging::init(cli.verbosity())?;
    cli.dispatch()
}
