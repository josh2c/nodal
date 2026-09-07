//! Acceptance for the output layer (T0.7): snapshot tests for both renderers.
//!
//! Every read type is built from fixed values — fixed identifiers, fixed instants, a
//! fixed `now` — and rendered twice: once as text for a person, once as JSON for a tool.
//! Both renderings are compared against a file in `tests/snapshots/`. A change to a
//! column, a label, a field name or a JSON key therefore shows up in review as a diff of
//! the output itself rather than as a diff of the code that produces it, which is the
//! point: `--json` is a surface other tools read.
//!
//! The stream is snapshotted the same way. `status --watch` polls, so the suite drives
//! it with a fixed list of answers and a zero interval and compares the NDJSON it wrote,
//! including the answer that repeats and is deliberately not written twice.
//!
//! Run with `UPDATE_SNAPSHOTS=1` to rewrite the files after a deliberate change.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use nodal_core::model::{
    Actor, ActorKind, ActorName, Base, BaseId, BranchName, CommitId, Digest, EnvId, EnvState,
    Epistemic, Event, EventId, EventKind, FingerprintPart, HostName, Objective, Platform, PortName,
    Ports, ProjectId, ProjectName, Slug, Timestamp, UnitId, UnitStatus, WorkspaceFp,
};
use nodal_core::output::view::{
    BaseList, BaseRow, EnvLine, EventLog, Freshness, InitReport, Running, SharedResource, Status,
    UnitDetail, UnitList, UnitRow, WorkTree,
};
use nodal_core::output::{Format, Render, render, watch};
use nodal_core::recipe::gap::{Gap, GapKey};

// ---------------------------------------------------------------- fixed values

/// The instant every relative time in these snapshots is measured from.
fn now() -> Timestamp {
    at("2026-09-06T14:22:00Z")
}

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("a fixed instant")
}

fn unit_id(text: &str) -> UnitId {
    UnitId::parse(text).expect("a canonical ULID")
}

fn env_id(text: &str) -> EnvId {
    EnvId::parse(text).expect("a canonical ULID")
}

fn digest(text: &str) -> Digest {
    Digest::parse(text).expect("a lowercase hex digest")
}

fn ports(entries: &[(&str, u16)]) -> Ports {
    let mut map = BTreeMap::new();
    for (name, port) in entries {
        map.insert(PortName::parse(*name).expect("a port name"), *port);
    }
    Ports(map)
}

/// Two units of one project: one that is fresh and idle, one that is stale and running.
/// Between them they exercise every cell the table can print, and the second is the
/// unit `docs/scenarios.md` §7 shows going stale under an open branch.
fn units() -> Vec<UnitRow> {
    vec![
        UnitRow {
            id: unit_id("01J9X2K4Q7QW8QG4M2N5B3T6HP"),
            slug: Slug::parse("worker-import").expect("a slug"),
            status: UnitStatus::Open,
            branch: BranchName::parse("nodal/worker-import").expect("a branch"),
            objective: Some(
                Objective::parse("worker import: handle missing supervisor_id").expect("one line"),
            ),
            freshness: Freshness::Fresh,
            work: Some(WorkTree { ahead: 2, behind: 0, uncommitted: 3 }),
            environment: Some(EnvLine {
                id: env_id("01J9X2K4Q7QW8QG4M2N5B3T6HQ"),
                home: PathBuf::from("/home/j/.nodal/project/e/01J9X2K4"),
                state: EnvState::Stopped,
                managed: true,
                disk_bytes: Some(287_000_000),
                ports: ports(&[("app", 41_230)]),
                running: Vec::new(),
            }),
            last_active: Some(at("2026-09-06T14:10:00Z")),
        },
        UnitRow {
            id: unit_id("01J9X3M8ZK4P2R7V9N6TQW3B5H"),
            slug: Slug::parse("payroll-export").expect("a slug"),
            status: UnitStatus::Open,
            branch: BranchName::parse("nodal/payroll-export").expect("a branch"),
            objective: Some(Objective::parse("payroll export CSV").expect("one line")),
            freshness: Freshness::Stale(vec![
                FingerprintPart::Dependencies,
                FingerprintPart::Schema,
            ]),
            work: Some(WorkTree { ahead: 0, behind: 4, uncommitted: 0 }),
            environment: Some(EnvLine {
                id: env_id("01J9X3M8ZK4P2R7V9N6TQW3B5J"),
                home: PathBuf::from("/home/j/.nodal/project/e/01J9X3M8"),
                state: EnvState::Running,
                managed: true,
                disk_bytes: Some(301_000_000),
                ports: ports(&[("app", 41_231), ("postgrest", 54_401)]),
                running: vec![Running { command: String::from("next dev"), port: Some(41_231) }],
            }),
            last_active: Some(at("2026-09-06T14:21:40Z")),
        },
    ]
}

/// A unit with nothing computed about it: no environment, no Git answer, no staleness.
/// It is here because that is the state every unit is in before the tasks that measure
/// those things exist, and the renderer must still print a whole row.
fn unmeasured_unit() -> UnitRow {
    UnitRow {
        id: unit_id("01J9X4P0000000000000000AAB"),
        slug: Slug::parse("auth-refresh").expect("a slug"),
        status: UnitStatus::Review,
        branch: BranchName::parse("feature/auth-refresh").expect("a branch"),
        objective: None,
        freshness: Freshness::Unknown,
        work: None,
        environment: None,
        last_active: None,
    }
}

fn history() -> Vec<Event> {
    vec![
        Event {
            id: EventId::parse("01J9X2K4Q7QW8QG4M2N5B3T700").expect("a canonical ULID"),
            unit: unit_id("01J9X2K4Q7QW8QG4M2N5B3T6HP"),
            environment: Some(env_id("01J9X2K4Q7QW8QG4M2N5B3T6HQ")),
            ts: at("2026-09-06T14:02:00Z"),
            actor: Actor {
                kind: ActorKind::Agent,
                name: ActorName::parse("claude-code").expect("an actor name"),
            },
            kind: EventKind::TestResult,
            epistemic: Epistemic::Observed,
            body: String::from("pnpm test: 0 failing"),
            refs: BTreeMap::new(),
            raw_ref: None,
        },
        Event {
            id: EventId::parse("01J9X2K4Q7QW8QG4M2N5B3T701").expect("a canonical ULID"),
            unit: unit_id("01J9X2K4Q7QW8QG4M2N5B3T6HP"),
            environment: Some(env_id("01J9X2K4Q7QW8QG4M2N5B3T6HQ")),
            ts: at("2026-09-06T14:10:00Z"),
            actor: Actor {
                kind: ActorKind::Human,
                name: ActorName::parse("j2c").expect("an actor name"),
            },
            kind: EventKind::Handoff,
            epistemic: Epistemic::Stated,
            body: String::from("legacy date parser still fails on two-digit years"),
            refs: BTreeMap::new(),
            raw_ref: None,
        },
    ]
}

fn status() -> Status {
    Status {
        now: now(),
        project: ProjectName::parse("project").expect("one line"),
        host: HostName::parse("laptop").expect("a host name"),
        units: units(),
        shared: vec![
            SharedResource {
                name: String::from("supabase stack"),
                detail: Some(String::from("10 containers")),
                disk_bytes: None,
            },
            SharedResource {
                name: String::from("pnpm store"),
                detail: None,
                disk_bytes: Some(2_400_000_000),
            },
        ],
    }
}

fn base_list() -> BaseList {
    BaseList {
        now: now(),
        bases: vec![BaseRow {
            base: Base {
                id: BaseId::parse("01J9X0000000000000000000AB").expect("a canonical ULID"),
                project_id: ProjectId::parse("01J9X0000000000000000000AA")
                    .expect("a canonical ULID"),
                ws_fingerprint: WorkspaceFp(digest("7f3ea1c0d4")),
                platform: Platform::parse("x86_64-unknown-linux-gnu").expect("a triple"),
                commit: CommitId::parse("9f1d2c3b4a5968778695a4b3c2d1e0f102030405")
                    .expect("an object id"),
                path: PathBuf::from("/home/j/.nodal/project/base/7f3e"),
                built_at: at("2026-09-05T09:00:00Z"),
                last_used: at("2026-09-06T13:22:00Z"),
            },
            pins: 1,
            disk_bytes: Some(1_160_000_000),
        }],
    }
}

fn init_report() -> InitReport {
    InitReport {
        path: PathBuf::from("/home/j/code/project/nodal.toml"),
        existed: false,
        contents: String::from("package_manager = \"pnpm\"\n"),
        gaps: vec![
            Gap::new(GapKey::Toolchain),
            Gap::new(GapKey::EnvRequiredLocal).note("45 names declared"),
        ],
    }
}

// ------------------------------------------------------------------ snapshots

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

/// Compare against the committed file, or rewrite it when `UPDATE_SNAPSHOTS` is set.
fn check(name: &str, actual: &str) {
    let path = snapshot_dir().join(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the snapshot directory is writable");
        }
        std::fs::write(&path, actual).expect("the snapshot is writable");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{}: {error}\nrun the suite with UPDATE_SNAPSHOTS=1 to create it", path.display())
    });
    assert_eq!(
        actual,
        expected,
        "{} is out of date; rerun with UPDATE_SNAPSHOTS=1 if the change is deliberate",
        path.display()
    );
}

/// Render one read type both ways and compare both against their files. The two
/// renderings are checked together because they are two views of one value: a read type
/// that gains a field must show the change in both, or in neither and say why.
fn both<T: Render>(name: &str, value: &T) {
    check(
        &format!("{name}.txt"),
        &render(value, Format::Human).expect("the human rendering never fails"),
    );
    check(&format!("{name}.json"), &render(value, Format::Json).expect("the value encodes"));
}

#[test]
fn unit_list_renders_both_ways() {
    let list = UnitList {
        project: ProjectName::parse("project").expect("one line"),
        now: now(),
        units: units(),
    };
    both("unit_list", &list);
}

#[test]
fn a_unit_nothing_has_been_measured_about_still_renders_a_whole_row() {
    let list = UnitList {
        project: ProjectName::parse("project").expect("one line"),
        now: now(),
        units: vec![unmeasured_unit()],
    };
    both("unit_list_unmeasured", &list);
}

#[test]
fn an_empty_list_says_so_rather_than_printing_a_bare_heading() {
    let list = UnitList {
        project: ProjectName::parse("project").expect("one line"),
        now: now(),
        units: Vec::new(),
    };
    both("unit_list_empty", &list);
}

#[test]
fn unit_detail_renders_both_ways() {
    let detail = UnitDetail { now: now(), unit: units().swap_remove(0), history: history() };
    both("unit_detail", &detail);
}

#[test]
fn event_log_renders_both_ways() {
    let log = EventLog {
        now: now(),
        unit: Some(Slug::parse("worker-import").expect("a slug")),
        events: history(),
    };
    both("event_log", &log);
}

#[test]
fn base_list_renders_both_ways() {
    both("base_list", &base_list());
}

#[test]
fn status_renders_both_ways() {
    both("status", &status());
}

#[test]
fn init_report_renders_both_ways() {
    both("init_report", &init_report());
}

// --------------------------------------------------------------- the stream

/// A source that answers from a fixed list, which is what makes the stream's output a
/// snapshot rather than a timing-dependent capture.
struct Answers {
    answers: Vec<Status>,
    next: usize,
}

impl watch::Source for Answers {
    type Item = Status;

    fn poll(&mut self) -> nodal_core::Result<Status> {
        let answer = self.answers.get(self.next).cloned().unwrap_or_else(status);
        self.next += 1;
        Ok(answer)
    }
}

#[test]
fn the_status_stream_is_one_document_per_line_and_repeats_nothing() {
    let first = status();
    // The same answer, taken two seconds later. The clock moved; the state did not, so
    // this must not reach the wire — otherwise polling would emit a frame every tick.
    let mut ticked = status();
    ticked.now = at("2026-09-06T14:22:02Z");
    // A unit went away. That is news.
    let mut moved = status();
    moved.now = at("2026-09-06T14:22:04Z");
    moved.units.swap_remove(1);
    let answers = vec![first, ticked, moved];

    let mut source = Answers { answers: answers.clone(), next: 0 };
    let mut out = Vec::new();
    let options = watch::Options {
        interval: Duration::ZERO,
        max_polls: Some(answers.len().try_into().expect("three fits in a u64")),
    };
    let report = watch::run(&mut source, &mut out, options).expect("the fixed source answers");

    assert_eq!(report.polls, 3, "every tick asks");
    assert_eq!(report.frames, 2, "only the clock moving is not news");
    let text = String::from_utf8(out).expect("NDJSON is UTF-8");
    for line in text.lines() {
        serde_json::from_str::<serde_json::Value>(line).expect("every line is one document");
    }
    check("status_stream.ndjson", &text);
}

/// The stream and `--json` must stay the same document, or a consumer cannot use one
/// reader for both. Compared as values, since one is pretty and the other is compact.
#[test]
fn a_frame_is_the_same_document_as_json_output() {
    let status = status();
    let pretty: serde_json::Value =
        serde_json::from_str(&render(&status, Format::Json).expect("the value encodes"))
            .expect("valid JSON");
    let mut stream = watch::Stream::new();
    let frame = stream.frame(&status).expect("the value encodes").expect("the first frame");
    let compact: serde_json::Value = serde_json::from_str(&frame).expect("valid JSON");
    assert_eq!(pretty, compact);
}

/// Snapshot files are only useful if nothing else writes them. This is the guard: a
/// renamed test that leaves its old file behind, or a file no test compares, fails here.
/// It is skipped while snapshots are being rewritten, because the other tests are then
/// creating the very files it counts.
#[test]
fn every_snapshot_file_is_claimed_by_a_test() {
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        return;
    }
    let expected: Vec<&str> = vec![
        "base_list.json",
        "base_list.txt",
        "event_log.json",
        "event_log.txt",
        "init_report.json",
        "init_report.txt",
        "status.json",
        "status.txt",
        "status_stream.ndjson",
        "unit_detail.json",
        "unit_detail.txt",
        "unit_list.json",
        "unit_list.txt",
        "unit_list_empty.json",
        "unit_list_empty.txt",
        "unit_list_unmeasured.json",
        "unit_list_unmeasured.txt",
    ];
    let directory = snapshot_dir();
    let mut found: Vec<String> = std::fs::read_dir(&directory)
        .expect("the snapshot directory exists")
        .map(|entry| entry.expect("a readable entry").file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(found, expected, "{}: an orphaned or missing snapshot", directory.display());
}
