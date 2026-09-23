//! A process a fixture starts, the tree it starts it in, and the promise that neither
//! outlives the test.
//!
//! A fixture that starts a process starts a family. `crates/nodal-core/tests/substrate.rs`
//! re-ran the test binary, the test binary ran the project's build command, and the build
//! command was a shell parked in a loop that forked `sleep 1` every second. Killing the
//! process the fixture had a handle on left the shell and the sleep running, reparented to
//! the process the system reaps orphans with, forking once a second for as long as the
//! machine stayed up. A host used for a week held fifty-eight of them, each standing in a
//! temporary directory that had been unlinked underneath it — a tree with no name left,
//! held on disk by one open file descriptor.
//!
//! So a fixture does not own a process here. It owns a **process group**, and [`Owned`] is
//! the one type that owns one.
//!
//! # What makes the number safe to signal
//!
//! A process identifier is reused. A fixture that remembers the number 3013790 and signals
//! it a minute later may be signalling somebody else's work, and that is the one mistake a
//! test kit must not make. There are two answers here, and each says which it is using.
//!
//! A group this owner **started** is proved by its leader. The command is put in a process
//! group of its own, so the leader of that group is this process's own child and the group's
//! number is the leader's. That number is this owner's twice over: the kernel cannot give it
//! away while the child is unreaped, and it cannot give it away while the group it names
//! still holds a process. So there is never a moment when the number names a group that is
//! not this owner's, and never a signal sent to one.
//!
//! **A group is signalled once, and only while its leader is unreaped.** That is the whole
//! of the rule. The moment the leader is reaped the number is held by the survivors alone,
//! and a group that then empties gives its number back to the kernel, which may hand it to
//! somebody else's leader. So the owner stops naming the group at that point: it takes the
//! list of what was still in the group — while the leader was still holding the number — and
//! from then on it names those processes, one at a time, by the rule below. A fixture that
//! kills the leader by itself ([`Owned::kill_the_leader_alone`]) crosses the same line, and
//! is downgraded the same way.
//!
//! A process this owner **adopted** — a tether, what a recipe hook backgrounded, or what was
//! left in a group whose leader has been reaped — is nobody's child, so nothing holds its
//! number. It is read when it is taken for the two things a later process of the same number
//! cannot repeat: the group it is in, and the moment it started. The reading is taken again
//! before anything is signalled. Where the host will not say, or where the two readings
//! differ, **nothing is signalled**: the process is left running where a person can see it
//! and the test fails naming it. A fixture that signals a number it cannot prove is a fixture
//! that kills somebody else's work, and a failing test is the cheaper of the two.
//!
//! What that leaves is one residual, and it is worth writing down. `ps` reports a start time
//! to the second, so two processes of the same number could read alike only by being in the
//! same process group and having started inside the same second as the one this owner took.
//! The kernel does not reissue a number that fast — it walks the whole range first — so the
//! residual is a coincidence nothing here has seen. It is a residual rather than a hole
//! because the alternative, signalling on the number alone, is the mistake this module exists
//! to refuse.
//!
//! # What the failure model is
//!
//! One signal, and it is `SIGKILL`. An owner is a teardown and not a conversation: the
//! ladder a person would use is the product's ([`nodal_core::runtime::stop`]), and a test
//! that wants one asserts it against the product. A target that is still there [`GRACE`]
//! after `SIGKILL` is a process the kernel will not schedule; it is named rather than
//! waited for, because a suite that hangs has failed a person more thoroughly than one
//! that says which process it could not stop.
//!
//! Nothing here sleeps blind. [`until`] and [`wait_for`] take a reading again until it
//! answers or a deadline passes, and both name what did not happen.
//!
//! # The marker
//!
//! Every process an owner starts carries [`MARKER`]. Nothing in Nodal reads it: it is
//! there so that `ci/acceptance-process-hygiene.sh` can run the whole suite and then ask
//! the machine one question — is anything from that run still running — and so that the
//! answer covers no process belonging to anybody else.

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use nodal_core::runtime::stop::{self, Live, Signal, Signals as _, Target};
use tempfile::TempDir;

/// The variable every process an [`Owned`] starts carries.
///
/// Not a `NODAL_` name, deliberately. A process scan keeps the variables of that prefix
/// and this one has nothing to do with the product, so it is named outside it and the
/// scan discards it like any other.
pub const MARKER: &str = "TEST_OWNER_RUN";

/// How long a target is given to go after `SIGKILL` before it is called a survivor.
pub const GRACE: Duration = Duration::from_secs(5);

/// How long [`wait_for`] and [`until`] give something to happen.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How often either of them looks.
const POLL: Duration = Duration::from_millis(10);

/// The value of [`MARKER`] for this run.
///
/// `ci/acceptance-process-hygiene.sh` exports one before it runs the suites, so every test
/// binary of one run marks its processes with the same value and the sentinel counts that
/// run's processes and nobody else's. A suite run without the script marks by test process
/// instead, which is still a value no other run on the machine has.
#[must_use]
pub fn token() -> String {
    std::env::var(MARKER).unwrap_or_else(|_| format!("run-of-{}", std::process::id()))
}

/// Put the marker in a command's environment, so that the sentinel can see what it starts.
///
/// [`Owned`] does this to everything it starts. [`crate::runner`] does it to every command
/// for the binary, so that what the product backgrounds is marked too.
pub fn mark(command: &mut Command) -> &mut Command {
    command.env(MARKER, token())
}

// ---------------------------------------------------------------------------
// The owner.
// ---------------------------------------------------------------------------

/// A process group a fixture started, or one process a fixture adopted, reclaimed when the
/// test ends whichever way it ends.
///
/// Reclaimed on a normal return, on a failed assertion, on a deadline and on a panic,
/// because every one of those unwinds through [`Drop`] — and the temporary tree the owner
/// was given is removed after the group has gone, never before, because the tree is a
/// field of the owner and a field is dropped after the value's own drop has run.
pub struct Owned {
    /// What a signal is addressed to.
    target: Target,
    /// What says the target is still the one this owner took.
    proof: Proof,
    /// A temporary tree this owner's processes are the only users of.
    tree: Option<TempDir>,
    /// Whether the reclaim has already run, so that a second one does nothing.
    cleared: bool,
}

/// What makes the number an owner is about to signal the number it took.
enum Proof {
    /// This owner started the group, and the leader is its own child. `None` once a caller
    /// that wanted the output has taken the child back, which happens only after the group
    /// is clear and there is nothing left to prove.
    Leader(Option<Child>),
    /// This owner adopted one process something else started.
    Adopted(Held),
    /// This owner started the group and has since reaped its leader, so the group's number
    /// is no longer anything this owner holds. These are the processes that were in the
    /// group at the moment the number was still its own, each held from then on by name.
    Survivors(Vec<Held>),
}

/// One process an owner holds by number alone, and what that number was when it took it.
struct Held {
    /// Its identifier.
    pid: u32,
    /// What `ps` said about it then: the group it is in, and the moment it started. `None`
    /// where the host would not say, which is a process this owner never signals.
    was: Option<String>,
}

impl Held {
    /// Take a process by its number, and read it while it is still the one being taken.
    fn take(pid: u32) -> Self {
        Self { pid, was: described(pid) }
    }
}

impl Owned {
    /// Start `command` in a process group of its own, and own that group.
    ///
    /// Standard input is closed. A fixture's process has nobody to read from, and one that
    /// inherited the terminal could stop the suite by asking it a question.
    ///
    /// # Panics
    ///
    /// If the command could not be started.
    #[must_use]
    pub fn spawn(command: &mut Command) -> Self {
        Self::started(command, None)
    }

    /// The same, owning `tree` as well and running in it.
    ///
    /// For a fixture whose process is the only user of a temporary directory. The tree is
    /// removed after the group has gone, so no process of this owner's ever stands in a
    /// directory that has been unlinked underneath it.
    ///
    /// # Panics
    ///
    /// As [`Owned::spawn`].
    #[must_use]
    pub fn in_tree(tree: TempDir, command: &mut Command) -> Self {
        command.current_dir(tree.path());
        Self::started(command, Some(tree))
    }

    /// Own a process something else started, by the identifier the test recorded.
    ///
    /// This is the backstop over a process the product is meant to stop: a tether, or what
    /// a recipe hook backgrounded. The test still asserts that the product stopped it. This
    /// is what reclaims it when the assertion before that one fails.
    #[must_use]
    pub fn adopt(pid: u32) -> Self {
        Self {
            target: Target::Process(pid),
            proof: Proof::Adopted(Held::take(pid)),
            tree: None,
            cleared: false,
        }
    }

    fn started(command: &mut Command, tree: Option<TempDir>) -> Self {
        mark(command).stdin(Stdio::null());
        group_of_its_own(command);
        let child = command.spawn().expect("the process starts");
        // The child was put in a group of its own, so it is that group's leader and the
        // group's number is the child's. On a host with no process groups there is one
        // process to own and it is named as one.
        let target =
            if cfg!(unix) { Target::Group(child.id()) } else { Target::Process(child.id()) };
        Self { target, proof: Proof::Leader(Some(child)), tree, cleared: false }
    }

    /// The number this owner's target is named by: the process identifier of the child it
    /// started, which is also that child's group's, or of the process it adopted.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        match self.target {
            Target::Process(pid) | Target::Group(pid) => pid,
        }
    }

    /// Where the tree this owner was given is.
    ///
    /// # Panics
    ///
    /// If this owner was not given one.
    #[must_use]
    pub fn tree(&self) -> &Path {
        self.tree.as_ref().expect("this owner was given a tree").path()
    }

    /// Whether anything this owner owns is still running.
    ///
    /// The group while the owner still names one; otherwise the processes it holds by name.
    /// The difference matters: once the leader has been reaped, the group's number may be
    /// somebody else's, and asking whether that number is alive would be asking about them.
    #[must_use]
    pub fn running(&self) -> bool {
        match &self.proof {
            Proof::Leader(_) => Live.alive(self.target),
            Proof::Adopted(held) => held.running(),
            Proof::Survivors(held) => held.iter().any(Held::running),
        }
    }

    /// Every process this owner still owns, by identifier.
    ///
    /// This is what "zero survivors" is asserted with. For a group it is read from the
    /// process table, and is empty where the host publishes none — which is not the same
    /// answer as none, so [`Owned::running`] is the question every host answers.
    #[must_use]
    pub fn survivors(&self) -> Vec<u32> {
        match &self.proof {
            Proof::Leader(_) => listing(self.target)
                .iter()
                .filter_map(|line| first_word(line)?.parse().ok())
                .collect(),
            Proof::Adopted(held) => held.running().then_some(held.pid).into_iter().collect(),
            Proof::Survivors(held) => {
                held.iter().filter(|one| one.running()).map(|one| one.pid).collect()
            }
        }
    }

    /// Stop everything this owner owns, wait for it to go, and reap what it started.
    ///
    /// Idempotent. A second call finds the work done and does nothing, and so does a call
    /// about a process that had already exited by itself — which is still reaped, because a
    /// fixture that leaves a zombie has left something behind too.
    ///
    /// # Panics
    ///
    /// If anything is still there, naming it. That includes the target this owner would not
    /// signal because it could not prove the number: it is left running and the test fails.
    pub fn reclaim(&mut self) {
        if let Some(left) = self.clear() {
            panic!("{left}");
        }
    }

    /// Everything the process printed, once it has finished.
    ///
    /// The group is cleared first, so a command that finished while something it started
    /// did not leaves nothing behind either.
    ///
    /// # Panics
    ///
    /// If this owner adopted its target rather than starting it, and as [`Owned::reclaim`].
    #[must_use]
    pub fn into_output(mut self) -> Output {
        self.reclaim();
        let Proof::Leader(held) = &mut self.proof else {
            panic!("an adopted process is not this test's to read the output of");
        };
        let child = held.take().expect("the child is held until it is asked for");
        child.wait_with_output().expect("the output is readable")
    }

    /// Signal the leader alone, and leave the rest of the group running.
    ///
    /// One fixture needs this and says why: `crates/nodal-cli/tests/merge.rs` kills a merge
    /// while the `git` it started is holding the branch, because what it asserts next is
    /// that the branch did not move. A signal to the group would take that `git` too, and
    /// the lock it is holding would stay on the disk as a thing no process is responsible
    /// for any more.
    ///
    /// The leader's number is provable here for the same reason the group's is: it is this
    /// owner's own unreaped child. What the rest of the group is, is the question this
    /// answers before it reaps.
    ///
    /// The leader is reaped rather than held, because a fixture that kills it by itself does
    /// it so that the product can be asked what it makes of a process that has gone, and a
    /// leader held unreaped has not gone. Reaping gives the number back, though, and with it
    /// the group's: what is left in the group is holding that number now, and when the last
    /// of them exits the kernel may give it to somebody else's leader. So the owner stops
    /// naming the group here. It takes what is in the group first — while the unreaped
    /// leader still makes the number its own — and from then on it holds those processes by
    /// name, each with the reading that says it is still the one that was taken.
    ///
    /// # Panics
    ///
    /// If this owner did not start its target, or if the leader was still there [`GRACE`]
    /// after `SIGKILL`.
    pub fn kill_the_leader_alone(&mut self) {
        assert!(
            matches!(self.proof, Proof::Leader(_)),
            "this owner did not start the process it is being asked to signal by itself"
        );
        Live.send(Target::Process(self.pid()), Signal::Kill);
        let deadline = Instant::now() + GRACE;
        while !self.exited() {
            assert!(
                Instant::now() < deadline,
                "the leader was still there {GRACE:?} after SIGKILL"
            );
            std::thread::sleep(POLL);
        }
        let held = held_in(self.target, self.pid());
        self.reap();
        self.proof = Proof::Survivors(held);
    }

    /// Whether the child this owner started has exited.
    ///
    /// Asked without reaping it. `Child::try_wait` reaps, and a reaped child's number is one
    /// the kernel may give away at once — which would take away the one thing that makes
    /// this owner's group provably its own. So the question is put with `waitid` and
    /// `WNOWAIT`, which reports the exit and leaves the entry in the table for the reap that
    /// comes after the group has been signalled.
    ///
    /// # Panics
    ///
    /// If this owner adopted its target rather than starting it.
    #[must_use]
    pub fn exited(&self) -> bool {
        assert!(
            matches!(self.proof, Proof::Leader(_)),
            "a process this owner does not hold as its own child is not this test's to wait on"
        );
        has_exited(self.pid())
    }

    /// Signal what this owner owns, wait for it to go, reap what it started, and answer
    /// with a word about whatever is still there.
    ///
    /// Never panics. [`Drop`] must not, while the thread it runs on is already unwinding,
    /// so what a survivor means is the caller's to decide.
    fn clear(&mut self) -> Option<String> {
        if self.cleared {
            return None;
        }
        self.cleared = true;
        match &self.proof {
            Proof::Leader(_) => self.stop_the_group(),
            Proof::Adopted(held) => stop_each(std::slice::from_ref(held)),
            Proof::Survivors(held) => stop_each(held),
        }
    }

    /// Kill the group this owner started, take what is left in it, reap the leader, and
    /// answer with whatever did not go.
    ///
    /// The one signal a group ever gets, and it is sent while the leader is unreaped, which
    /// is what says the number is this owner's. Everything after that is about processes:
    /// the list is taken before the reap, for the same reason, and each of them is waited
    /// for by name.
    fn stop_the_group(&mut self) -> Option<String> {
        if stop::is_spared(self.target) {
            return Some(format!("{} is this test's own, so nothing was signalled", self.target));
        }
        Live.send(self.target, Signal::Kill);
        let held = held_in(self.target, self.pid());
        self.reap();
        stop_each(&held)
    }

    /// Wait for the leader, so that no fixture leaves a zombie.
    ///
    /// Bounded like everything else: a leader still running after `SIGKILL` is one
    /// [`settle`] is about to name, and this is not the place to wait for it forever.
    fn reap(&mut self) {
        let Proof::Leader(Some(child)) = &mut self.proof else { return };
        let deadline = Instant::now() + GRACE;
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) if Instant::now() >= deadline => return,
                Ok(None) => std::thread::sleep(POLL),
            }
        }
    }
}

impl Drop for Owned {
    /// The tree is a field, so it is removed after this has run and never before.
    fn drop(&mut self) {
        let Some(left) = self.clear() else { return };
        let report = format!("a fixture left a process behind: {left}");
        assert!(std::thread::panicking(), "{report}");
        eprintln!("the test was already failing, and {report}");
    }
}

impl Held {
    /// Whether this process is still there.
    fn running(&self) -> bool {
        Live.alive(Target::Process(self.pid))
    }
}

/// Stop every process an owner holds by name, and answer with a word about what is left.
///
/// Each is proved before it is signalled and each says for itself why it was not, so a test
/// that fails here is told which process and which reason.
fn stop_each(held: &[Held]) -> Option<String> {
    let left: Vec<String> = held.iter().filter_map(stop_one).collect();
    (!left.is_empty()).then(|| left.join("; "))
}

/// Kill one process an owner holds, if it is provably still the process the owner took.
fn stop_one(held: &Held) -> Option<String> {
    let target = Target::Process(held.pid);
    if !Live.alive(target) {
        return None;
    }
    let Some(was) = held.was.as_deref() else {
        return Some(format!(
            "{target} is running and this host would not say what it was when this test took \
             it, so nothing was signalled and it is left where a person can see it"
        ));
    };
    let Some(now) = described(held.pid) else {
        // It went between the two questions, which is the answer this was looking for. A
        // target that has gone needs no signal, and `settle` is what says it has.
        return settle(target);
    };
    if was != now {
        return Some(format!(
            "{target} is running and this test cannot prove it is the process it took — it \
             was {was:?} and it is {now:?} ({what}) — so nothing was signalled and it is left \
             where a person can see it",
            what = listing(target).join("; ")
        ));
    }
    if stop::is_spared(target) {
        return Some(format!("{target} is this test's own, so nothing was signalled"));
    }
    Live.send(target, Signal::Kill);
    settle(target)
}

/// Wait up to [`GRACE`] for a target to go, and answer with a word about it when it does
/// not.
///
/// `SIGKILL` cannot be caught, so the only thing that survives one is a process the kernel
/// will not schedule. Naming it is more use to a person than waiting for it.
fn settle(target: Target) -> Option<String> {
    let deadline = Instant::now() + GRACE;
    while Live.alive(target) {
        if Instant::now() >= deadline {
            return Some(format!(
                "{target} was still there {GRACE:?} after SIGKILL: {}",
                listing(target).join("; ")
            ));
        }
        std::thread::sleep(POLL);
    }
    None
}

/// The processes a group holds, apart from one, each taken by name.
///
/// Called while the group's number is still the caller's — which is while its leader is
/// unreaped — because a list taken after that could hold somebody else's process. The one
/// left out is the leader itself, which is about to be reaped.
fn held_in(target: Target, leader: u32) -> Vec<Held> {
    listing(target)
        .iter()
        .filter_map(|line| first_word(line)?.parse::<u32>().ok())
        .filter(|pid| *pid != leader)
        .map(Held::take)
        .collect()
}

/// The two things about a process that a later process of the same number cannot repeat: the
/// group it is in, and the moment it started.
///
/// **Not the command.** A process whose argument vector the host can no longer read — one on
/// its way out, which is exactly when this reading is taken — is reported by its accounting
/// name in brackets instead, so the command changes while the process does not. A proof that
/// included it would refuse to signal processes it had every right to, which is a refusal
/// that reads like a bug and hides a real one. The command is still printed when a proof
/// fails, because a person reading that message wants to know what the process is.
///
/// `ps` rather than `/proc`, because this is the reading a macOS host has to be able to take
/// as well. `None` where `ps` would not say, which is a target nothing signals. The start
/// time is reported to the second, which is the residual the module documentation states.
fn described(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-o", "pgid=,lstart=", "-p", &pid.to_string()])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Every process a target still holds, as a line a person can read.
///
/// The whole table is read and filtered here rather than asked for by group, because the
/// flag that selects a process group is spelled differently on the two hosts this suite
/// runs on and one of those spellings means a user group instead.
fn listing(target: Target) -> Vec<String> {
    let Ok(output) =
        Command::new("ps").args(["-eo", "pid=,pgid=,args="]).stderr(Stdio::null()).output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| holds(target, line))
        .map(|line| line.trim().to_owned())
        .collect()
}

/// Whether one line of the process table is a process the target holds.
fn holds(target: Target, line: &str) -> bool {
    let mut words = line.split_whitespace();
    let Some(Ok(process)) = words.next().map(str::parse::<u32>) else { return false };
    let Some(Ok(group)) = words.next().map(str::parse::<u32>) else { return false };
    match target {
        Target::Process(wanted) => process == wanted,
        Target::Group(wanted) => group == wanted,
    }
}

/// The first word of a line, which in a process table is the identifier.
fn first_word(line: &str) -> Option<&str> {
    line.split_whitespace().next()
}

/// Whether a child has exited, leaving it in the process table either way.
///
/// `WNOWAIT` is what makes it a reading rather than a reap. A child that has not exited
/// answers with the identifier left at zero, which is the shape `WNOHANG` reports "not yet"
/// with.
#[cfg(unix)]
fn has_exited(pid: u32) -> bool {
    let pid = libc::id_t::from(pid);
    // SAFETY: `waitid` is given a `siginfo_t` of ours to write into and reads no pointer of
    // ours. The flags ask it to report an exit and to leave the child where it is.
    let mut about: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `waitid` takes two integers, one pointer to the `siginfo_t` above, which is
    // live for the call and correctly typed, and a flag word. It writes only through that
    // pointer.
    let asked = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &raw mut about,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    // SAFETY: the call answered, so the structure it wrote into is initialised.
    asked == 0 && unsafe { about.si_pid() != 0 }
}

/// A host that will not say answers that the child is still running, so a bounded wait
/// reaches its deadline rather than calling a running process finished.
#[cfg(not(unix))]
const fn has_exited(_pid: u32) -> bool {
    false
}

/// Put a command in a process group of its own, so that one signal reaches everything it
/// starts and everything those start in turn.
#[cfg(unix)]
fn group_of_its_own(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0);
}

/// A host with no process groups owns the one process instead, and says so by making no
/// group.
#[cfg(not(unix))]
fn group_of_its_own(_command: &mut Command) {}

// ---------------------------------------------------------------------------
// The shapes a test starts a process in.
// ---------------------------------------------------------------------------

/// A `sleep` whose variables this host shows to the account that started it.
///
/// On Linux that is `sleep` itself. On macOS `/bin/sleep` is a restricted binary, and the
/// kernel zeroes the variables of one (`nodal_core::runtime::processes`). A copy that
/// `codesign` signs again is an ordinary program, as a tool a person installed is. The
/// copy is made once, beside the test binaries. Each maker signs a draft of its own and
/// renames it into place, so that two tests that make it at once both get a whole file.
///
/// # Panics
///
/// If the copy cannot be made or signed.
#[must_use]
pub fn readable_sleep() -> std::path::PathBuf {
    if !cfg!(target_os = "macos") {
        return std::path::PathBuf::from("sleep");
    }
    let exe = std::env::current_exe().expect("a test binary knows its own path");
    let directory = exe.parent().expect("a test binary is in a directory").join("readable");
    let program = directory.join("sleep");
    if program.is_file() {
        return program;
    }
    std::fs::create_dir_all(&directory).expect("the directory for the copy is made");
    let draft = tempfile::Builder::new()
        .prefix("sleep.")
        .tempfile_in(&directory)
        .expect("a draft of the copy is made");
    std::fs::copy("/bin/sleep", draft.path()).expect("/bin/sleep is copied");
    let signed = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(draft.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("codesign runs");
    assert!(signed.success(), "codesign signed the copy of /bin/sleep");
    draft.persist(&program).expect("the signed copy is put in place");
    program
}

/// A sleeping process carrying a unit's environment, as an activated shell gives it.
///
/// This is the signal attribution calls certain: the process says which unit it is in, in
/// the two variables `nodal shell`, the prompt hook and direnv all set. The program is
/// [`readable_sleep`], so that both hosts show the variables.
///
/// # Panics
///
/// As [`Owned::spawn`].
#[must_use]
pub fn carrying(unit: &str, home: &Path) -> Owned {
    let mut command = Command::new(readable_sleep());
    command.arg("30").env("NODAL_ID", unit).env("NODAL_ROOT", home);
    Owned::spawn(&mut command)
}

/// A sleeping process carrying one unit's environment and standing in another's home.
///
/// The case two readings of the process table can disagree over. It carries `NODAL_ID`,
/// so it is something Nodal started and not a stranger; the identifier is not the
/// identifier of the unit whose home it stands in, so a reclaim of *that* unit will never
/// signal it and would move the home out from under it. That is a bystander to the home
/// it stands in and owned runtime to the unit it names, both at once.
///
/// # Panics
///
/// As [`Owned::spawn`].
#[must_use]
pub fn of_another_unit(unit: &str, its_home: &Path, standing_in: &Path) -> Owned {
    let mut command = Command::new(readable_sleep());
    command.arg("30").current_dir(standing_in).env("NODAL_ID", unit).env("NODAL_ROOT", its_home);
    Owned::spawn(&mut command)
}

/// A process holding one file open **for writing**, standing nowhere near it.
///
/// This is the shape occupancy used to miss altogether: a test runner or
/// a dev server started from a terminal that has since changed directory, writing its
/// database into a git-ignored corner of a home. Its working directory is `/`, it carries
/// no Nodal variable, and it would go on writing into the inode after the home was
/// renamed out from under it, never learning that it moved.
///
/// The descriptor is opened by the shell and then `exec`'d through, so the process that
/// survives is `sleep` holding a descriptor the shell opened. A shell redirection is not
/// close-on-exec, which is what makes that work and is why no helper binary is needed.
///
/// # Panics
///
/// As [`Owned::spawn`], and when the file's directory cannot be made.
#[must_use]
pub fn writing_into(file: &Path) -> Owned {
    holding(file, ">>")
}

/// The same process, holding the file open for **reading** only.
///
/// The control for [`writing_into`], and the whole reason the widened rule is affordable.
/// An editor, a language server, a `tail` and a `grep` all hold descriptors like this one,
/// and a rule that refused over them would refuse every reclaim on a working machine.
/// `/proc/<pid>/fdinfo/<n>` carries the open flags, so the two are told apart at the cost
/// of one file read.
///
/// # Panics
///
/// As [`writing_into`].
#[must_use]
pub fn reading_from(file: &Path) -> Owned {
    holding(file, "<")
}

/// One process holding `file` open with `redirection`, from a working directory of `/`.
fn holding(file: &Path, redirection: &str) -> Owned {
    if let Some(above) = file.parent() {
        std::fs::create_dir_all(above).expect("the directory the file goes in is made");
    }
    if !file.exists() {
        std::fs::write(file, b"").expect("the file to hold open is made");
    }
    let sleep = readable_sleep();
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("exec 9{redirection}\"$1\" && exec \"$2\" 30"))
        .arg("holding")
        .arg(file)
        .arg(&sleep)
        .current_dir("/")
        .env_remove("NODAL_ID")
        .env_remove("NODAL_ROOT");
    let owned = Owned::spawn(&mut command);
    // The descriptor is open once the shell has `exec`'d, and the caller's next act is to
    // ask the machine what is holding the file. Waiting for the hold to be real is the
    // difference between asserting the rule and asserting a race.
    wait_for("the descriptor is open", || held_by(owned.pid(), file));
    owned
}

/// Whether this process holds `file` open, as the host publishes it.
///
/// **A host that does not publish it answers yes.** macOS has no per-descriptor view a
/// test may read without privilege, so there is nothing here to wait for, and waiting
/// anyway would spend the whole timeout and then fail a test over a reading the host was
/// never going to make. The tests whose claim needs the descriptor to be *seen* name macOS
/// and skip; this waits only where the answer means something.
///
/// **The comparison is between resolved paths.** The kernel names an open file by the path
/// it resolved to, with every link on the way taken out, and the acceptance script runs
/// this whole tree under a second name — so a test that compared the name it wrote would
/// wait thirty seconds for a hold that was there all along. This is the same rule
/// [`nodal_core::lifecycle::assess::bystander`] states for a home reached through a link.
fn held_by(pid: u32, file: &Path) -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    let Ok(wanted) = std::fs::canonicalize(file) else { return false };
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { return false };
    entries.flatten().any(|entry| std::fs::read_link(entry.path()).is_ok_and(|held| held == wanted))
}

/// A sleeping process that merely stands in a home and carries no Nodal variable.
///
/// This is the signal attribution calls probable: a terminal with no integration and no
/// direnv, standing in the directory.
///
/// # Panics
///
/// As [`Owned::spawn`].
#[must_use]
pub fn standing_in(home: &Path) -> Owned {
    let mut command = Command::new("sleep");
    command.arg("30").current_dir(home).env_remove("NODAL_ID").env_remove("NODAL_ROOT");
    Owned::spawn(&mut command)
}

/// A sleeping process carrying no Nodal variable and standing nowhere a unit owns.
///
/// This is the bystander a stop must never reach. Nothing recorded it, nothing attributes
/// it, and its group identifier is one no registry row holds — so a teardown that signalled
/// it would be signalling by proximity rather than by record. The test that starts one owns
/// it like any other, which is what keeps the bystander out of the next run.
///
/// # Panics
///
/// As [`Owned::spawn`].
#[must_use]
pub fn in_a_group_of_its_own() -> Owned {
    let mut command = Command::new("sleep");
    command.arg("30").env_remove("NODAL_ID").env_remove("NODAL_ROOT");
    Owned::spawn(&mut command)
}

// ---------------------------------------------------------------------------
// Reading the machine, without hanging on it.
// ---------------------------------------------------------------------------

/// Whether a process is still there.
///
/// Signal zero on one process identifier, sent in this process rather than by running
/// `kill`: the question is asked on every poll of every bounded wait in the suite, and a
/// reading that costs a fork and an exec each time is a reading tests pay for by the
/// thousand. It is the product's own existence check
/// ([`nodal_core::runtime::stop::Signals::alive`]), which is the same call by the same rule
/// — a process that exists but belongs to another account answers yes — and a host with no
/// process table answers it too.
#[must_use]
pub fn alive(pid: u32) -> bool {
    Live.alive(Target::Process(pid))
}

/// Run a command and insist that it finishes within `limit`.
///
/// `Command::output` waits for as long as the process takes, so a test of "this does not
/// block" written with it does not fail — it hangs, and the suite is killed by whatever is
/// watching the job. This is the bounded form: the process is polled, and a deadline it
/// passes is a named failure with the command in it.
///
/// The command runs as an [`Owned`] group, so a deadline takes the whole family and not
/// only the process the deadline was about.
///
/// One caller so far: `nodal new --carry` against a checkout holding a named pipe. A reader
/// of a pipe waits for a writer that may never come, so "the refusal is reached without
/// opening it" is a claim about time and has to be asserted as one.
///
/// Both streams are pipes, which is safe for a command whose whole output is a refusal and
/// a report. A command that filled a pipe buffer would block on the write and be killed
/// here as though it had hung, so this is not the runner for a chatty one.
///
/// # Panics
///
/// If the command could not be started, or had not finished within `limit`.
pub fn within(command: &mut Command, limit: Duration) -> Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let owned = Owned::spawn(command);
    let deadline = Instant::now() + limit;
    while !owned.exited() {
        assert!(Instant::now() < deadline, "{command:?} had not finished within {limit:?}");
        std::thread::sleep(POLL);
    }
    owned.into_output()
}

/// Wait for something to become true, and insist that it does.
///
/// A signal is delivered rather than applied, so "the process is gone" is a claim about a
/// moment shortly after the command returned and not about the instant it did.
///
/// # Panics
///
/// If it has not happened within [`TIMEOUT`], naming what did not happen.
pub fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    until(what, || ready().then_some(()));
}

/// Wait for a reading to answer, and answer with it.
///
/// The bounded form of "start a process, then read the machine". A process exists before it
/// has replaced itself with the program it was started for, so a scan taken the instant
/// after a spawn can see a process that is not yet the one the test means: no row, or a row
/// with the wrong command in it. That is a race a test loses rarely and unreproducibly.
/// This is the shape that cannot lose it — the reading is taken again until it answers.
///
/// It reads the product rather than working around it. What is retried is the question,
/// never the answer: a reading that comes back wrong rather than absent fails here as it
/// would anywhere.
///
/// # Panics
///
/// If nothing answered within [`TIMEOUT`], naming what did not.
pub fn until<T>(what: &str, mut reading: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(found) = reading() {
            return found;
        }
        assert!(Instant::now() < deadline, "{what} did not happen within {TIMEOUT:?}");
        std::thread::sleep(POLL);
    }
}
