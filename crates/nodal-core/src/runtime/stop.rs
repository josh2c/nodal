//! Stopping what a unit is running: ask first, insist afterwards, and say what is left.
//!
//! Reclaiming a unit removes its home. A process still standing in that directory, or
//! still holding one of its ports, is a process that will fail in a way nobody can read
//! — a build that cannot find its own source tree, a server bound to a port the next
//! unit has since been granted. So the runtime attributed to a unit ([`super::ps`]) is
//! stopped before the home is moved.
//!
//! Two signals, in the order a person would use them. `SIGTERM` first, because a
//! development server that is asked to stop flushes what it was writing and removes its
//! own socket. Then a wait, and `SIGKILL` for whatever is still there. A process that
//! survives both is reported rather than waited on forever: the report is the point,
//! and an operation that hangs because one process ignores signals has failed a person
//! more thoroughly than one that says which process it could not stop.
//!
//! **Two processes are never signalled: this one, and the one that started it.** A
//! `nodal reclaim` run from inside the unit's own home is attributed to that unit,
//! because it carries `NODAL_ID` like everything else there — and killing the shell
//! somebody typed the command into, in the middle of the command, is not a thing to do.
//! Their working directory stops existing, which they can see; their session does not.
//!
//! [`Signals`] is the seam, so a test can watch what would have been sent without a
//! machine having to lose a process.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long a process is given to stop after it is asked, before it is made to.
pub const GRACE: Duration = Duration::from_secs(5);

/// How often the wait looks to see whether a process has gone.
pub const POLL: Duration = Duration::from_millis(50);

/// One of the two signals this module sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// Ask the process to stop, which it may handle.
    Terminate,
    /// Make it stop, which it may not.
    Kill,
}

/// Where a signal goes. One implementation for a machine, one for a test.
pub trait Signals {
    /// Send `signal` to `pid`. `false` when there is no such process, or when this
    /// account may not signal it.
    fn send(&self, pid: u32, signal: Signal) -> bool;

    /// Whether a process with this identifier is still there.
    fn alive(&self, pid: u32) -> bool;

    /// Wait, so that a test can answer at once rather than sleep.
    fn wait(&self, period: Duration) {
        std::thread::sleep(period);
    }
}

/// The processes of this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Live;

/// What one stop did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stopped {
    /// Processes that stopped when they were asked.
    pub asked: Vec<u32>,
    /// Processes that had to be made to stop.
    pub killed: Vec<u32>,
    /// Processes that were still there after both signals.
    pub left: Vec<u32>,
    /// Processes that were never signalled because they are this operation's own.
    pub spared: Vec<u32>,
}

impl Stopped {
    /// How many processes are no longer running because of this.
    #[must_use]
    pub fn count(&self) -> usize {
        self.asked.len() + self.killed.len()
    }

    /// Whether anything is still running.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.left.is_empty()
    }
}

/// Stop every process in `pids`, and report what became of each.
///
/// Idempotent, which is what a step needs: a second call finds every process gone and
/// reports nothing asked, nothing killed and nothing left.
#[must_use]
pub fn processes(signals: &dyn Signals, pids: &[u32], grace: Duration) -> Stopped {
    let mut stopped = Stopped::default();
    let spared = spared();
    let mut asked = Vec::new();
    for pid in pids.iter().copied() {
        if spared.contains(&pid) {
            stopped.spared.push(pid);
            continue;
        }
        if signals.send(pid, Signal::Terminate) {
            asked.push(pid);
        }
    }
    let remaining = settle(signals, &asked, grace);
    stopped.asked.extend(asked.iter().copied().filter(|pid| !remaining.contains(pid)));
    for pid in remaining {
        signals.send(pid, Signal::Kill);
        signals.wait(POLL);
        if signals.alive(pid) {
            stopped.left.push(pid);
        } else {
            stopped.killed.push(pid);
        }
    }
    stopped
}

/// Wait up to `grace` for the asked processes to go, and answer with the ones that did
/// not. Returns as soon as they are all gone rather than sleeping out the whole period.
fn settle(signals: &dyn Signals, asked: &[u32], grace: Duration) -> Vec<u32> {
    let deadline = Instant::now() + grace;
    loop {
        let alive: Vec<u32> = asked.iter().copied().filter(|pid| signals.alive(*pid)).collect();
        if alive.is_empty() || Instant::now() >= deadline {
            return alive;
        }
        signals.wait(POLL);
    }
}

/// The processes this operation will not signal: itself, and whatever started it.
///
/// Public because the verification a reclaim ends with reads the same table and has to
/// leave the same two processes out of it. A shell that is still standing in a home
/// Nodal has just moved is not a leftover; it is the person who asked.
#[must_use]
pub fn spared() -> Vec<u32> {
    vec![std::process::id(), parent_id()]
}

/// The process that started this one. Zero on a platform that will not say, which is
/// not a process identifier and so spares nothing.
#[cfg(unix)]
fn parent_id() -> u32 {
    std::os::unix::process::parent_id()
}

#[cfg(not(unix))]
const fn parent_id() -> u32 {
    0
}

#[cfg(unix)]
impl Signals for Live {
    fn send(&self, pid: u32, signal: Signal) -> bool {
        let Ok(pid) = i32::try_from(pid) else { return false };
        let number = match signal {
            Signal::Terminate => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: `kill` takes two integers and no pointer. A process identifier this
        // account may not signal, or that has already gone, is a return value rather
        // than undefined behaviour.
        unsafe { libc::kill(pid, number) == 0 }
    }

    fn alive(&self, pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else { return false };
        // SAFETY: as above; signal zero delivers nothing and only reports existence.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

/// A platform with no signals stops nothing and says so by leaving every process.
#[cfg(not(unix))]
impl Signals for Live {
    fn send(&self, _pid: u32, _signal: Signal) -> bool {
        false
    }

    fn alive(&self, _pid: u32) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::cell::RefCell;
    use std::time::Duration;

    use super::{Signal, Signals, processes};

    /// A machine whose processes stop when they are asked, unless they are stubborn.
    #[derive(Default)]
    struct Fake {
        alive: RefCell<Vec<u32>>,
        stubborn: Vec<u32>,
        sent: RefCell<Vec<(u32, Signal)>>,
    }

    impl Signals for Fake {
        fn send(&self, pid: u32, signal: Signal) -> bool {
            self.sent.borrow_mut().push((pid, signal));
            if !self.alive.borrow().contains(&pid) {
                return false;
            }
            let obeys = signal == Signal::Kill || !self.stubborn.contains(&pid);
            if obeys {
                self.alive.borrow_mut().retain(|alive| *alive != pid);
            }
            true
        }

        fn alive(&self, pid: u32) -> bool {
            self.alive.borrow().contains(&pid)
        }

        fn wait(&self, _period: Duration) {}
    }

    fn machine(alive: &[u32], stubborn: &[u32]) -> Fake {
        Fake {
            alive: RefCell::new(alive.to_vec()),
            stubborn: stubborn.to_vec(),
            sent: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn a_process_that_stops_when_asked_is_never_killed() {
        let fake = machine(&[41, 42], &[]);
        let stopped = processes(&fake, &[41, 42], Duration::ZERO);
        assert_eq!(stopped.asked, [41, 42]);
        assert!(stopped.killed.is_empty() && stopped.is_clear());
        assert!(fake.sent.borrow().iter().all(|(_, signal)| *signal == Signal::Terminate));
    }

    #[test]
    fn a_process_that_ignores_the_first_signal_gets_the_second() {
        let fake = machine(&[7], &[7]);
        let stopped = processes(&fake, &[7], Duration::ZERO);
        assert_eq!(stopped.killed, [7]);
        assert!(stopped.asked.is_empty() && stopped.is_clear());
        assert_eq!(stopped.count(), 1);
        assert!(fake.sent.borrow().contains(&(7, Signal::Kill)));
    }

    #[test]
    fn stopping_what_has_already_stopped_asks_for_nothing() {
        let fake = machine(&[], &[]);
        let stopped = processes(&fake, &[7], Duration::ZERO);
        assert_eq!(stopped.count(), 0);
        assert!(stopped.is_clear());
    }

    #[test]
    fn this_process_and_the_one_that_started_it_are_left_alone() {
        let mine = std::process::id();
        let fake = machine(&[mine], &[]);
        let stopped = processes(&fake, &[mine], Duration::ZERO);
        assert_eq!(stopped.spared, [mine]);
        assert!(stopped.left.is_empty(), "spared is not the same answer as would not stop");
        assert!(fake.sent.borrow().is_empty(), "nothing was signalled");
        assert!(fake.alive(mine));
    }
}
