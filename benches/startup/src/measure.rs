//! Spawn the binary and time each spawn.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::stats::Sorted;

/// Run the binary once and return how long the whole spawn took.
///
/// The timer starts before the fork and stops after the child exits, so the number
/// includes the parent's fork and exec cost. It is therefore an upper bound on process
/// startup, and it is the same method the earlier startup measurement used, so the two
/// compare.
fn once(binary: &Path, arguments: &[OsString]) -> io::Result<Duration> {
    let started = Instant::now();
    let status = Command::new(binary)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    let elapsed = started.elapsed();
    if !status.success() {
        return Err(io::Error::other(format!(
            "the binary exited with {status}, so its startup was not measured"
        )));
    }
    Ok(elapsed)
}

/// Run the binary `warmup` times without recording, then `runs` times with recording.
///
/// The warm-up runs pay the page-cache cost of the first read of the binary. They are
/// discarded because the gate is about the startup of a binary a shell keeps calling.
pub fn set(
    binary: &Path,
    arguments: &[OsString],
    warmup: usize,
    runs: usize,
) -> io::Result<Sorted> {
    for _ in 0..warmup {
        once(binary, arguments)?;
    }
    let mut timings = Vec::with_capacity(runs);
    for _ in 0..runs {
        timings.push(once(binary, arguments)?);
    }
    Ok(Sorted::new(timings))
}
