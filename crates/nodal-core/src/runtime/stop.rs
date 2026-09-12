//! Stopping what a unit is running: ask first, insist afterwards, and say what is left.
//!
//! Reclaiming a unit removes its home. A process still standing in that directory, or
//! still holding one of its ports, is a process that will fail in a way nobody can read
//! — a build that cannot find its own source tree, a server bound to a port the next
//! unit has since been granted. So the runtime attributed to a unit ([`super::ps`]) is
//! stopped before the home is moved.
//!
//! # Two kinds of target
//!
//! A [`Target`] is one process, or one whole process group. The group is the certain
//! one: `nodal run --tether` puts a command in a process group of its own and writes
//! that group into the registry, so the group is a record rather than an inference.
//! Every reclaim stops the groups it has records for before it stops anything a scan
//! merely attributed, and a signal sent to a group reaches every process in it. That is
//! what makes a development server die whole: the server, the compiler it started, and
//! the watcher that compiler started.
//!
//! A group is also the one target this module can read on a host with no process table.
//! `kill` answers for a group whatever the host is, so a tether is stopped, and its
//! survival is reported, on a machine where attribution can see nothing at all.
//!
//! # The ladder
//!
//! Three signals, in the order a person would use them, with a grace period between
//! each pair. `SIGINT` first, because it is what `Ctrl-C` sends and what every
//! development server is written to handle. Then `SIGTERM`, which a process that
//! ignores interrupts still usually honours. Then `SIGKILL` for whatever is left. A
//! target that goes on one rung never sees the next.
//!
//! A process that survives all three is reported rather than waited on forever: the
//! report is the point, and an operation that hangs because one process ignores signals
//! has failed a person more thoroughly than one that says which process it could not
//! stop.
//!
//! # What is never signalled
//!
//! **This process, the one that started it, and the process groups either of them is
//! in.** A `nodal reclaim` run from inside the unit's own home is attributed to that
//! unit, because it carries `NODAL_ID` like everything else there — and killing the
//! shell somebody typed the command into, in the middle of the command, is not a thing
//! to do. Their working directory stops existing, which they can see; their session does
//! not. The group half of that rule matters more than the process half, because a
//! signal to a shell's process group reaches the shell, the command, and every job the
//! shell is holding.
//!
//! Two identifiers are never signalled either. Group zero means "the group this process
//! is in", which is the one group a stop must not reach, and process one is the process
//! the system itself is.
//!
//! [`Signals`] is the seam, so a test can watch what would have been sent without a
//! machine having to lose a process.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long a target is given to stop after one signal, before the next one is sent.
pub const GRACE: Duration = Duration::from_secs(5);

/// How often the wait looks to see whether a target has gone.
pub const POLL: Duration = Duration::from_millis(50);

/// The identifiers no signal is ever sent to. Zero addresses this process's own group;
/// one is the process the system itself is.
const RESERVED: [u32; 2] = [0, 1];

/// One of the three signals this module sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// Interrupt it, which is what a person pressing `Ctrl-C` sends.
    Interrupt,
    /// Ask the process to stop, which it may handle.
    Terminate,
    /// Make it stop, which it may not.
    Kill,
}

/// The three signals in the order they are sent. A target that goes on one rung is
/// never sent the next.
pub const LADDER: [Signal; 3] = [Signal::Interrupt, Signal::Terminate, Signal::Kill];

/// What a signal is sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// One process, by identifier.
    Process(u32),
    /// Every process of one group, by group identifier. This is what a tether is.
    Group(u32),
}

impl Target {
    /// Whether this is a whole process group.
    #[must_use]
    pub const fn is_group(&self) -> bool {
        matches!(self, Self::Group(_))
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Process(pid) => write!(out, "process {pid}"),
            Self::Group(pgid) => write!(out, "group {pgid}"),
        }
    }
}

/// Where a signal goes. One implementation for a machine, one for a test.
pub trait Signals {
    /// Send `signal` to `target`. `false` when there is no such process or group, or
    /// when this account may not signal it.
    fn send(&self, target: Target, signal: Signal) -> bool;

    /// Whether the target is still there. A group is there while it holds one process.
    fn alive(&self, target: Target) -> bool;

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
    /// Targets that stopped when they were asked, on `SIGINT` or on `SIGTERM`.
    pub asked: Vec<Target>,
    /// Targets that had to be made to stop.
    pub killed: Vec<Target>,
    /// Targets that were still there after every signal.
    pub left: Vec<Target>,
    /// Targets that were never signalled because they are this operation's own.
    pub spared: Vec<Target>,
}

impl Stopped {
    /// How many targets are no longer running because of this.
    #[must_use]
    pub fn count(&self) -> usize {
        self.asked.len() + self.killed.len()
    }

    /// How many of those were whole process groups.
    #[must_use]
    pub fn groups(&self) -> usize {
        self.asked.iter().chain(self.killed.iter()).filter(|target| target.is_group()).count()
    }

    /// How many of those were single processes.
    #[must_use]
    pub fn processes(&self) -> usize {
        self.count() - self.groups()
    }

    /// Whether anything is still running.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.left.is_empty()
    }
}

/// Stop every target in `targets`, and report what became of each.
///
/// The order of `targets` is the order they are signalled in, so a caller puts the
/// certain ones first. Idempotent, which is what a step needs: a second call finds
/// every target gone and reports nothing asked, nothing killed and nothing left.
#[must_use]
pub fn processes(signals: &dyn Signals, targets: &[Target], grace: Duration) -> Stopped {
    let mut stopped = Stopped::default();
    let mut standing = Vec::new();
    for target in targets.iter().copied() {
        if standing.contains(&target) || stopped.spared.contains(&target) {
            continue;
        }
        if is_spared(target) {
            stopped.spared.push(target);
        } else {
            standing.push(target);
        }
    }
    let mut reached: Vec<(Target, Signal)> = Vec::new();
    for signal in LADDER {
        if standing.is_empty() {
            break;
        }
        let period = if signal == Signal::Kill { POLL } else { grace };
        standing = rung(signals, &standing, signal, period, &mut reached);
    }
    account(&mut stopped, &reached, standing);
    stopped
}

/// Send one rung's signal to everything still standing, wait out the period, and answer
/// with what is still there. A target the signal could not reach has gone, or is not
/// this account's to signal, and leaves the ladder either way.
fn rung(
    signals: &dyn Signals,
    standing: &[Target],
    signal: Signal,
    period: Duration,
    reached: &mut Vec<(Target, Signal)>,
) -> Vec<Target> {
    let mut sent = Vec::new();
    for target in standing.iter().copied() {
        if !signals.send(target, signal) {
            continue;
        }
        reached.retain(|(seen, _)| *seen != target);
        reached.push((target, signal));
        sent.push(target);
    }
    settle(signals, &sent, period)
}

/// Turn "the last signal each target received" into the report's three lists.
///
/// A target that was never reached appears nowhere: it had already gone when the stop
/// began, and a stop reports what it did rather than what it found.
fn account(stopped: &mut Stopped, reached: &[(Target, Signal)], standing: Vec<Target>) {
    for (target, signal) in reached.iter().copied() {
        if standing.contains(&target) {
            continue;
        }
        if signal == Signal::Kill {
            stopped.killed.push(target);
        } else {
            stopped.asked.push(target);
        }
    }
    stopped.left = standing;
}

/// Wait up to `period` for the signalled targets to go, and answer with the ones that
/// did not. Returns as soon as they are all gone rather than sleeping out the whole
/// period.
fn settle(signals: &dyn Signals, sent: &[Target], period: Duration) -> Vec<Target> {
    let deadline = Instant::now() + period;
    loop {
        let alive: Vec<Target> =
            sent.iter().copied().filter(|target| signals.alive(*target)).collect();
        if alive.is_empty() || Instant::now() >= deadline {
            return alive;
        }
        signals.wait(POLL);
    }
}

/// Whether this target is one no signal is ever sent to.
///
/// Public because a caller that has just made a process group of its own has to be able
/// to ask the same question before it records that group as a unit's to stop. Recording
/// one of these would put this process's own shell in the registry as something a later
/// `nodal reclaim` may signal.
#[must_use]
pub fn is_spared(target: Target) -> bool {
    match target {
        Target::Process(pid) => RESERVED.contains(&pid) || spared().contains(&pid),
        Target::Group(pgid) => RESERVED.contains(&pgid) || spared_groups().contains(&pgid),
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

/// The process groups this operation will not signal: the one it is in, and the one
/// whatever started it is in.
///
/// This is the rule that protects the shell. A person's terminal runs each command in a
/// process group of its own, and the shell sits in another; a stop that signalled either
/// would take down the session the command was typed into. A tether is never one of
/// these two, because `nodal run --tether` makes a group that nothing else is in.
#[must_use]
pub fn spared_groups() -> Vec<u32> {
    vec![own_group(), parent_group()]
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

/// The process group this process is in.
#[cfg(unix)]
fn own_group() -> u32 {
    // SAFETY: `getpgrp` takes no argument and no pointer, and cannot fail.
    let group = unsafe { libc::getpgrp() };
    u32::try_from(group).unwrap_or(0)
}

/// The process group the process that started this one is in. Zero when the host will
/// not say, which is a reserved identifier and so spares nothing new.
#[cfg(unix)]
fn parent_group() -> u32 {
    let Ok(parent) = i32::try_from(parent_id()) else { return 0 };
    // SAFETY: `getpgid` takes one integer and no pointer. A process that has gone is a
    // return value of -1 rather than undefined behaviour.
    let group = unsafe { libc::getpgid(parent) };
    u32::try_from(group).unwrap_or(0)
}

#[cfg(not(unix))]
const fn own_group() -> u32 {
    0
}

#[cfg(not(unix))]
const fn parent_group() -> u32 {
    0
}

/// The argument `kill` is given for a target: a process identifier, or the negative of
/// a group identifier, which is how the system addresses a whole group.
#[cfg(unix)]
fn addressed(target: Target) -> Option<i32> {
    match target {
        Target::Process(pid) => i32::try_from(pid).ok(),
        Target::Group(pgid) => i32::try_from(pgid).ok().and_then(i32::checked_neg),
    }
}

#[cfg(unix)]
impl Signals for Live {
    fn send(&self, target: Target, signal: Signal) -> bool {
        let Some(addressee) = addressed(target) else { return false };
        let number = match signal {
            Signal::Interrupt => libc::SIGINT,
            Signal::Terminate => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: `kill` takes two integers and no pointer. A target this account may
        // not signal, or that has already gone, is a return value rather than undefined
        // behaviour.
        unsafe { libc::kill(addressee, number) == 0 }
    }

    fn alive(&self, target: Target) -> bool {
        let Some(addressee) = addressed(target) else { return false };
        // SAFETY: as above; signal zero delivers nothing and only reports existence. For
        // a group it reports whether the group still holds one process.
        if unsafe { libc::kill(addressee, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

/// A platform with no signals stops nothing and says so by leaving every target.
#[cfg(not(unix))]
impl Signals for Live {
    fn send(&self, _target: Target, _signal: Signal) -> bool {
        false
    }

    fn alive(&self, _target: Target) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::cell::RefCell;
    use std::time::Duration;

    use super::{Signal, Signals, Target, processes};

    /// A machine whose targets stop on the first signal each of them honours.
    #[derive(Default)]
    struct Fake {
        alive: RefCell<Vec<Target>>,
        /// What each target ignores. Anything not listed stops on any signal.
        ignores: Vec<(Target, Signal)>,
        sent: RefCell<Vec<(Target, Signal)>>,
    }

    impl Signals for Fake {
        fn send(&self, target: Target, signal: Signal) -> bool {
            self.sent.borrow_mut().push((target, signal));
            if !self.alive.borrow().contains(&target) {
                return false;
            }
            if !self.ignores.contains(&(target, signal)) {
                self.alive.borrow_mut().retain(|standing| *standing != target);
            }
            true
        }

        fn alive(&self, target: Target) -> bool {
            self.alive.borrow().contains(&target)
        }

        fn wait(&self, _period: Duration) {}
    }

    fn machine(alive: &[Target], ignores: &[(Target, Signal)]) -> Fake {
        Fake {
            alive: RefCell::new(alive.to_vec()),
            ignores: ignores.to_vec(),
            sent: RefCell::new(Vec::new()),
        }
    }

    /// Every signal one target was sent, in order.
    fn ladder(fake: &Fake, target: Target) -> Vec<Signal> {
        fake.sent
            .borrow()
            .iter()
            .filter(|(sent, _)| *sent == target)
            .map(|(_, signal)| *signal)
            .collect()
    }

    #[test]
    fn a_process_that_stops_on_the_interrupt_never_sees_the_other_two() {
        let (first, second) = (Target::Process(41), Target::Process(42));
        let fake = machine(&[first, second], &[]);
        let stopped = processes(&fake, &[first, second], Duration::ZERO);
        assert_eq!(stopped.asked, [first, second]);
        assert!(stopped.killed.is_empty() && stopped.is_clear());
        assert_eq!(ladder(&fake, first), [Signal::Interrupt]);
        assert_eq!(ladder(&fake, second), [Signal::Interrupt]);
    }

    #[test]
    fn the_ladder_is_climbed_one_rung_at_a_time_and_only_as_far_as_it_has_to_be() {
        let target = Target::Process(7);
        let fake = machine(&[target], &[(target, Signal::Interrupt)]);
        let stopped = processes(&fake, &[target], Duration::ZERO);
        assert_eq!(stopped.asked, [target], "it stopped when it was asked, on the second rung");
        assert!(stopped.killed.is_empty() && stopped.is_clear());
        assert_eq!(ladder(&fake, target), [Signal::Interrupt, Signal::Terminate]);
    }

    #[test]
    fn a_target_that_honours_nothing_is_killed_and_the_kill_is_the_last_rung() {
        let target = Target::Process(7);
        let fake = machine(&[target], &[(target, Signal::Interrupt), (target, Signal::Terminate)]);
        let stopped = processes(&fake, &[target], Duration::ZERO);
        assert_eq!(stopped.killed, [target]);
        assert!(stopped.asked.is_empty() && stopped.is_clear());
        assert_eq!(stopped.count(), 1);
        assert_eq!(ladder(&fake, target), [Signal::Interrupt, Signal::Terminate, Signal::Kill]);
    }

    #[test]
    fn a_tether_is_stopped_as_one_group_rather_than_as_the_processes_in_it() {
        let group = Target::Group(900);
        let fake = machine(&[group], &[]);
        let stopped = processes(&fake, &[group], Duration::ZERO);
        assert_eq!(stopped.asked, [group]);
        assert_eq!(stopped.groups(), 1);
        assert_eq!(stopped.processes(), 0);
        assert_eq!(ladder(&fake, group), [Signal::Interrupt]);
    }

    #[test]
    fn the_targets_are_signalled_in_the_order_they_were_given() {
        let (group, process) = (Target::Group(900), Target::Process(41));
        let fake = machine(&[group, process], &[]);
        let _ = processes(&fake, &[group, process], Duration::ZERO);
        let order: Vec<Target> = fake.sent.borrow().iter().map(|(target, _)| *target).collect();
        assert_eq!(order, [group, process], "the certain target is signalled first");
    }

    #[test]
    fn stopping_what_has_already_stopped_asks_for_nothing() {
        let fake = machine(&[], &[]);
        let stopped = processes(&fake, &[Target::Process(7), Target::Group(9)], Duration::ZERO);
        assert_eq!(stopped.count(), 0);
        assert!(stopped.is_clear());
    }

    #[test]
    fn this_process_and_the_one_that_started_it_are_left_alone() {
        let mine = Target::Process(std::process::id());
        let fake = machine(&[mine], &[]);
        let stopped = processes(&fake, &[mine], Duration::ZERO);
        assert_eq!(stopped.spared, [mine]);
        assert!(stopped.left.is_empty(), "spared is not the same answer as would not stop");
        assert!(fake.sent.borrow().is_empty(), "nothing was signalled");
        assert!(fake.alive(mine));
    }

    #[test]
    fn the_group_this_process_is_in_is_never_signalled() {
        let ours = Target::Group(super::own_group());
        let fake = machine(&[ours], &[]);
        let stopped = processes(&fake, &[ours], Duration::ZERO);
        assert_eq!(stopped.spared, [ours], "signalling it would reach the caller's shell");
        assert!(fake.sent.borrow().is_empty());
    }

    #[test]
    fn group_zero_and_process_one_are_never_signalled() {
        let reserved = [Target::Group(0), Target::Process(0), Target::Process(1)];
        let fake = machine(&reserved, &[]);
        let stopped = processes(&fake, &reserved, Duration::ZERO);
        assert_eq!(stopped.spared.len(), reserved.len(), "{stopped:?}");
        assert!(fake.sent.borrow().is_empty());
    }
}
