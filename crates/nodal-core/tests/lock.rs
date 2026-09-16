//! Acceptance for the one question the write verbs ask: may this process write here.
//!
//! A hold is an actor and a lineage. The actor alone was not enough, because a fleet of
//! agents all report as `claude-code`: the name matched itself, and a second agent
//! entered a home the first one held without a word. These tests state the rule that
//! replaced it.
//!
//! The lineage is the POSIX session, and not the process group. Every write verb runs in
//! a new process, so the recorded pid cannot be what a re-entry matches — the first
//! `nodal run` has exited before the second one starts. A process group cannot be it
//! either: a shell with job control puts every foreground command in a group of its own,
//! so two `nodal run` commands typed one after the other are two groups. Both of them,
//! and the shell that started them, are in one session.
//!
//! Most claims here read the process table, which macOS does not publish. Each of those
//! asserts both hosts rather than skipping: the reading on Linux, and on macOS the older
//! behaviour — the name alone — which is what "I cannot see" has to fall back to. The
//! claims that hold on every host, such as a refresh writing what it reports, are
//! asserted once and not branched.

#![allow(clippy::unwrap_used, reason = "tests fail by panicking")]
#![allow(clippy::expect_used, reason = "tests fail by panicking")]

use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command};

use nodal_core::model::{Project, ProjectId, Timestamp, Unit, UnitId};
use nodal_core::runtime::lock::{self, Entered};
use nodal_core::runtime::processes;
use nodal_core::store::{Store, locks, projects, units};
use nodal_safety::rows;

/// The unit every hold in this file is about.
const UNIT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// The project it belongs to.
const PROJECT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

/// Whether this host publishes the table a lineage is read from.
///
/// The repository's idiom, and the reason both branches of every test are written out:
/// a host that cannot see must not quietly pass the claim it did not check.
fn can_see_processes() -> bool {
    cfg!(target_os = "linux")
}

/// A registry holding one unit of one project, and the two rows an entry needs.
fn fixture(root: &Path) -> (Store, Unit, Project) {
    let now = Timestamp::now();
    let project =
        rows::project(ProjectId::parse(PROJECT).unwrap(), root.join("checkout"), "fixture", now);
    let unit = rows::unit(
        UnitId::parse(UNIT).unwrap(),
        project.id,
        "fix-worker-import",
        "nodal/fix-worker-import",
        now,
    );
    let store = Store::open(root.join("registry.db")).unwrap();
    projects::insert(store.conn(), &project).unwrap();
    units::insert(store.conn(), &unit).unwrap();
    (store, unit, project)
}

/// A live process in a session of its own: the second agent of a fleet, in one process.
///
/// The session is made between the fork and the exec rather than by the `setsid`
/// command. That command forks where it is already a process group leader, which a test
/// binary is, and the identifier it hands back is then not the session's. This child
/// calls `setsid` itself, so it is the session leader and its own identifier is the
/// session's, with nothing to read back and nothing left running.
struct Elsewhere(Child);

impl Elsewhere {
    /// Start it, and answer the session it leads.
    fn started() -> Self {
        let mut command = Command::new("sleep");
        command.arg("300");
        // SAFETY: the closure runs in the child between the fork and the exec, where
        // only async-signal-safe calls are allowed. `setsid` is one: it takes no
        // pointer, allocates nothing and reads nothing of this process.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self(command.spawn().expect("a child process starts"))
    }

    /// The session it leads, which is its own identifier.
    fn session(&self) -> u32 {
        self.0.id()
    }

    /// End it now, so the session holds no process.
    fn end(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Elsewhere {
    /// A test that fails part way through must not leave the process behind:
    /// `ci/acceptance-process-hygiene.sh` counts what a run leaves running.
    fn drop(&mut self) {
        self.end();
    }
}

/// Rewrite the recorded lineage of the hold on `unit`, leaving everything else standing.
fn recorded_as(store: &Store, unit: UnitId, session: Option<u32>) {
    let held = locks::get(store.conn(), unit).unwrap().expect("a hold to rewrite");
    locks::hand_over(store.conn(), &nodal_core::model::Lock { session, ..held }).unwrap();
}

/// One entry of a process table a test writes, in the shape `/proc` publishes.
///
/// `stat` is `pid (comm) state ppid pgrp session ...`, and the reading takes the tail
/// from the last `)`, so the session is the fourth field after it.
#[cfg(target_os = "linux")]
fn table_entry(table: &Path, pid: u32, session: u32, record: Option<&str>) {
    let directory = table.join(pid.to_string());
    std::fs::create_dir_all(&directory).unwrap();
    let Some(record) = record else { return };
    std::fs::write(directory.join("stat"), record.replace("{session}", &session.to_string()))
        .unwrap();
}

/// A process table nobody could list says "I cannot see", and never "it is gone".
///
/// This is the reading the refusal rests on. A `false` here lets a hold go with no
/// `--take`, so a container with no `/proc`, or a table this call could not list, must
/// not produce one: it would hand a live holder's home to the next process that asked.
#[test]
#[cfg(target_os = "linux")]
fn a_process_table_that_cannot_be_listed_is_unreadable_and_never_gone() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("no-proc-here");
    assert_eq!(
        processes::session_is_live_in(&missing, 4_242),
        None,
        "a table that could not be listed answered about a session"
    );
}

/// A record this account may not read says nothing, and nothing is not "gone".
///
/// `hidepid`, and another account's process on a host two people share, both land here.
/// That host is the one the lock exists for, so a reading that called another account's
/// live session gone would hand away a hold nobody let go of.
#[test]
#[cfg(target_os = "linux")]
fn a_record_that_cannot_be_read_leaves_the_answer_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let table = directory.path();
    table_entry(table, 11, 4_242, Some("11 (sh) S 1 11 {session} 0"));
    table_entry(table, 12, 0, Some("nothing this reading can parse"));

    assert_eq!(
        processes::session_is_live_in(table, 4_242),
        Some(true),
        "a session that is in the table was not found"
    );
    assert_eq!(
        processes::session_is_live_in(table, 9_999),
        None,
        "an unreadable record was counted as proof the session had gone"
    );
}

/// A table read all the way through, with the session not in it, is the one `false`.
///
/// A process that ended between the listing and the read is `gone` and not `unknown`.
/// Folding the two together would make "I cannot see" the answer on any busy machine
/// and leave every lapsed hold standing for the whole idle window.
#[test]
#[cfg(target_os = "linux")]
fn a_table_that_was_read_through_says_the_session_has_gone() {
    let directory = tempfile::tempdir().unwrap();
    let table = directory.path();
    table_entry(table, 11, 4_242, Some("11 (sh) S 1 11 {session} 0"));
    // Listed, then gone before its record could be read: ordinary, and not a failure.
    table_entry(table, 12, 0, None);

    assert_eq!(
        processes::session_is_live_in(table, 9_999),
        Some(false),
        "a table that was read through did not answer"
    );
    assert_eq!(
        processes::session_is_live_in(directory.path().join("empty").as_path(), 9_999),
        None,
        "a table that does not exist answered"
    );
}

/// The engineer's own workflow: one command after another, from one shell.
///
/// This is the claim the rule had to keep. Each write verb is a new process with a new
/// identifier and, under job control, a new process group; a rule keyed on either one
/// would refuse the second command. Both hosts pass it, because on a host that cannot
/// read a lineage the name alone still matches.
#[test]
fn the_same_lineage_enters_a_home_it_already_holds() {
    let directory = tempfile::tempdir().unwrap();
    let (store, unit, project) = fixture(directory.path());
    let now = Timestamp::now();

    let first = lock::enter(store.conn(), &unit, &project, false, now).unwrap();
    assert_eq!(first, Entered::Took, "a free home was not taken");

    let later = Timestamp::from_unix_seconds(now.unix_seconds() + 1).unwrap();
    let again = lock::enter(store.conn(), &unit, &project, false, later).unwrap();
    assert_eq!(again, Entered::Refreshed, "the holder was refused its own next command");

    // A refresh that wrote nothing is not a refresh. The idle window is measured from
    // this stamp, so a claim the statement did not match would report a window that
    // moved while leaving the hold to lapse at the old deadline.
    let held = locks::get(store.conn(), unit.id).unwrap().unwrap();
    assert_eq!(held.refreshed_at, later, "the refresh reported a write it did not make");
}

/// A claim whose lineage is not the row's matches nothing, so no entry may build one.
///
/// This is the statement the refresh rests on, asserted on both hosts because it is what
/// a host that cannot read a lineage most depends on: there, every entry falls back, and
/// a claim carrying this process's own session instead of the row's would match nothing
/// and write nothing while the entry reported a refresh.
#[test]
fn a_claim_matches_the_lineage_the_row_records_and_no_other() {
    let directory = tempfile::tempdir().unwrap();
    let (store, unit, project) = fixture(directory.path());
    let now = Timestamp::now();
    lock::enter(store.conn(), &unit, &project, false, now).unwrap();
    recorded_as(&store, unit.id, Some(4_242));

    let held = locks::get(store.conn(), unit.id).unwrap().unwrap();
    let deadline = held.idle_deadline(8);
    let mine = |session| nodal_core::model::Lock { session, ..held.clone() };

    assert!(
        !locks::take(store.conn(), &mine(Some(4_243)), now, deadline).unwrap(),
        "a claim carrying another lineage of the same actor matched the row"
    );
    assert!(
        locks::take(store.conn(), &mine(Some(4_242)), now, deadline).unwrap(),
        "a claim carrying the row's own lineage did not match it"
    );
}

/// A second process of one actor is a second holder, and `--take` is the way through.
#[test]
fn a_second_process_of_one_actor_is_refused_while_its_lineage_is_live() {
    let directory = tempfile::tempdir().unwrap();
    let (store, unit, project) = fixture(directory.path());
    let now = Timestamp::now();
    lock::enter(store.conn(), &unit, &project, false, now).unwrap();

    if !can_see_processes() {
        // The reading is not available, so the rule falls back to the name alone. The
        // claim not being made is the refusal; the claim being made is that nothing
        // here refuses the tool its own use.
        recorded_as(&store, unit.id, Some(4_242));
        let entered = lock::enter(store.conn(), &unit, &project, false, now).unwrap();
        assert_eq!(entered, Entered::Refreshed, "a host that cannot see refused somebody");
        return;
    }

    let other = Elsewhere::started();
    recorded_as(&store, unit.id, Some(other.session()));

    let refused = lock::enter(store.conn(), &unit, &project, false, now).unwrap_err();
    assert!(
        matches!(refused, nodal_core::Error::UnitLocked { .. }),
        "a second process of one actor was let in: {refused}"
    );
    assert!(
        refused.to_string().contains("second process of the same name"),
        "the refusal did not say which of the two holds it: {refused}"
    );

    let taken = lock::enter(store.conn(), &unit, &project, true, now).unwrap();
    assert_eq!(taken, Entered::TakenOver, "--take did not move a hold it was given for");
}

/// A lineage that has gone has let the hold go, and the take is recorded as one.
#[test]
fn a_recorded_lineage_that_has_gone_lets_the_hold_go() {
    let directory = tempfile::tempdir().unwrap();
    let (store, unit, project) = fixture(directory.path());
    let now = Timestamp::now();
    lock::enter(store.conn(), &unit, &project, false, now).unwrap();

    if !can_see_processes() {
        // A host that publishes no process table cannot tell a lineage that has gone
        // from one at work, so it lets nothing go and refuses nobody. The claim made
        // here is the fallback itself: the entry passes as a refresh, and no hand-off is
        // written for a holder this host never read.
        recorded_as(&store, unit.id, Some(4_242));
        let entered = lock::enter(store.conn(), &unit, &project, false, now).unwrap();
        assert_eq!(entered, Entered::Refreshed, "a host that cannot see moved a hold");
        return;
    }

    let mut other = Elsewhere::started();
    let sid = other.session();
    other.end();
    recorded_as(&store, unit.id, Some(sid));

    let taken = lock::enter(store.conn(), &unit, &project, false, now).unwrap();
    assert_eq!(taken, Entered::TakenOver, "a hold whose lineage had gone was not let go");

    let held = locks::get(store.conn(), unit.id).unwrap().unwrap();
    assert_eq!(
        held.session,
        processes::current_session(),
        "the take did not record the lineage that took it"
    );
}
