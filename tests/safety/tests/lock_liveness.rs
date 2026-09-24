//! A hold is held until its holder is proven dead.
//!
//! The write lock is the one record that crosses two Linux accounts on one box, so it is
//! the last thing standing between two agents and one home. Everything it refuses rests
//! on one reading: is the actor who took this hold still here. A reading that answers
//! "gone" when it means "I cannot tell" does not merely mis-report — it grants the write
//! it exists to refuse, and two sessions then drive one home.
//!
//! That is what happened. A hold records the process that took it; a refresh writes the
//! refreshing process's identifier and keeps the instant the hold began; and the reading
//! compared the one against the other. So every refreshed hold named a process that had
//! started after the hold, `nodal show` printed "pid N is not on this host any more"
//! while pid N was running, and the refusal told the next actor the home was free.
//!
//! The rule the suite holds to is the safety kernel's: **live unless proven dead**.
//! Proven dead is a reading of this host's table that nothing carries the holder's
//! identity. Nothing else is proof, and nothing else lets a hold go.
//!
//! | property | what a break would look like |
//! |---|---|
//! | an unresolved identity keeps the hold | a holder the reader cannot pin is read as gone, and its home is handed to the next actor |
//! | an unreadable table keeps the hold | a host that publishes nothing reads every holder as gone |
//! | a proven absence is the one thing that releases it | a hold nobody is behind stands for the whole idle window |
//! | a refreshed hold is still its own holder | a hold is read as gone while the process that took it is running |
//! | each ground for a move prints its own line | a hold moves and the record does not say whether a reading or a clock moved it |
//!
//! ## The kernel and the reading
//!
//! Both decisions here are kernels: a function of a reading, not a reading of their own.
//! [`nodal_core::runtime::lock::liveness`] is given a [`Seen`], and
//! [`nodal_core::runtime::lock::lineage`] is given what the table said about a session.
//! So every shape below is stated rather than arranged, and every one of them is
//! asserted on both hosts — no property in this file is skipped for the host it runs on.
//!
//! What is host-specific is the reading itself, and only Linux publishes a seam for it:
//! `/proc` is a directory, so a table can be stated as one
//! ([`nodal_core::runtime::processes::session_is_live_in`]). macOS answers per process
//! through the kernel and has no such seam, so the two shapes that need a stated table
//! say so out loud there ([`nodal_safety::platform::skipped`]) and the reading of the
//! real machine is asserted on both.
//!
//! The three unresolved-identity shapes below are the ones to carry into the adversarial
//! grid (lane C) when it lands: an identifier the reader cannot pin, an identifier proven
//! absent, and a table that could not be read.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test fails by panicking")]

use std::collections::BTreeMap;

use nodal_core::model::{Actor, ActorKind, ActorName, Holding, HostName, Lock, Timestamp, UnitId};
use nodal_core::output::view::{HolderState, Unknowable};
use nodal_core::runtime::lock::{self, Lineage, Seen};
use nodal_core::runtime::processes::{self, Presence, Processes, Running};

/// The unit every hold here is on. Nothing is written, so one identifier does.
const UNIT: &str = "01J9X2K4Q7QW8QG4M2N5B3T6HP";

/// The identifier the holder carries.
const HELD_BY: u32 = 4_120;

/// The lineage the holder was taken from.
const LINEAGE: u32 = 4_100;

/// When the holder's process started, as the row pins it.
///
/// Stated, not read. A pin is compared for equality, so what a test controls is whether
/// the row and the table say one instant or two.
fn began() -> Timestamp {
    Timestamp::from_unix_seconds(1_700_000_000).unwrap()
}

/// A table that answers for identifiers without dating any of them.
///
/// The shape a host gives for a process it will name and will not time: another
/// account's process on macOS. It answers `gone` for an identifier it does not hold.
struct Undated(Vec<u32>);

impl Processes for Undated {
    fn scan(&self) -> nodal_core::Result<Vec<Running>> {
        Ok(Vec::new())
    }

    fn presences(&self, pids: &[u32]) -> nodal_core::Result<BTreeMap<u32, Presence>> {
        Ok(pids
            .iter()
            .map(|pid| {
                let held = self.0.contains(pid);
                let presence =
                    if held { Presence::Running { started_at: None } } else { Presence::Gone };
                (*pid, presence)
            })
            .collect())
    }
}

/// A table that dates every identifier it holds, which is what a host with a clock
/// answers and what an identifier that came round again is told apart by.
struct Dated(Vec<u32>, Timestamp);

impl Processes for Dated {
    fn scan(&self) -> nodal_core::Result<Vec<Running>> {
        Ok(Vec::new())
    }

    fn presences(&self, pids: &[u32]) -> nodal_core::Result<BTreeMap<u32, Presence>> {
        Ok(pids
            .iter()
            .map(|pid| {
                let presence = if self.0.contains(pid) {
                    Presence::Running { started_at: Some(self.1) }
                } else {
                    Presence::Gone
                };
                (*pid, presence)
            })
            .collect())
    }
}

/// A table this host cannot read at all.
struct Unreadable;

impl Processes for Unreadable {
    fn scan(&self) -> nodal_core::Result<Vec<Running>> {
        Err(nodal_core::Error::ProcessScanUnsupported { host: "workstation" })
    }
}

/// The agent every hold here belongs to. A fleet of them all report this name, which is
/// why the hold carries a lineage as well.
fn holder() -> Actor {
    Actor { kind: ActorKind::Agent, name: ActorName::parse("claude-code").unwrap() }
}

/// A hold on this host, by `process`, taken from [`LINEAGE`].
fn held(process: Option<Holding>) -> Lock {
    let taken = began();
    Lock {
        unit_id: UnitId::parse(UNIT).unwrap(),
        host: HostName::current(),
        actor: Some(holder()),
        process,
        session: Some(LINEAGE),
        taken_at: taken,
        refreshed_at: taken,
        expires_at: Timestamp::from_unix_seconds(taken.unix_seconds() + 28_800).unwrap(),
    }
}

/// What this host's reading says about a hold, given the table `table` states.
fn state(lock: &Lock, table: &dyn Processes) -> HolderState {
    let pids: Vec<u32> = lock.process.iter().map(|process| process.pid).collect();
    lock::liveness(lock, &HostName::current(), &Seen::read(table, &pids))
}

// ---------------------------------------------------------------------------
// An identity that cannot be resolved keeps the hold.
// ---------------------------------------------------------------------------

/// A holder the reader cannot pin keeps its hold, in each of the three ways a pin goes
/// missing.
///
/// A process found and not dated, a row that records no instant for the process it
/// names, and a row that records no process at all. In every one of them a process is or
/// may be wearing the identifier and nothing can say whether it is the holder's, so the
/// answer is that nothing was proved. `gone` in any of the three would be a false "the
/// holder is gone" on the strength of a reading nobody could complete.
#[test]
fn a_holder_whose_identity_the_reader_cannot_resolve_keeps_the_hold() {
    let pinned = held(Some(Holding { pid: HELD_BY, started_at: Some(began()) }));
    assert_eq!(
        state(&pinned, &Undated(vec![HELD_BY])),
        HolderState::Unknown { why: Unknowable::Undated },
        "a table that would not date the process read the holder as gone"
    );

    let unpinned = held(Some(Holding { pid: HELD_BY, started_at: None }));
    assert_eq!(
        state(&unpinned, &Dated(vec![HELD_BY], began())),
        HolderState::Unknown { why: Unknowable::Undated },
        "a row written before holds carried a pin read the holder as gone"
    );

    let nothing = held(None);
    assert_eq!(
        state(&nothing, &Dated(vec![HELD_BY], began())),
        HolderState::Unknown { why: Unknowable::NoPid },
        "a row that names no process read its holder as gone"
    );
}

/// A table this host could not read keeps every hold on it.
///
/// "I cannot see" is not "nobody is there". A host that publishes no process table would
/// otherwise read every holder as gone at once, which is the same fault the pin was
/// added to remove, pointing at every unit instead of one.
#[test]
fn a_table_this_host_could_not_read_keeps_the_hold() {
    let lock = held(Some(Holding { pid: HELD_BY, started_at: Some(began()) }));
    assert_eq!(
        state(&lock, &Unreadable),
        HolderState::Unknown { why: Unknowable::NoProcessTable },
        "an unread table read the holder as gone"
    );
}

/// A hold taken on another machine keeps it too: an identifier of this host names
/// nothing there.
#[test]
fn a_hold_taken_on_another_machine_is_never_read_as_gone_here() {
    let elsewhere = Lock {
        host: HostName::parse("desktop").unwrap(),
        ..held(Some(Holding { pid: HELD_BY, started_at: Some(began()) }))
    };
    let pids = vec![HELD_BY];
    let seen = Seen::read(&Dated(Vec::new(), began()), &pids);
    assert_eq!(
        lock::liveness(&elsewhere, &HostName::current(), &seen),
        HolderState::Unknown { why: Unknowable::AnotherHost },
        "a hold from another host was judged by this host's table"
    );
}

// ---------------------------------------------------------------------------
// A proven absence is the one reading that releases it.
// ---------------------------------------------------------------------------

/// A holder the table was read all the way through for, and does not hold, is gone.
///
/// This is the reading that is allowed to contradict the row, and the only one. The
/// second shape is the same proof by a different route: the identifier is there and the
/// process wearing it began at an instant the hold does not name, so it is a stranger.
#[test]
fn a_holder_proven_absent_from_the_table_is_read_as_gone() {
    let lock = held(Some(Holding { pid: HELD_BY, started_at: Some(began()) }));
    assert_eq!(
        state(&lock, &Dated(Vec::new(), began())),
        HolderState::Gone,
        "a table that holds no such process did not prove the holder gone"
    );

    let later = Timestamp::from_unix_seconds(began().unix_seconds() + 86_400).unwrap();
    assert_eq!(
        state(&lock, &Dated(vec![HELD_BY], later)),
        HolderState::Gone,
        "an identifier that came round again was read as the holder"
    );
}

/// The holder's own process, found at its identifier and dated to its pin, is live.
///
/// The pair is the identity: one number and one instant, both sides agreeing. Neither
/// half says it alone.
#[test]
fn the_process_the_hold_pinned_is_read_as_live() {
    let lock = held(Some(Holding { pid: HELD_BY, started_at: Some(began()) }));
    assert_eq!(state(&lock, &Dated(vec![HELD_BY], began())), HolderState::Live);
}

/// The defect, at the reading that had it: a hold refreshed by a later process of its
/// own lineage is still held by that process.
///
/// A refresh keeps `taken_at`, because the hold began when it began, and writes the
/// refreshing process's own identifier, because that is the process to look for now. The
/// reading used to compare the second against the first and call every refreshed hold a
/// recycled identifier. `nodal show` then printed "pid N is not on this host any more"
/// about a process that was running, and the refusal told the next actor to take the
/// home.
///
/// The pin the row carries is the refreshing process's own, so the two sides agree and
/// the holder reads live for as long as it is there.
#[test]
fn a_hold_refreshed_by_a_later_process_of_its_lineage_is_not_read_as_gone() {
    let refreshed_at = Timestamp::from_unix_seconds(began().unix_seconds() + 14_400).unwrap();
    let refreshing = Holding { pid: HELD_BY + 1, started_at: Some(refreshed_at) };
    // The hold began where it began; the process to look for is the one that refreshed.
    let lock = Lock { process: Some(refreshing), refreshed_at, ..held(None) };

    assert_eq!(
        state(&lock, &Dated(vec![HELD_BY + 1], refreshed_at)),
        HolderState::Live,
        "a refreshed hold was read as gone while the process that refreshed it was running"
    );
}

// ---------------------------------------------------------------------------
// The lineage the refusal rests on.
// ---------------------------------------------------------------------------

/// The decision that moves a hold reads gone on one answer and no other.
///
/// This is the kernel the refusal rests on, given each of the three answers a reading of
/// this host can carry. Only "the whole table was read and no process is in that session"
/// frees the hold. "A process is" refuses a second lineage, and "the reading could not be
/// taken" refuses nobody and frees nobody.
#[test]
fn a_lineage_is_freed_only_by_a_reading_that_proves_it_gone() {
    let lock = held(Some(Holding { pid: HELD_BY, started_at: Some(began()) }));
    let elsewhere = Some(LINEAGE + 1);

    assert_eq!(lock::lineage(&lock, Some(LINEAGE), Some(false)), Lineage::Same, "own lineage");
    assert_eq!(lock::lineage(&lock, elsewhere, Some(true)), Lineage::Second);
    assert_eq!(lock::lineage(&lock, elsewhere, Some(false)), Lineage::Gone);
    assert_eq!(
        lock::lineage(&lock, elsewhere, None),
        Lineage::Unreadable,
        "a reading that could not be taken freed a lineage"
    );
}

/// A record that states no lineage frees nobody, whatever the table says.
///
/// A row written before holds carried a lineage, and a host that will not say which
/// session this process is in, both state nothing. A `false` read against a number
/// nobody wrote down would free a hold on the strength of a match with an absence.
#[test]
fn an_unstated_lineage_is_never_proven_gone() {
    let unstated = Lock { session: None, ..held(None) };
    for read in [Some(true), Some(false), None] {
        assert_eq!(
            lock::lineage(&unstated, Some(LINEAGE), read),
            Lineage::Unreadable,
            "a row that states no lineage was freed by a reading of {read:?}"
        );
        assert_eq!(
            lock::lineage(&held(None), None, read),
            Lineage::Unreadable,
            "a process that states no lineage freed a hold on a reading of {read:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The reading itself, on the host that publishes a seam for it.
// ---------------------------------------------------------------------------

/// The claim only one host publishes a seam for: a process table stated by a test.
#[cfg(not(target_os = "linux"))]
const STATED_TABLE: &str = "a session read against a process table a test states; macOS answers \
                            per process through the kernel and publishes no directory to state";

/// This process is in a session this host says holds a process.
///
/// The reading of the real machine, asserted on both hosts. It is the other half of the
/// kernel above: the kernel decides, and this says the reading it is given is taken.
#[test]
fn this_hosts_own_reading_finds_the_session_this_process_is_in() {
    let session = processes::current_session().expect("this host says which session this is");
    assert_eq!(
        processes::session_is_live(session),
        Some(true),
        "this host read its own live session as gone"
    );
}

/// A table that cannot be listed, and a table holding a record that cannot be read,
/// answer "I cannot see" and never "it is gone".
#[cfg(target_os = "linux")]
#[test]
fn a_table_that_cannot_be_read_says_so_rather_than_gone() {
    let missing = tempfile::tempdir().unwrap();
    let path = missing.path().join("no-such-table");
    assert_eq!(
        processes::session_is_live_in(&path, LINEAGE),
        None,
        "a table that could not be listed was read as an empty one"
    );

    // A table with one record this account cannot read: the shape `hidepid` and another
    // account's process both give. Nothing else in it matches, so a `false` here would be
    // a table that was not read all the way through answering as if it had been.
    //
    // The record is present and says nothing a reader can use, which is what a refused
    // one amounts to: `hidepid` leaves the directory and takes the contents away. A
    // record that is simply not there is a different answer — a process that ended
    // between the listing and the read — and says nothing about any session.
    let hidden = tempfile::tempdir().unwrap();
    std::fs::create_dir(hidden.path().join("7")).unwrap();
    std::fs::write(hidden.path().join("7").join("stat"), "").unwrap();
    assert_eq!(
        processes::session_is_live_in(hidden.path(), LINEAGE),
        None,
        "a record that could not be read was counted as a session that is not there"
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn a_table_that_cannot_be_read_says_so_rather_than_gone() {
    assert!(nodal_safety::platform::skipped(STATED_TABLE, "this host publishes no /proc"));
}

/// A table that was read all the way through, and holds the session, answers that it is
/// there; one that does not hold it answers that it is not.
#[cfg(target_os = "linux")]
#[test]
fn a_table_read_all_the_way_through_answers_for_the_session_it_holds() {
    let table = tempfile::tempdir().unwrap();
    let record = |pid: u32, session: u32| {
        let directory = table.path().join(pid.to_string());
        std::fs::create_dir(&directory).unwrap();
        // `stat`: pid, (comm), state, ppid, pgrp, session, and the rest, which this
        // reading does not look at.
        std::fs::write(directory.join("stat"), format!("{pid} (sh) S 1 {pid} {session} 0 0\n"))
            .unwrap();
    };

    record(11, LINEAGE + 1);
    assert_eq!(
        processes::session_is_live_in(table.path(), LINEAGE),
        Some(false),
        "a table read all the way through did not answer that the session had gone"
    );

    record(12, LINEAGE);
    assert_eq!(
        processes::session_is_live_in(table.path(), LINEAGE),
        Some(true),
        "a session holding a process was read as gone"
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn a_table_read_all_the_way_through_answers_for_the_session_it_holds() {
    assert!(nodal_safety::platform::skipped(STATED_TABLE, "this host publishes no /proc"));
}
