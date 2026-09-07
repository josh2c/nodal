//! `nodal-fixture <directory>`: write the fixture project so CI can build it.
//!
//! The same generator the Rust tests use, with a directory instead of a temporary one,
//! so what CI installs and builds is exactly what inference is asserted against.

use std::process::ExitCode;

/// Write the fixture into the directory named on the command line.
fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(root) = arguments.next() else {
        eprintln!("usage: nodal-fixture <directory>");
        return ExitCode::FAILURE;
    };
    if arguments.next().is_some() {
        eprintln!("usage: nodal-fixture <directory>");
        return ExitCode::FAILURE;
    }
    match nodal_fixture::try_write(&root) {
        Ok(root) => {
            println!("{}", root.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
