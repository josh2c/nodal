//! Acceptance test: concurrent writers to one registry lose nothing.
//!
//! The claim this suite defends is the whole reason the registry is a database rather
//! than a file: a shell hook, an agent's note and a command wrapper append to the same
//! log at the same moment, from different processes, and every append that was accepted
//! is still there afterwards — with the content it was written with, exactly once.
//!
//! Each writer opens its own [`Store`], as a separate process would, and they are held
//! at a barrier so the appends genuinely overlap rather than queue up behind each
//! other's start-up. The events are generated from a deterministic pseudo-random source
//! so the corpus is the same on every machine and a failure can be reproduced from the
//! seed printed in the assertion.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Barrier;

use nodal_core::model::{
    Actor, ActorKind, ActorName, BranchName, Digest, Epistemic, Event, EventId, EventKind,
    Objective, Project, ProjectId, ProjectName, RawRef, RefName, Slug, Timestamp, Unit, UnitId,
    UnitStatus,
};
use nodal_core::store::{Store, events, projects, units};
use tempfile::TempDir;

/// The number of appends the acceptance requires to survive.
const APPENDS: u32 = 50;

/// Every way of splitting [`APPENDS`] over two or more writers that divides evenly,
/// plus the extreme of one writer per append. Two writers is the case the task names;
/// fifty is the case the acceptance names.
const SPLITS: &[(u32, u32)] = &[(2, 25), (5, 10), (10, 5), (25, 2), (50, 1)];

/// The alphabet a canonical ULID is written in.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Every event kind, so generated events cover the whole contract.
const KINDS: &[EventKind] = &[
    EventKind::Attached,
    EventKind::Detached,
    EventKind::Command,
    EventKind::Commit,
    EventKind::TestResult,
    EventKind::Failure,
    EventKind::FileTouched,
    EventKind::Finding,
    EventKind::Decision,
    EventKind::Question,
    EventKind::Handoff,
    EventKind::Sync,
    EventKind::Note,
];

/// A deterministic generator, so the corpus is the same on every machine.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A value below `limit`.
    fn below(&mut self, limit: usize) -> usize {
        usize::try_from(self.next() % limit as u64).unwrap()
    }
}

/// The identifier of the `n`th event of a run: a canonical ULID whose low bits count.
///
/// Generating them from the index rather than the clock is what lets the test state the
/// expected set exactly, so a duplicate and a missing row are both failures.
fn event_id(n: u32) -> EventId {
    let mut text = String::from("01J8Z6H00000000000");
    for shift in (0..8).rev() {
        let digit = (u64::from(n) >> (shift * 5)) & 31;
        text.push(char::from(CROCKFORD[usize::try_from(digit).unwrap()]));
    }
    EventId::parse(&text).unwrap()
}

fn ulid(last: char) -> String {
    format!("01J8Z6H000000000000000000{last}")
}

fn project_id() -> ProjectId {
    ulid('1').parse().unwrap()
}

fn unit_id() -> UnitId {
    ulid('2').parse().unwrap()
}

/// The `n`th event of a run, with generated content.
///
/// Timestamps are whole seconds, which is the resolution the registry keeps: an event
/// read back is compared against what was written, so the sample must be in that form.
fn generate(n: u32, random: &mut Xorshift) -> Event {
    let mut refs = BTreeMap::new();
    for _ in 0..random.below(3) {
        let name = format!("ref{}", random.below(9));
        refs.insert(RefName::parse(name).unwrap(), format!("{:x}", random.next()));
    }
    Event {
        id: event_id(n),
        unit: unit_id(),
        environment: None,
        ts: Timestamp::from_unix_seconds(1_788_689_472 + i64::from(n)).unwrap(),
        actor: Actor {
            kind: if random.below(2) == 0 { ActorKind::Human } else { ActorKind::Agent },
            name: ActorName::parse(format!("writer-{}", random.below(7))).unwrap(),
        },
        kind: KINDS[random.below(KINDS.len())],
        epistemic: if random.below(2) == 0 { Epistemic::Observed } else { Epistemic::Stated },
        body: format!("appended {n} with nonce {:x}", random.next()),
        refs,
        raw_ref: if random.below(3) == 0 {
            Some(RawRef::parse(format!("logs/{n}.log")).unwrap())
        } else {
            None
        },
    }
}

/// A registry holding the project and unit the appends belong to.
fn seeded(dir: &Path) -> PathBuf {
    let path = dir.join("registry.db");
    let store = Store::open(&path).unwrap();
    projects::insert(
        store.conn(),
        &Project {
            id: project_id(),
            root: PathBuf::from("/home/dev/acme"),
            name: ProjectName::parse("acme").unwrap(),
            recipe_hash: Digest::parse("0f1e2d").unwrap(),
            created_at: Timestamp::from_unix_seconds(1_788_689_400).unwrap(),
            remote_url: None,
        },
    )
    .unwrap();
    units::insert(
        store.conn(),
        &Unit {
            id: unit_id(),
            project_id: project_id(),
            slug: Slug::parse("fix-worker-import").unwrap(),
            objective: Some(Objective::parse("fix the worker import").unwrap()),
            objective_epistemic: Some(Epistemic::Stated),
            branch: BranchName::parse("nodal/fix-worker-import").unwrap(),
            parent_branch: None,
            base_commit: None,
            status: UnitStatus::Open,
            created_at: Timestamp::from_unix_seconds(1_788_689_400).unwrap(),
            updated_at: Timestamp::from_unix_seconds(1_788_689_400).unwrap(),
        },
    )
    .unwrap();
    path
}

/// Append `writers * per_writer` generated events, one connection per writer, all
/// starting at the same moment, and return what the log holds afterwards next to what
/// was written.
fn append_concurrently(writers: u32, per_writer: u32, seed: u64) -> (Vec<Event>, Vec<Event>) {
    let dir = TempDir::new().unwrap();
    let path = seeded(dir.path());

    let mut random = Xorshift(seed);
    let written: Vec<Event> = (0..writers * per_writer).map(|n| generate(n, &mut random)).collect();

    let barrier = Barrier::new(usize::try_from(writers).unwrap());
    std::thread::scope(|scope| {
        for writer in 0..writers {
            let (path, barrier, written) = (&path, &barrier, &written);
            scope.spawn(move || {
                // Open before the barrier, but wait on it whatever the outcome: a
                // writer that failed to open and never arrived would hang the others
                // rather than fail the test.
                let opened = Store::open(path);
                barrier.wait();
                let store = opened.unwrap_or_else(|error| panic!("writer {writer}: {error}"));
                for index in 0..per_writer {
                    let n = usize::try_from(writer * per_writer + index).unwrap();
                    events::append(store.conn(), &written[n]).unwrap_or_else(|error| {
                        panic!("writer {writer} lost append {index}: {error}")
                    });
                }
            });
        }
    });

    let reader = Store::open(&path).unwrap();
    let stored = events::list_for_unit(reader.conn(), unit_id()).unwrap();
    (written, stored)
}

/// The acceptance: fifty appends, made at the same moment by fifty writers on fifty
/// connections, are all in the log afterwards, each exactly once and unchanged.
#[test]
fn fifty_concurrent_event_appends_lose_nothing() {
    let (written, stored) = append_concurrently(APPENDS, 1, 0x5EED_0001);

    assert_eq!(stored.len(), usize::try_from(APPENDS).unwrap(), "the log is short");
    let expected: BTreeSet<EventId> = written.iter().map(|event| event.id).collect();
    let found: BTreeSet<EventId> = stored.iter().map(|event| event.id).collect();
    assert_eq!(found, expected, "an append was lost or duplicated");

    let mut sorted = written;
    sorted.sort_by_key(|event| event.id);
    assert_eq!(stored, sorted, "an event came back changed, or out of order");
}

/// The same property over every split of the fifty appends across writers, including
/// the two-writer case: the invariant is not a property of one arrangement of threads.
#[test]
fn appends_lose_nothing_however_the_writers_are_split() {
    for (round, (writers, per_writer)) in SPLITS.iter().enumerate() {
        let seed = 0x5EED_0100 + u64::try_from(round).unwrap();
        let (written, stored) = append_concurrently(*writers, *per_writer, seed);
        let expected: BTreeSet<EventId> = written.iter().map(|event| event.id).collect();
        let found: BTreeSet<EventId> = stored.iter().map(|event| event.id).collect();
        assert_eq!(
            found, expected,
            "{writers} writers of {per_writer} appends lost or duplicated a row (seed {seed:#x})"
        );
        let mut sorted = written;
        sorted.sort_by_key(|event| event.id);
        assert_eq!(stored, sorted, "content changed with {writers} writers (seed {seed:#x})");
    }
}

/// Two writers on different tables, at the same moment: neither blocks the other out,
/// and both sets of rows are there afterwards. Writes to a registry are not confined to
/// the log, so the invariant is checked where two operations meet as well.
#[test]
fn two_writers_on_different_tables_both_land() {
    let dir = TempDir::new().unwrap();
    let path = seeded(dir.path());
    let mut random = Xorshift(0x5EED_0200);
    let written: Vec<Event> = (0..APPENDS).map(|n| generate(n, &mut random)).collect();
    let statuses = [UnitStatus::Review, UnitStatus::Merged, UnitStatus::Archived];

    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let (path, barrier, written) = (&path, &barrier, &written);
        scope.spawn(move || {
            let opened = Store::open(path);
            barrier.wait();
            let store = opened.unwrap();
            for event in written {
                events::append(store.conn(), event).unwrap();
            }
        });
        scope.spawn(move || {
            let opened = Store::open(path);
            barrier.wait();
            let store = opened.unwrap();
            for (round, status) in statuses.iter().cycle().take(30).enumerate() {
                let at =
                    Timestamp::from_unix_seconds(1_788_690_000 + i64::try_from(round).unwrap())
                        .unwrap();
                assert!(units::update_status(store.conn(), unit_id(), *status, at).unwrap());
            }
        });
    });

    let reader = Store::open(&path).unwrap();
    assert_eq!(events::count_for_unit(reader.conn(), unit_id()).unwrap(), u64::from(APPENDS));
    let unit = units::get(reader.conn(), unit_id()).unwrap().unwrap();
    assert_eq!(unit.status, UnitStatus::Archived, "the last status written is the one held");
}

/// A reader sees a consistent log while a writer is appending: write-ahead logging is
/// what makes `nodal ls` in one terminal safe while another is recording events.
#[test]
fn a_reader_is_never_blocked_out_by_a_writer() {
    let dir = TempDir::new().unwrap();
    let path = seeded(dir.path());
    let mut random = Xorshift(0x5EED_0300);
    let written: Vec<Event> = (0..APPENDS).map(|n| generate(n, &mut random)).collect();

    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let (path, barrier, written) = (&path, &barrier, &written);
        scope.spawn(move || {
            let opened = Store::open(path);
            barrier.wait();
            let store = opened.unwrap();
            for event in written {
                events::append(store.conn(), event).unwrap();
            }
        });
        scope.spawn(move || {
            let opened = Store::open(path);
            barrier.wait();
            let store = opened.unwrap();
            let mut seen = 0;
            for _ in 0..200 {
                let listed = events::list_for_unit(store.conn(), unit_id()).unwrap();
                assert!(listed.len() >= seen, "the log went backwards");
                seen = listed.len();
            }
        });
    });

    let reader = Store::open(&path).unwrap();
    assert_eq!(events::count_for_unit(reader.conn(), unit_id()).unwrap(), u64::from(APPENDS));
}

/// Fifty processes opening a registry that does not exist yet all get a migrated
/// database and none of them fails: first run of `nodal` in a project is exactly this
/// race when a shell hook and a command start together.
#[test]
fn concurrent_first_opens_all_succeed_and_migrate_once() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("registry.db");
    let barrier = Barrier::new(usize::try_from(APPENDS).unwrap());

    std::thread::scope(|scope| {
        for opener in 0..APPENDS {
            let (path, barrier) = (&path, &barrier);
            scope.spawn(move || {
                barrier.wait();
                let store =
                    Store::open(path).unwrap_or_else(|error| panic!("opener {opener}: {error}"));
                let mode: String =
                    store.conn().query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
                assert_eq!(mode, "wal", "opener {opener} did not get write-ahead logging");
            });
        }
    });

    // Against a registry only one process ever opened, rather than against a number
    // written down here: a migration that adds a table should not have to edit a test
    // about concurrency.
    let alone = TempDir::new().unwrap();
    assert_eq!(
        table_count(&Store::open(&path).unwrap()),
        table_count(&Store::open(alone.path().join("registry.db")).unwrap()),
        "the schema was applied once, not once per opener"
    );
}

/// How many tables a registry has.
fn table_count(store: &Store) -> u32 {
    store
        .conn()
        .query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table'", [], |row| row.get(0))
        .unwrap()
}
