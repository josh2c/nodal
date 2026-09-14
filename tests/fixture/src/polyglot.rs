//! A project of three ecosystems: a Rust binary, a Node CLI and a Python tool.
//!
//! The shape the three-day proof is run against, and the shape the single-manager
//! inference was wrong about. A repository like this one carries three lockfiles, and a
//! recipe that named one of them described a third of the tree: the base installed the
//! Node dependencies and left the Rust and Python halves cold.
//!
//! Every file here is evidence some source reads. The Cargo manifest and its lock name
//! the Rust half and its minimum toolchain; `clippy.toml` is what makes `cargo clippy`
//! a command this project states rather than one Nodal assumed. The `pyproject.toml`
//! declares `pytest` among its dependencies and configures `ruff`, which is the only
//! reason a Python test and lint command are proposed at all. `go.mod` is there for its
//! one directive, because a Go module states its language version nowhere else.
//!
//! It is not built by CI. The pnpm fixture beside it is the one that installs and
//! builds; this one exists to be read.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built fails the test it was built for"
)]

use std::path::{Path, PathBuf};

/// One file of the fixture: where it goes, and what is in it.
type File = (&'static str, &'static str);

/// Every file, in the order they are written.
const FILES: &[File] = &[
    // The Rust binary.
    ("Cargo.toml", CARGO_TOML),
    ("Cargo.lock", CARGO_LOCK),
    ("rust-toolchain.toml", RUST_TOOLCHAIN),
    ("clippy.toml", CLIPPY_TOML),
    ("src/main.rs", MAIN_RS),
    // The Node CLI.
    ("package.json", PACKAGE_JSON),
    ("pnpm-lock.yaml", PNPM_LOCK),
    ("cli/index.mjs", CLI_INDEX),
    // The Python tool.
    ("pyproject.toml", PYPROJECT_TOML),
    ("uv.lock", UV_LOCK),
    ("tool/__init__.py", TOOL_INIT),
    // The Go module, which is here for its one directive.
    ("go.mod", GO_MOD),
];

/// The Rust half. `rust-version` is the minimum the crate builds with.
const CARGO_TOML: &str = "\
[package]
name = \"polyglot-bin\"
version = \"0.1.0\"
edition = \"2021\"
rust-version = \"1.88\"
";

/// The lockfile is what names Cargo as a manager. Its contents are never read.
const CARGO_LOCK: &str =
    "version = 3\n\n[[package]]\nname = \"polyglot-bin\"\nversion = \"0.1.0\"\n";

/// The channel `rustup` selects, in the table form, which is the form that cannot be
/// read as a plain pin file.
const RUST_TOOLCHAIN: &str = "[toolchain]\nchannel = \"1.88.0\"\n";

/// The file that states this project lints with Clippy. Without it there is no `lint`
/// command for the Rust half, because `cargo clippy` is a component a toolchain may
/// not have and inference never asks the host.
const CLIPPY_TOML: &str = "cognitive-complexity-threshold = 12\n";

const MAIN_RS: &str = "fn main() {\n    println!(\"polyglot\");\n}\n";

/// The Node half: a pinned manager, and the scripts the Node commands are taken from.
/// It declares no `build`, so the `build` command comes from the Rust half.
const PACKAGE_JSON: &str = r#"{
  "name": "polyglot-cli",
  "private": true,
  "packageManager": "pnpm@9.12.3",
  "engines": { "node": "22.11.0" },
  "scripts": {
    "dev": "node cli/index.mjs",
    "lint": "node --check cli/index.mjs"
  }
}
"#;

const PNPM_LOCK: &str = "lockfileVersion: '9.0'\n";

const CLI_INDEX: &str = "console.log(\"polyglot\");\n";

/// The Python half. `requires-python` answers the toolchain, `pytest` in the dependency
/// group is what makes a test command readable, and `[tool.ruff]` is what makes a lint
/// command readable. Neither is assumed from the language.
const PYPROJECT_TOML: &str = "\
[project]
name = \"polyglot-tool\"
version = \"0.1.0\"
requires-python = \">=3.12\"
dependencies = []

[dependency-groups]
dev = [\"pytest>=8.0\"]

[tool.ruff]
line-length = 100
";

const UV_LOCK: &str = "version = 1\nrequires-python = \">=3.12\"\n";

const TOOL_INIT: &str = "def main() -> None:\n    print(\"polyglot\")\n";

const GO_MOD: &str = "module example.com/polyglot\n\ngo 1.23.4\n";

/// The package-manager pin the Node half states.
pub const PACKAGE_MANAGER_PIN: &str = "pnpm@9.12.3";

/// Write the fixture under `root` and return it.
///
/// # Panics
///
/// If a file cannot be written, which means the test cannot run at all.
#[must_use]
pub fn write(root: impl AsRef<Path>) -> PathBuf {
    let root = root.as_ref().to_path_buf();
    for (relative, contents) in FILES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the directory is created");
        }
        std::fs::write(&path, contents).expect("the file is written");
    }
    root
}

/// Every path the fixture writes, relative to its root.
#[must_use]
pub fn paths() -> Vec<&'static str> {
    FILES.iter().map(|(relative, _)| *relative).collect()
}

#[cfg(test)]
mod tests {
    use super::{paths, write};

    #[test]
    fn no_path_is_written_twice() {
        let mut seen = paths();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "a path appears twice in the table");
    }

    #[test]
    fn writing_it_twice_leaves_the_same_project() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let once = write(directory.path());
        let read = || -> Vec<String> {
            paths()
                .iter()
                .map(|p| std::fs::read_to_string(once.join(p)).unwrap_or_default())
                .collect()
        };
        let first = read();
        drop(write(directory.path()));
        assert_eq!(first, read());
    }
}
