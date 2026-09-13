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
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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

/// A sleeping process in a process group of its own, carrying no Nodal variable and
/// standing nowhere a unit owns.
///
/// This is the bystander a stop must never reach. Nothing recorded it, nothing
/// attributes it, and its group identifier is one no registry row holds — so a teardown
/// that signalled it would be signalling by proximity rather than by record.
///
/// # Panics
///
/// As [`Sleeper::spawn`].
#[must_use]
pub fn in_a_group_of_its_own() -> Sleeper {
    let mut command = Command::new("sleep");
    command.arg("30").env_remove("NODAL_ID").env_remove("NODAL_ROOT");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    Sleeper::spawn(&mut command)
}

/// Whether a process is still there.
///
/// `kill -0` on one process id, rather than `/proc` and rather than a process group. A
/// host with no process table still answers this, and `kill` is asked about a plain
/// positive number, which every implementation of it reads the same way. A negative
/// argument does not read the same way everywhere, so nothing here passes one.
///
/// # Panics
///
/// If `kill` could not be run at all, which is a host no property here can be asserted
/// on.
#[must_use]
pub fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .expect("kill runs")
        .success()
}

/// How long [`wait_for`] gives something to happen.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How often [`wait_for`] looks.
const POLL: Duration = Duration::from_millis(10);

/// Wait for something to become true, and insist that it does.
///
/// A signal is delivered rather than applied, so "the process is gone" is a claim about
/// a moment shortly after the command returned and not about the instant it did.
///
/// # Panics
///
/// If it has not happened within [`TIMEOUT`], naming what did not happen.
pub fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("{what} did not happen within {TIMEOUT:?}");
}
