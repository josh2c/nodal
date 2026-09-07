//! `nodal-fixture [--shapes] <directory>`: write a fixture so CI can build or read it.
//!
//! The same generators the Rust tests use, with a directory instead of a temporary one,
//! so what CI installs, builds and compares against is exactly what the tests assert on.
//!
//! Two fixtures. Without a flag, the project: a pnpm monorepo with no recipe gaps.
//! With `--shapes`, a Git repository whose branches are the shapes real work is in,
//! which is what the list is read against. It prints the repository path, then one
//! `name<tab>verdict` line per branch, so a comparison reads the expectation from the
//! fixture rather than keeping a second copy of it.

use std::process::ExitCode;

/// How to call this program.
const USAGE: &str = "usage: nodal-fixture [--shapes] <directory>";

/// Write the fixture the arguments name into the directory they name.
fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (shapes, rest) = match arguments.split_first() {
        Some((first, rest)) if first == "--shapes" => (true, rest),
        _ => (false, arguments.as_slice()),
    };
    let [root] = rest else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    if shapes {
        println!("{}", nodal_fixture::shapes::origin(root).display());
        for branch in nodal_fixture::shapes::BRANCHES {
            println!("{}\t{}", branch.name, branch.verdict);
        }
        return ExitCode::SUCCESS;
    }
    match nodal_fixture::try_write(root) {
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
