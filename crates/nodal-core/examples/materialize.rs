//! Clone one tree into another with the backend this machine selects.
//!
//! Usage: `cargo run -p nodal-core --example materialize -- <source> <destination>`
//!
//! The clone applies the default exclusion list. Every path after the destination is
//! added to it, which is what a recipe's `base.exclude` does. The example prints one
//! JSON object: which backend ran, how long the clone took, and what it holds.
//!
//! `nodal new` will do this as one step of an operation. Until that command exists,
//! this is how the backends are run outside a test, and it is what
//! `ci/acceptance-materialize.sh` measures.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use nodal_core::workspace::sharing::Sharing;
use nodal_core::workspace::{Excludes, Report, home, select_backend};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [source, destination, excludes @ ..] = arguments.as_slice() else {
        eprintln!("usage: materialize <source> <destination> [excluded path ...]");
        return ExitCode::FAILURE;
    };
    let recipe: Vec<PathBuf> = excludes.iter().map(PathBuf::from).collect();
    match clone(Path::new(source), Path::new(destination), &recipe) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("materialize: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Clone the tree and render what happened as one JSON object.
///
/// The backend comes from the answer recorded for the state root, exactly as it does
/// for `nodal new`. The example asks no filesystem anything: a probe writes a file, and
/// this example is run over a person's own checkout. `NODAL_HOME` is what points the
/// state root at the filesystem being measured.
fn clone(source: &Path, destination: &Path, recipe: &[PathBuf]) -> nodal_core::Result<String> {
    let backend = select_backend(&Sharing::ensure(&home::directory()?));
    let exclude = Excludes::with_recipe(recipe);
    let started = Instant::now();
    let report = backend.clone_tree(source, destination, &exclude)?;
    let elapsed = started.elapsed();
    Ok(render(backend.name(), elapsed.as_secs_f64(), &report))
}

/// One line of JSON: the backend, the seconds it took, and the report.
fn render(backend: &str, seconds: f64, report: &Report) -> String {
    let report = serde_json::to_string(report).unwrap_or_else(|_| String::from("{}"));
    format!("{{\"backend\":\"{backend}\",\"seconds\":{seconds:.4},\"report\":{report}}}")
}
