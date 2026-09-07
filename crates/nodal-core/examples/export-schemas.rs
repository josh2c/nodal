//! Write the JSON schema set to disk.
//!
//! Usage: `cargo run -p nodal-core --example export-schemas [directory]`
//! (the directory defaults to `schemas/`, relative to the repository root).
//!
//! The generation itself is `nodal_core::model::schema`, which has no IO; this example
//! is the one place that writes the files, and `ci/schema-diff.sh` runs it and fails if
//! the working tree then differs from what is committed.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nodal_core::Error;
use nodal_core::model::schema;

fn main() -> ExitCode {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| String::from("schemas")));
    match export(&root) {
        Ok(count) => {
            println!("wrote {count} schemas to {}", root.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("export-schemas: {error}");
            ExitCode::FAILURE
        }
    }
}

fn export(root: &Path) -> nodal_core::Result<usize> {
    let documents = schema::documents();
    for document in &documents {
        let path = root.join(document.path());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        std::fs::write(&path, document.render()).map_err(Error::io(&path))?;
    }
    Ok(documents.len())
}
