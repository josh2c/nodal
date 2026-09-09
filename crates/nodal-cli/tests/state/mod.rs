//! Where a test's commands keep their state: never where the person running them keeps
//! theirs.
//!
//! Nodal's state directory is `~/.nodal` unless `NODAL_HOME` says otherwise, and the
//! first command that opens the registry makes the directory and the file. A test that
//! built a command for the binary without naming a state directory therefore ran against
//! the registry of whoever was running the tests. Two things came of that, and both are
//! why this module exists: a test wrote rows into a person's own record of their own
//! units, and a build older than the migrations in that file refused to start at all, so
//! four suites failed on a workstation and passed on every clean runner.
//!
//! So this is the one place in this crate's tests where the binary is named. What is
//! done with the name is the test kit's [`nodal_safety::runner`], which every suite in
//! the workspace builds its commands with; [`Machine`] is a state directory for a test
//! that had no other reason to make one.
//!
//! `tests/state_directory.rs` is what keeps this true. It proves both halves — a command
//! built the old way writes into the home directory it is given, and a command from here
//! does not — and it refuses a second place in these tests that names the binary.

#![allow(dead_code, reason = "each test file uses the part of the harness it needs")]
#![allow(clippy::unwrap_used, reason = "a harness that cannot be built fails the test")]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// The binary under test.
///
/// For the two tests that put the path in a shell script rather than spawning it. A
/// script spawned that way still needs the state directory: give the `sh` command
/// [`Machine::path`] as `NODAL_HOME`, which is what [`nodal`] does for a direct spawn.
pub const BINARY: &str = env!("CARGO_BIN_EXE_nodal");

/// A command for the binary, with `state` as its state directory and the two files
/// every unit on a machine shares moved into it.
pub fn nodal(state: &Path) -> Command {
    nodal_safety::runner::isolated(BINARY, state)
}

/// A state directory of a test's own, removed when the test ends.
///
/// For a test about something other than where state goes: `nodal --version`, the
/// logging switches, `nodal init` on a project in a temporary directory. Those have no
/// state of their own to keep, which is exactly how they came to keep it in the person's.
pub struct Machine {
    /// The directory, kept so that it outlives the test.
    directory: TempDir,
}

impl Machine {
    /// A state directory nothing else on this machine shares.
    #[must_use]
    pub fn new() -> Self {
        Self { directory: TempDir::new().unwrap() }
    }

    /// Where it is.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    /// The same as [`nodal`], for this machine's state directory.
    #[must_use]
    pub fn nodal(&self) -> Command {
        nodal(self.path())
    }

    /// The registry file this machine's commands write, whether or not it is there yet.
    #[must_use]
    pub fn registry(&self) -> PathBuf {
        self.path().join("registry.db")
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}
