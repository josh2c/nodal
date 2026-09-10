//! Acceptance for the output layer: snapshot tests for both renderers.
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

use nodal_core::git::integration::{Divergence, Integration, Reason};
use nodal_core::model::{
    Actor, ActorKind, ActorName, Base, BaseId, BranchName, CommitId, Digest, EnvId, EnvName,
    EnvState, Epistemic, Event, EventId, EventKind, FingerprintPart, HostName, Missing, Objective,
    Platform, PortName, Ports, ProjectId, ProjectName, Slug, Timestamp, UnitId, UnitStatus, Want,
    WorkspaceFp,
};
use nodal_core::output::view::verdict::{Behind, RowKind, Verdict, WorktreeRow};
use nodal_core::output::view::{
    Arrival, BaseList, BaseRow, Created, Done, EnvLine, EventLog, Exclusion, Explained, Freshness,
    InitReport, Invalidation, Origin, PortLine, Ps, Remote, Running, SharedResource, StandInLine,
    Status, ToolSessions, UnitDetail, UnitList, UnitRow, WorkTree,
};
use nodal_core::output::{Format, Render, render, watch};
use nodal_core::recipe::gap::{Gap, GapKey};
use nodal_core::runtime::attribute::{Attributed, Confidence, Kind, Note, Source};
use nodal_core::workspace::tracked::Kept;

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

fn tool(name: &str, count: u32) -> ToolSessions {
    ToolSessions { tool: ActorName::parse(name).expect("an actor name"), count }
}

/// The revision every row in these snapshots is measured against.
fn base() -> String {
    String::from("refs/remotes/origin/main")
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
            objective_epistemic: Some(Epistemic::Stated),
            freshness: Freshness::Fresh,
            work: Some(WorkTree {
                dirty: 3,
                staged: 1,
                untracked: 2,
                detached: false,
                base: base(),
                main: Divergence { ahead: 2, behind: 0 },
                integration: Integration::Open,
                remote: Some(Remote {
                    upstream: String::from("origin/nodal/worker-import"),
                    divergence: Divergence { ahead: 2, behind: 0 },
                }),
            }),
            environment: Some(EnvLine {
                id: env_id("01J9X2K4Q7QW8QG4M2N5B3T6HQ"),
                home: PathBuf::from("/home/j/.nodal/project/e/01J9X2K4"),
                state: EnvState::Stopped,
                managed: true,
                disk_bytes: Some(287_000_000),
                ports: ports(&[("app", 41_230)]),
                running: Vec::new(),
            }),
            created_at: at("2026-09-04T09:15:00Z"),
            sessions: vec![tool("claude-code", 1)],
            last_active: Some(at("2026-09-06T14:10:00Z")),
        },
        UnitRow {
            id: unit_id("01J9X3M8ZK4P2R7V9N6TQW3B5H"),
            slug: Slug::parse("payroll-export").expect("a slug"),
            status: UnitStatus::Open,
            branch: BranchName::parse("nodal/payroll-export").expect("a branch"),
            objective: Some(Objective::parse("payroll export CSV").expect("one line")),
            // Recovered rather than stated: this is the unit `nodal adopt` made of a
            // worktree another tool left behind, and every rendering has to say so.
            objective_epistemic: Some(Epistemic::Observed),
            freshness: Freshness::Stale(vec![
                FingerprintPart::Dependencies,
                FingerprintPart::Schema,
            ]),
            work: Some(WorkTree {
                dirty: 0,
                staged: 0,
                untracked: 0,
                detached: false,
                base: base(),
                main: Divergence { ahead: 1, behind: 4 },
                integration: Integration::Open,
                remote: None,
            }),
            environment: Some(EnvLine {
                id: env_id("01J9X3M8ZK4P2R7V9N6TQW3B5J"),
                home: PathBuf::from("/home/j/.nodal/project/e/01J9X3M8"),
                state: EnvState::Running,
                managed: true,
                disk_bytes: Some(301_000_000),
                ports: ports(&[("app", 41_231), ("postgrest", 54_401)]),
                running: vec![Running { command: String::from("next dev"), port: Some(41_231) }],
            }),
            created_at: at("2026-09-06T09:40:00Z"),
            sessions: vec![tool("codex", 1), tool("josh", 1)],
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
        objective_epistemic: None,
        freshness: Freshness::Unknown,
        work: None,
        environment: None,
        created_at: at("2026-09-05T08:00:00Z"),
        sessions: Vec::new(),
        last_active: None,
    }
}

/// One row per integration verdict, so every word the `main` column can print is in a
/// snapshot: a branch merged, a branch squash-merged, a branch that would conflict, and
/// a home whose HEAD names a commit rather than a branch.
fn integration_units() -> Vec<UnitRow> {
    let shapes = [
        ("merged-plain", Integration::Integrated(Reason::Ancestor), 0, 4, false),
        ("squash-landed", Integration::Integrated(Reason::Absorbed), 1, 2, false),
        ("clashes", Integration::Conflict, 1, 4, false),
        ("detached-head", Integration::Unknown, 0, 0, true),
    ];
    shapes
        .into_iter()
        .enumerate()
        .map(|(index, (slug, integration, ahead, behind, detached))| {
            let mut row = unmeasured_unit();
            row.id = unit_id(&format!("01J9X4P00000000000000000{index}0"));
            row.slug = Slug::parse(slug).expect("a slug");
            row.branch = BranchName::parse(format!("nodal/{slug}")).expect("a branch");
            row.status = UnitStatus::Open;
            row.work = Some(WorkTree {
                dirty: 0,
                staged: 0,
                untracked: 0,
                detached,
                base: base(),
                main: Divergence { ahead, behind },
                integration,
                remote: None,
            });
            row
        })
        .collect()
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
        worktrees: Vec::new(),
        notes: Vec::new(),
    };
    both("unit_list", &list);
}

#[test]
fn every_integration_verdict_renders_both_ways() {
    let list = UnitList {
        project: ProjectName::parse("project").expect("one line"),
        now: now(),
        units: integration_units(),
        worktrees: Vec::new(),
        notes: vec![String::from("who: a process scan reads /proc, which mac does not have")],
    };
    both("unit_list_integration", &list);
}

#[test]
fn a_unit_nothing_has_been_measured_about_still_renders_a_whole_row() {
    let list = UnitList {
        project: ProjectName::parse("project").expect("one line"),
        now: now(),
        units: vec![unmeasured_unit()],
        worktrees: Vec::new(),
        notes: Vec::new(),
    };
    both("unit_list_unmeasured", &list);
}

/// The worktrees of a checkout, one of each shape the table has a column for.
fn found() -> Vec<WorktreeRow> {
    vec![
        WorktreeRow {
            kind: RowKind::Worktree,
            name: String::from("../oauth-login"),
            path: PathBuf::from("/home/j/code/oauth-login"),
            branch: Some(String::from("oauth-login")),
            intent: Some(String::from(
                "Add oauth login to the account page and keep the old form working for the                  accounts that already use it",
            )),
            done: Integration::Open,
            unpushed: 3,
            uncommitted: 2,
            behind: Some(Behind {
                commits: 12,
                reference: String::from("origin/main"),
                upstream: true,
            }),
            bytes: Some(7_850_000_000),
            partial: false,
            made_at: Some(Timestamp::parse("2026-08-14T09:00:00Z").expect("a fixed instant")),
            note: None,
        },
        WorktreeRow {
            kind: RowKind::Worktree,
            name: String::from(".claude/worktrees/finished"),
            path: PathBuf::from("/home/j/code/project/.claude/worktrees/finished"),
            branch: Some(String::from("finished")),
            intent: None,
            done: Integration::Integrated(Reason::Absorbed),
            unpushed: 0,
            uncommitted: 0,
            behind: Some(Behind {
                commits: 40,
                reference: String::from("origin/main"),
                upstream: true,
            }),
            bytes: Some(1_200_000_000),
            partial: true,
            made_at: Some(Timestamp::parse("2026-07-02T11:30:00Z").expect("a fixed instant")),
            note: None,
        },
        WorktreeRow {
            kind: RowKind::Worktree,
            name: String::from("/var/tmp/agent-held"),
            path: PathBuf::from("/var/tmp/agent-held"),
            branch: None,
            intent: None,
            done: Integration::Unknown,
            unpushed: 0,
            uncommitted: 0,
            behind: None,
            bytes: None,
            partial: false,
            made_at: None,
            note: Some(String::from("locked (an agent is working here)")),
        },
    ]
}

#[test]
fn the_verdict_on_a_checkout_renders_both_ways() {
    let seen = Verdict {
        checkout: PathBuf::from("/home/j/code/project"),
        project: None,
        now: now(),
        base: Some(String::from("origin/main")),
        rows: found(),
        notes: Vec::new(),
    };
    both("verdict", &seen);
}

#[test]
fn a_checkout_with_no_other_worktrees_says_so_and_still_makes_the_promise() {
    let seen = Verdict {
        checkout: PathBuf::from("/home/j/code/project"),
        project: None,
        now: now(),
        base: Some(String::from("main")),
        rows: Vec::new(),
        notes: Vec::new(),
    };
    both("verdict_empty", &seen);
}

/// A registered project whose repository also names worktrees Nodal did not make.
///
/// The leading column is the whole point of the snapshot: every row says which kind it
/// is, and no folder Nodal did not make appears under the word `unit`.
#[test]
fn a_project_holding_both_units_and_worktrees_prints_one_table() {
    let list = UnitList {
        project: ProjectName::parse("project").expect("one line"),
        now: now(),
        units: units(),
        worktrees: found(),
        notes: Vec::new(),
    };
    both("unit_list_with_worktrees", &list);
}

#[test]
fn an_empty_list_says_so_rather_than_printing_a_bare_heading() {
    let list = UnitList {
        project: ProjectName::parse("project").expect("one line"),
        now: now(),
        units: Vec::new(),
        worktrees: Vec::new(),
        notes: Vec::new(),
    };
    both("unit_list_empty", &list);
}

/// The two declared names a fixture home has no value for.
fn missing() -> Vec<Missing> {
    [("DATABASE_URL", Want::RequiredLocal), ("STRIPE_KEY", Want::Secret)]
        .into_iter()
        .map(|(name, want)| Missing { name: EnvName::parse(name).expect("an env name"), want })
        .collect()
}

/// What `nodal new` answers with: the fields of the home it made, and no sentence under
/// them. Nothing here happened to a directory of the person's.
///
/// The kept row is on this one because a create is the form that copies: a default
/// exclusion row that yielded to what the project tracks is a fact about a copy, and an
/// adoption in place makes none.
#[test]
fn a_created_unit_renders_both_ways() {
    let unit = units().swap_remove(0);
    let created = Created {
        now: now(),
        arrival: Arrival::Created,
        unit,
        missing: missing(),
        kept: vec![Kept {
            path: PathBuf::from("test-results"),
            reason: String::from("test output"),
        }],
        stand_ins: vec![
            EnvName::parse("REDIS_URL").expect("an env name"),
            EnvName::parse("SUPABASE_URL").expect("an env name"),
        ],
    };
    both("created", &created);
}

/// What `nodal adopt --in-place` answers with.
///
/// The report closes with a sentence, because the field reported that it closed with a
/// list of env names under a column heading and nothing that said what the list was or
/// what had just happened to the checkout.
#[test]
fn an_adopted_unit_closes_with_a_summary_of_what_happened() {
    let mut unit = units().swap_remove(1);
    if let Some(environment) = unit.environment.as_mut() {
        environment.managed = false;
        environment.home = PathBuf::from("/home/j/code/app/.claude/worktrees/payroll");
    }
    let adopted = Created {
        now: now(),
        arrival: Arrival::AdoptedInPlace,
        unit,
        missing: missing(),
        kept: Vec::new(),
        stand_ins: Vec::new(),
    };
    both("created_adopted", &adopted);
}

/// An adoption that had every value it needed still says what it did, and says that.
#[test]
fn an_adoption_with_nothing_missing_still_says_what_it_did() {
    let unit = units().swap_remove(0);
    let adopted = Created {
        now: now(),
        arrival: Arrival::Adopted,
        unit,
        missing: Vec::new(),
        kept: Vec::new(),
        stand_ins: Vec::new(),
    };
    both("created_adopted_complete", &adopted);
}

#[test]
fn unit_detail_renders_both_ways() {
    let detail = UnitDetail { now: now(), unit: units().swap_remove(0), history: history() };
    both("unit_detail", &detail);
}

/// The other shape of a unit: a checkout adopted where it stood, whose objective was
/// read out of a session record rather than stated, and whose home Nodal must never
/// move. Every one of those three facts has to be on the page.
#[test]
fn the_detail_of_an_adopted_unit_says_it_is_one() {
    let mut unit = units().swap_remove(1);
    if let Some(environment) = unit.environment.as_mut() {
        environment.managed = false;
        environment.home = PathBuf::from("/home/j/code/app/.claude/worktrees/payroll");
    }
    let detail = UnitDetail { now: now(), unit, history: Vec::new() };
    both("unit_detail_adopted", &detail);
}

/// A unit whose home was cloned from a base: the whole of what `nodal explain` has to
/// answer — which tree, why that one, what the clone left out, what was taken out of the
/// copy afterwards, and where the ports came from.
#[test]
fn an_explanation_of_a_cloned_home_renders_both_ways() {
    let explained = Explained {
        now: now(),
        slug: Slug::parse("worker-import").expect("a slug"),
        home: Some(PathBuf::from("/home/j/.nodal/project/e/01J9X2K4")),
        origin: Origin::Cloned {
            base: "01J9W0000000000000000BASE1".parse().expect("a canonical ULID"),
            path: PathBuf::from("/home/j/.nodal/project/b/01J9W000"),
            fingerprint: WorkspaceFp(digest(&"a1".repeat(32))),
            platform: Platform::parse("aarch64-apple-darwin").expect("a target triple"),
            commit: CommitId::parse("f".repeat(40)).expect("an object id"),
            built_at: at("2026-09-06T09:00:00Z"),
        },
        excluded: vec![
            Exclusion {
                path: String::from(".claude/worktrees"),
                reason: String::from("checkouts another tool made, which the clone would multiply"),
                decided_by: String::from("nodal"),
            },
            Exclusion {
                path: String::from("var/log"),
                reason: String::from("named by the project in base.exclude"),
                decided_by: String::from("project"),
            },
        ],
        invalidated: vec![Invalidation {
            at: at("2026-09-06T09:02:00Z"),
            removed: String::from("2"),
            from: String::from("/home/j/.nodal/project/b/01J9W000"),
            body: String::from("Removed 2 caches from the new home."),
        }],
        stand_ins: Some(vec![StandInLine {
            name: String::from("REDIS_URL"),
            source: String::from(
                "nodal, at create: no adapter produced it, so the value is derived from \
                 the unit's handle and a port of the block 41230\u{2013}41329",
            ),
        }]),
        ports: vec![PortLine {
            name: String::from("app"),
            port: 41_230,
            source: String::from("the block 41230-41329 this project was granted"),
        }],
    };
    both("explanation", &explained);
}

/// The other origin: a checkout adopted where it stood. Nothing was copied, so the two
/// sections about copying have nothing in them and each says why rather than being
/// left blank.
#[test]
fn an_explanation_of_an_adopted_checkout_renders_both_ways() {
    let explained = Explained {
        now: now(),
        slug: Slug::parse("payroll-export").expect("a slug"),
        home: Some(PathBuf::from("/home/j/code/app/.claude/worktrees/payroll")),
        origin: Origin::Adopted { root: true },
        excluded: Vec::new(),
        invalidated: Vec::new(),
        stand_ins: Some(Vec::new()),
        ports: vec![PortLine {
            name: String::from("app"),
            port: 41_231,
            source: String::from("the block 41230-41329 this project was granted"),
        }],
    };
    both("explanation_adopted", &explained);
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

#[test]
fn a_pushed_unit_renders_both_ways() {
    let report = Done {
        now: now(),
        slug: Slug::parse("worker-import").expect("a slug"),
        branch: BranchName::parse("nodal/worker-import").expect("a branch"),
        remote: String::from("origin"),
        host: Some(String::from("github.com")),
        pushed: vec![
            String::from("refs/heads/nodal/worker-import"),
            String::from("refs/nodal/01ARZ3NDEKTSV4RRFFQ69G5FAV/wip"),
        ],
        snapshot: Some(String::from("refs/nodal/01ARZ3NDEKTSV4RRFFQ69G5FAV/wip")),
        compare: Some(String::from(
            "https://github.com/team/project/compare/nodal/worker-import?expand=1",
        )),
        status: UnitStatus::Review,
        notes: Vec::new(),
    };
    both("done", &report);
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
        "created.json",
        "created.txt",
        "created_adopted.json",
        "created_adopted.txt",
        "created_adopted_complete.json",
        "created_adopted_complete.txt",
        "done.json",
        "done.txt",
        "event_log.json",
        "event_log.txt",
        "explanation.json",
        "explanation.txt",
        "explanation_adopted.json",
        "explanation_adopted.txt",
        "init_report.json",
        "init_report.txt",
        "ps.json",
        "ps.txt",
        "ps_unreadable.json",
        "ps_unreadable.txt",
        "status.json",
        "status.txt",
        "status_stream.ndjson",
        "unit_detail.json",
        "unit_detail.txt",
        "unit_detail_adopted.json",
        "unit_detail_adopted.txt",
        "unit_list.json",
        "unit_list.txt",
        "unit_list_empty.json",
        "unit_list_empty.txt",
        "unit_list_integration.json",
        "unit_list_integration.txt",
        "unit_list_unmeasured.json",
        "unit_list_unmeasured.txt",
        "unit_list_with_worktrees.json",
        "unit_list_with_worktrees.txt",
        "verdict.json",
        "verdict.txt",
        "verdict_empty.json",
        "verdict_empty.txt",
    ];
    let directory = snapshot_dir();
    let mut found: Vec<String> = std::fs::read_dir(&directory)
        .expect("the snapshot directory exists")
        .map(|entry| entry.expect("a readable entry").file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(found, expected, "{}: an orphaned or missing snapshot", directory.display());
}

/// Every kind of row and both confidences, so a column that moves shows up as a diff.
fn ps() -> Ps {
    let unit = unit_id("01J9X2K4Q7QW8QG4M2N5B3T6HP");
    let environment = env_id("01J9X2K4Q7QW8QG4M2N5B3T6HQ");
    let slug = Slug::parse("worker-import").expect("a slug");
    let row = |kind, what: &str, pid, port, confidence, signal| Attributed {
        unit,
        slug: slug.clone(),
        environment,
        kind,
        what: String::from(what),
        pid,
        port,
        confidence,
        signal,
    };
    Ps {
        now: now(),
        host: HostName::parse("laptop").expect("a host name"),
        rows: vec![
            row(
                Kind::Process,
                "next dev",
                Some(8_812),
                None,
                Confidence::Certain,
                Source::Environment,
            ),
            row(Kind::Process, "vim", Some(8_940), None, Confidence::Probable, Source::Cwd),
            row(
                Kind::Container,
                "nodal-worker-import-db",
                None,
                None,
                Confidence::Certain,
                Source::Docker,
            ),
            row(Kind::Listener, "app", None, Some(41_230), Confidence::Probable, Source::Listener),
        ],
        notes: vec![Note::new(Source::Docker, "docker is not installed")],
    }
}

#[test]
fn ps_renders_both_ways() {
    both("ps", &ps());
}

/// A machine where every signal is there and nothing is running is not the same answer
/// as a machine that could not be read, and the two must not print alike.
///
/// The notes are the two a mac really reports: one process table cannot be read, and
/// both signals that needed it went quiet for that one reason. The JSON keeps both,
/// because a tool asks which signals it lost. The page prints the reason once and names
/// them, because a person reading the same sentence twice reads it as two faults.
#[test]
fn a_ps_with_nothing_running_says_so_and_still_prints_its_notes() {
    let why = "a process scan reads /proc, which macos does not have";
    let quiet = Ps {
        now: now(),
        host: HostName::parse("laptop").expect("a host name"),
        rows: Vec::new(),
        notes: vec![Note::new(Source::Environment, why), Note::new(Source::Cwd, why)],
    };
    both("ps_unreadable", &quiet);
}
