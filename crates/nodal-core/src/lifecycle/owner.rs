//! Whose run an operation is, and whether that run is still going.
//!
//! An unfinished operation in the journal means one of three things: another `nodal`
//! on this machine is running it right now, a `nodal` on another machine is, or the
//! process that was running it is gone and the work is left half done. Only the third
//! is ours to clean up, and telling it apart from the first is what this file is for.

use crate::model::HostName;

/// The name used when the machine will not say what it is called. A registry that only
/// ever sees one host still needs the column filled in.
const UNKNOWN_HOST: &str = "localhost";

/// The longest host name accepted from the operating system, including the terminator.
const HOST_NAME_MAX: usize = 256;

/// The process that started an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    /// The machine it ran on. Process identifiers only mean anything on their own host.
    pub host: HostName,
    /// Its process identifier on that host.
    pub pid: u32,
}

impl Owner {
    /// This process.
    #[must_use]
    pub fn current() -> Self {
        Self { host: current_host(), pid: std::process::id() }
    }

    /// What this owner is to the process asking.
    #[must_use]
    pub fn state(&self, asking: &Self) -> Liveness {
        if self.host != asking.host {
            return Liveness::Elsewhere;
        }
        if self.pid == asking.pid || is_running(self.pid) {
            return Liveness::Running;
        }
        Liveness::Gone
    }
}

/// What became of the process that started an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Still there. Its operation is in flight and must be left alone.
    Running,
    /// On another machine, where this process can neither see it nor undo its work.
    Elsewhere,
    /// Gone. Whatever it left half done is ours to resolve.
    Gone,
}

/// What this machine is called, or [`UNKNOWN_HOST`] when it will not say.
///
/// A name the model rejects — one that is not visible ASCII — is treated the same as no
/// name at all. The value is a label in a report and a guard against acting on another
/// machine's rows, not something to fail an operation over.
#[must_use]
pub fn current_host() -> HostName {
    let fallback =
        || HostName::parse(UNKNOWN_HOST).unwrap_or_else(|_| unreachable!("localhost is a token"));
    read_host_name().and_then(|name| HostName::parse(name).ok()).unwrap_or_else(fallback)
}

/// The host name as the operating system gives it, if it gives one.
#[cfg(unix)]
fn read_host_name() -> Option<String> {
    let mut buffer = [0_u8; HOST_NAME_MAX];
    // SAFETY: `buffer` is a live array of `HOST_NAME_MAX` bytes and the length passed
    // is one less than that, so the terminator the call writes stays inside it and the
    // last byte, already zero, is never overwritten.
    let code = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len() - 1) };
    if code != 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    let name = core::str::from_utf8(&buffer[..end]).ok()?;
    Some(name.to_owned())
}

/// Windows and anything else: the environment is the only portable source here, and it
/// is allowed to be silent.
#[cfg(not(unix))]
fn read_host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok()
}

/// Whether a process with this identifier exists on this machine.
///
/// Signal zero is the portable existence check: the kernel performs the permission and
/// existence checks and sends nothing. A process that exists but belongs to another
/// user answers `EPERM`, which is still an answer of yes.
///
/// Identifiers are reused, so a very long-dead operation whose number has come round
/// again reads as running and is left alone rather than resolved. Leaving work
/// unresolved is the safe side of that trade, and the operation stays in the report.
#[cfg(unix)]
fn is_running(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else { return false };
    // SAFETY: `kill` with signal zero delivers nothing; it only reports whether the
    // process could be signalled. No pointer is involved.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Without a way to ask, assume the process is still there: reporting an operation
/// that has in fact finished is harmless, and undoing one that is still running is not.
#[cfg(not(unix))]
fn is_running(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{Liveness, Owner, current_host, is_running};
    use crate::model::HostName;

    #[test]
    fn this_process_is_running() {
        assert!(is_running(std::process::id()));
    }

    #[test]
    fn a_host_name_is_always_produced() {
        assert!(!current_host().as_str().is_empty());
    }

    #[test]
    fn another_machines_run_is_never_ours_to_resolve() {
        let here = Owner::current();
        let Ok(elsewhere_host) = HostName::parse("some-other-machine") else {
            unreachable!("a token host name parses")
        };
        let there = Owner { host: elsewhere_host, pid: here.pid };
        assert_eq!(there.state(&here), Liveness::Elsewhere);
    }

    #[test]
    fn our_own_run_reads_as_running() {
        let here = Owner::current();
        assert_eq!(here.state(&here), Liveness::Running);
    }
}
