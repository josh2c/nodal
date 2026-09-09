//! A process a test starts, and the two shapes a test starts one in.
//!
//! Attribution is what `nodal ps` answers: which unit a running process belongs to, and
//! how sure the answer is. It is asserted against real processes rather than a table a
//! test wrote, so two suites start one and then read the machine. Both wrote the same
//! two shapes and the same guard against a host with no process table.
//!
//! A process a test starts is killed when the test ends, whichever way it ends, because
//! a test that leaves a `sleep` behind leaves one on every run.

use std::path::Path;
use std::process::{Child, Command};

/// A child that is killed when the test ends, whichever way it ends.
pub struct Sleeper(Child);

impl Sleeper {
    /// Start `command` and hold it until this value is dropped.
    ///
    /// # Panics
    ///
    /// If the command could not be started.
    #[must_use]
    pub fn spawn(command: &mut Command) -> Self {
        Self(command.spawn().expect("the process starts"))
    }

    /// Which process it is, which is what a row of `nodal ps` is found by.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

/// A sleeping process carrying a unit's environment, as an activated shell gives it.
///
/// This is the signal attribution calls certain: the process says which unit it is in,
/// in the two variables `nodal shell`, the prompt hook and direnv all set.
///
/// # Panics
///
/// As [`Sleeper::spawn`].
#[must_use]
pub fn carrying(unit: &str, home: &Path) -> Sleeper {
    Sleeper::spawn(Command::new("sleep").arg("30").env("NODAL_ID", unit).env("NODAL_ROOT", home))
}

/// A sleeping process that merely stands in a home and carries no Nodal variable.
///
/// This is the signal attribution calls probable: a terminal with no integration and no
/// direnv, standing in the directory.
///
/// # Panics
///
/// As [`Sleeper::spawn`].
#[must_use]
pub fn standing_in(home: &Path) -> Sleeper {
    Sleeper::spawn(
        Command::new("sleep")
            .arg("30")
            .current_dir(home)
            .env_remove("NODAL_ID")
            .env_remove("NODAL_ROOT"),
    )
}
