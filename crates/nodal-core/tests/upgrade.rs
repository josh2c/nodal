//! Upgrading a registry an older Nodal wrote, from every schema version there has been.
//!
//! Version drift has two directions and Nodal answers them differently. A database from
//! a *later* Nodal is refused, because this binary does not know what is in it
//! (`Error::StoreTooNew`, asserted in `store.rs`). A database from an *earlier* Nodal is
//! migrated forward, and this file is what says the forward direction keeps the rows.
//!
//! The registry migrates on open, so every upgrade a person ever runs is one of the
//! paths below, and a bad step loses their units silently. There is one frozen fixture
//! per schema version — a committed file holding the schema that version produced and
//! the rows a Nodal of that version held (`frozen/`) — and every test here opens all of
//! them with today's binary.
//!
//! What "the rows survive" means is the whole subject. Not that the migration ran, and
//! not that the tables are still there: that a unit is still that unit, with its handle,
//! its branch and its status; that an event keeps the kind it was written under; that
//! the trash keeps its verdict and what the verdict rested on. Each test below reads one
//! subject back through today's readers, out of every version, and knows for each
//! version what that version could hold: a column a later migration added is expected
//! empty in the files that predate it, and expected filled in the files that do not.
//!
//! Two guards keep the fixtures honest, because a fixture that follows the code proves
//! nothing: a fixture must record the version its name states, and its schema must be
//! the schema that version's migrations produce. The second one fails when a migration
//! that has already shipped is edited, which the append-only rule forbids and nothing
//! else notices.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod frozen;

use std::path::PathBuf;

use nodal_core::lifecycle::journal;
use nodal_core::model::{
    ActorKind, BranchName, EnvState, Epistemic, EventKind, Outside, ResourceKey, Rested, Slug,
    Timestamp, Unit, UnitStatus,
};
use nodal_core::store::{
    SCHEMA_VERSION, Store, bases, environments, events, leases, locks, port_allocations,
    port_blocks, projects, sessions, templates, trash, units,
};
use tempfile::TempDir;

use crate::frozen::rows::{
    BASE, COMMIT, ENVIRONMENT, EVENT, OPERATION, PORT, PROJECT, PRUNED, RECLAIMED, SESSION,
    SUFFIXED, TEMPLATE, TRASHED, UNIT, VERDICT,
};

/// Every version there has ever been, which is every upgrade a person can run.
fn versions() -> impl Iterator<Item = u32> {
    1..=SCHEMA_VERSION
}

/// A frozen fixture, opened with today's binary, which is what migrates it.
///
/// The directory comes back with the store because it is what the file lives in: a
/// store whose directory has been dropped is a store with no registry behind it.
fn opened(version: u32) -> (TempDir, Store) {
    let directory = TempDir::new().unwrap();
    let path = frozen::registry(directory.path(), version);
    let store = Store::open(&path)
        .unwrap_or_else(|why| panic!("a version {version} registry does not open: {why}"));
    (directory, store)
}

/// An instant, as the fixtures write them.
fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_seconds(seconds).unwrap()
}

// ---------------------------------------------------------------------------
// The fixtures themselves: complete, and frozen.
// ---------------------------------------------------------------------------

/// Every schema version has a fixture, and every fixture has a schema version.
///
/// The first half is the coverage claim: a migration appended without a fixture leaves
/// its own upgrade path untested, and this is what says so. The second half catches a
/// fixture left behind by a version that was never released.
#[test]
fn every_schema_version_has_a_frozen_fixture() {
    assert_eq!(
        frozen::committed(),
        versions().collect::<Vec<u32>>(),
        "a fixture is committed for exactly the versions there are"
    );
}

/// A fixture starts from the migration its name states.
///
/// The guard the fixtures cannot do without. A file named for version 9 that stamps 8
/// would start its upgrade one migration early and pass every assertion below while
/// testing a path nobody takes. `frozen::registry` reads the version back out of the
/// file it just laid down, so this holds for every test in this file as well.
#[test]
fn a_fixture_records_the_version_it_starts_from() {
    for version in versions() {
        let directory = TempDir::new().unwrap();
        let path = frozen::registry(directory.path(), version);
        assert_eq!(frozen::version_of(&path), version);
    }
}

/// A fixture holds the schema its version's migrations produce.
///
/// Migrations are append-only, and nothing enforced it. An edit to a migration that has
/// already shipped changes what a person's registry crossed years ago without changing
/// any frozen file, and this is the test that fails when that happens: the committed
/// schema of version N and the schema migrations one to N produce today are compared
/// statement by statement.
#[test]
fn a_fixture_holds_the_schema_its_version_produced() {
    for version in versions() {
        let directory = TempDir::new().unwrap();
        let path = frozen::registry(directory.path(), version);
        let frozen_schema = frozen::schema_of(&rusqlite::Connection::open(&path).unwrap());

        assert_eq!(
            frozen_schema,
            frozen::schema_of(&frozen::replayed(version)),
            "the frozen schema of version {version} is not what its migrations now produce, \
             so a migration that had already shipped was edited"
        );
    }
}

/// Opening a fixture of any version brings the file to the current version.
#[test]
fn every_frozen_fixture_reaches_the_current_schema() {
    for version in versions() {
        let directory = TempDir::new().unwrap();
        let path = frozen::registry(directory.path(), version);

        Store::open(&path).unwrap();

        assert_eq!(
            frozen::version_of(&path),
            SCHEMA_VERSION,
            "a version {version} registry did not reach the current schema"
        );
    }
}

// ---------------------------------------------------------------------------
// What the rows mean afterwards.
// ---------------------------------------------------------------------------

/// The project is the project, and the remote a later migration gave it is kept.
#[test]
fn the_project_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let project =
            projects::get(store.conn(), PROJECT.parse().unwrap()).unwrap().expect("the project");

        assert_eq!(project.root, PathBuf::from("/home/dev/acme"), "version {version}");
        assert_eq!(project.name.as_str(), "acme", "version {version}");
        assert_eq!(project.recipe_hash.as_str(), "0f1e2d", "version {version}");
        assert_eq!(project.created_at, at(1_788_688_800), "version {version}");
        // Migration 9 added the remote. There is no back-fill in SQL: a project written
        // before it keeps the checkout path as its identity until a checkout is read.
        let remote = project.remote_url.as_ref().map(ToString::to_string);
        assert_eq!(remote.as_deref(), (version >= 9).then_some("github.com/acme/acme"));
    }
}

/// The unit is the unit: its handle, its branch, its status and its objective.
#[test]
fn the_unit_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let unit = units::get(store.conn(), UNIT.parse().unwrap()).unwrap().expect("the unit");

        assert_eq!(unit.slug.as_str(), "fix-worker-import", "version {version}");
        assert_eq!(unit.branch.as_str(), "nodal/fix-worker-import", "version {version}");
        assert_eq!(unit.parent_branch.as_ref().map(BranchName::as_str), Some("main"));
        assert_eq!(unit.status, UnitStatus::Open, "version {version}");
        assert_eq!(
            unit.objective.as_ref().map(ToString::to_string).as_deref(),
            Some("fix the worker import")
        );
        assert_eq!(unit.created_at, at(1_788_688_860), "version {version}");
        // Migration 10 added the commit a unit forked from, and invented none for the
        // rows already there: the value cannot be recovered, so it stays unknown.
        assert_eq!(
            unit.base_commit.as_ref().map(ToString::to_string).as_deref(),
            (version >= 10).then_some(COMMIT),
            "version {version}"
        );
    }
}

/// An objective a version before six held is stated afterwards.
///
/// Migration 6 works out a value rather than leaving it for a reader to guess: stating
/// an objective was the only way to give a unit one, so every unit that carries one
/// carries a stated one.
#[test]
fn an_objective_older_than_its_epistemic_column_is_stated() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let unit = units::get(store.conn(), UNIT.parse().unwrap()).unwrap().expect("the unit");

        assert_eq!(unit.objective_epistemic, Some(Epistemic::Stated), "version {version}");
    }
}

/// The home is the home: where it is, what it is warm for, and the ports it holds.
#[test]
fn the_home_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let home = environments::get(store.conn(), ENVIRONMENT.parse().unwrap())
            .unwrap()
            .expect("the environment");

        assert_eq!(home.home, PathBuf::from("/home/dev/.nodal/acme/e/01J8Z6H0"), "v{version}");
        assert!(home.managed, "version {version}");
        assert_eq!(home.base_id.map(|id| id.to_string()).as_deref(), Some(BASE), "v{version}");
        assert_eq!(home.state, EnvState::Stopped, "version {version}");
        assert_eq!(home.fixed_port, Some(5432), "version {version}");
        assert_eq!(home.ports.0.values().copied().collect::<Vec<u16>>(), vec![PORT], "v{version}");
    }
}

/// The base is the base, and what built it is kept where it was recorded.
#[test]
fn the_base_and_the_template_of_every_version_survive() {
    for version in versions() {
        let (_directory, store) = opened(version);
        let conn = store.conn();

        let base = bases::get(conn, BASE.parse().unwrap()).unwrap().expect("the base");
        assert_eq!(base.commit.as_str(), COMMIT, "version {version}");
        assert_eq!(base.ws_fingerprint.0.as_str(), "7c1e9a2b", "version {version}");
        // Migration 12 added the provenance, and a base built before it has none. That
        // is the one thing a stale base cannot be asked about, not a base to rebuild.
        let built_by = base.provenance.as_ref().map(|by| by.nodal_version.to_string());
        assert_eq!(built_by.as_deref(), (version >= 12).then_some("0.1.0-rc.1"), "v{version}");

        let template = templates::get(conn, TEMPLATE.parse().unwrap()).unwrap().expect("template");
        assert_eq!(template.db_name.as_str(), "acme_t_41bd", "version {version}");
    }
}

/// The session is the session, and the group a tether put it in is kept.
#[test]
fn the_session_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let open = sessions::list_open(store.conn(), ENVIRONMENT.parse().unwrap()).unwrap();

        assert_eq!(open.len(), 1, "the session of a version {version} registry is still open");
        assert_eq!(open[0].id.to_string(), SESSION, "version {version}");
        assert_eq!(open[0].actor.kind, ActorKind::Agent, "version {version}");
        assert_eq!(open[0].actor.name.as_str(), "claude-code", "version {version}");
        assert_eq!(open[0].pid, Some(4242), "version {version}");
        // Migration 5 added the process group. A column it added is empty, not wrong.
        assert_eq!(open[0].pgid, (version >= 5).then_some(4240), "version {version}");
    }
}

/// Every event kind a version could write keeps its kind, its words and its order.
///
/// The log is the record a person is promised, so this is per kind and not per count: a
/// migration that rewrote the table — and migration 16 rewrites it, to widen the check
/// constraint — has to carry every row across with the kind it was written under.
#[test]
fn every_event_kind_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let log = events::list_for_unit(store.conn(), UNIT.parse().unwrap()).unwrap();

        assert_eq!(
            log.iter().map(|event| event.kind).collect::<Vec<EventKind>>(),
            kinds_of_version_one(),
            "a version {version} registry lost or changed a kind"
        );
        let commit = log.iter().find(|event| event.id.to_string() == EVENT).expect("the commit");
        assert_eq!(commit.body, "wired the worker import", "version {version}");
        assert_eq!(commit.epistemic, Epistemic::Observed, "version {version}");
        assert_eq!(commit.refs.values().next().map(String::as_str), Some(COMMIT), "v{version}");
        assert_eq!(commit.environment.map(|id| id.to_string()).as_deref(), Some(ENVIRONMENT));
    }
}

/// The kinds version 1 had, in the order the fixtures' identifiers put them in.
fn kinds_of_version_one() -> Vec<EventKind> {
    vec![
        EventKind::Commit,
        EventKind::Attached,
        EventKind::Detached,
        EventKind::Command,
        EventKind::TestResult,
        EventKind::Failure,
        EventKind::FileTouched,
        EventKind::Finding,
        EventKind::Decision,
        EventKind::Question,
        EventKind::Handoff,
        EventKind::Sync,
        EventKind::Note,
    ]
}

/// A verdict written under the kind migration 16 added is still a verdict.
#[test]
fn the_verdict_event_of_every_version_that_had_one_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let log = events::list_for_unit(store.conn(), RECLAIMED.parse().unwrap()).unwrap();

        if version < 16 {
            assert!(log.is_empty(), "version {version} had no kind to write a verdict under");
            continue;
        }
        assert_eq!(log.len(), 1, "version {version}");
        assert_eq!(log[0].id.to_string(), VERDICT, "version {version}");
        assert_eq!(log[0].kind, EventKind::Verdict, "version {version}");
    }
}

/// The trash keeps where the home went, what the prune dropped, and its verdict.
#[test]
fn the_trash_row_of_every_version_keeps_its_verdict() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let entry = trash::get(store.conn(), TRASHED.parse().unwrap()).unwrap();

        // Migration 4 made the trash. A version before it holds no such row at all.
        let Some(entry) = entry else {
            assert!(version < 4, "version {version} lost the row in its trash");
            continue;
        };
        assert_eq!(entry.unit_id.to_string(), RECLAIMED, "version {version}");
        assert_eq!(entry.slug.as_str(), "worker-import", "version {version}");
        assert_eq!(entry.path, PathBuf::from("/home/dev/.nodal/acme/trash/01J8Z6H1"), "v{version}");
        assert_eq!(entry.expires_at, at(1_789_898_560), "version {version}");
        // Migration 8 added what the prune dropped, and zero is the honest figure for a
        // home trashed before Nodal pruned anything.
        assert_eq!(entry.pruned_bytes, if version >= 8 { PRUNED } else { 0 }, "v{version}");
        assert_eq!(entry.rested, rested_at(version), "version {version}");
    }
}

/// The verdict a version's reclaim could write down.
///
/// Migration 15 added it. A row written before it says nothing, and nothing is what it
/// reads as: `Unrecorded` is a third answer and not a claim that the home was safe, so
/// `gc` reads the home again rather than believing a row that never recorded a verdict.
fn rested_at(version: u32) -> Rested {
    if version < 15 {
        return Rested::Unrecorded;
    }
    Rested::Safe {
        copies: vec![Outside {
            repository: PathBuf::from("/home/dev/acme"),
            references: vec!["refs/remotes/origin/nodal/worker-import".to_owned()],
            commits: 3,
        }],
    }
}

/// The ports a project handed out, and the block they came from.
#[test]
fn the_ports_of_every_version_survive() {
    for version in versions() {
        let (_directory, store) = opened(version);
        let conn = store.conn();

        let block = port_blocks::get(conn, PROJECT.parse().unwrap()).unwrap();
        let held =
            port_allocations::list_for_environment(conn, ENVIRONMENT.parse().unwrap()).unwrap();

        // Migration 3 made both tables, so a version before it holds neither row.
        if version < 3 {
            assert!(block.is_none() && held.is_empty(), "version {version}");
            continue;
        }
        let block = block.expect("the project's block");
        assert_eq!((block.first, block.last), (20_000, 20_009), "version {version}");
        assert_eq!(held.len(), 1, "version {version}");
        assert_eq!(held[0].port, PORT, "version {version}");
        assert_eq!(held[0].name.as_str(), "app", "version {version}");
    }
}

/// The lease on the pinned port is still held by the home that took it.
#[test]
fn the_lease_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let lease = leases::get(store.conn(), &ResourceKey::parse("port:5432").unwrap())
            .unwrap()
            .expect("the lease");

        assert_eq!(lease.environment_id.to_string(), ENVIRONMENT, "version {version}");
        assert_eq!(lease.expires_at, at(1_788_692_700), "version {version}");
    }
}

/// The lock names the host it always named, and whoever the row could name since.
#[test]
fn the_lock_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);

        let lock = locks::get(store.conn(), UNIT.parse().unwrap()).unwrap().expect("the lock");

        assert_eq!(lock.host.as_str(), "laptop", "version {version}");
        assert_eq!(lock.expires_at, at(1_788_692_700), "version {version}");
        // Migration 11 added the actor. A row that records nobody refuses nobody, and
        // its idle clocks default to the epoch, so the first read expires the hold
        // rather than inventing one that never happened.
        let actor = lock.actor.as_ref().map(|actor| actor.name.to_string());
        assert_eq!(actor.as_deref(), (version >= 11).then_some("claude-code"), "v{version}");
        assert_eq!(lock.taken_at, at(if version >= 11 { 1_788_689_100 } else { 0 }), "v{version}");
        // Migrations 14 and 17: the lineage the hold belongs to, and the pin that makes
        // a reading of the table a proof about the process that took it.
        assert_eq!(lock.session, (version >= 14).then_some(4200), "version {version}");
        assert_eq!(lock.pid(), (version >= 11).then_some(4242), "version {version}");
        assert_eq!(
            lock.process.and_then(|held| held.started_at),
            (version >= 17).then(|| at(1_788_689_090)),
            "version {version}"
        );
    }
}

/// The journal keeps the run that made the unit, its steps, and what they produced.
#[test]
fn the_journal_of_every_version_survives() {
    for version in versions() {
        let (_directory, store) = opened(version);
        let conn = store.conn();

        let run = journal::get(conn, OPERATION.parse().unwrap()).unwrap();

        // Migration 2 made the journal.
        let Some(run) = run else {
            assert!(version < 2, "version {version} lost the record of a run");
            continue;
        };
        assert_eq!(run.kind, "new", "version {version}");
        assert_eq!(run.subject, "fix-worker-import", "version {version}");
        assert_eq!(run.state, journal::State::Committed, "version {version}");
        let steps = journal::steps(conn, OPERATION.parse().unwrap()).unwrap();
        assert_eq!(steps.len(), 2, "version {version}");
        assert_eq!(steps[1].key, "materialize", "version {version}");
        // Migration 7 added what a step produced, which is what a rebuilt plan reads.
        assert_eq!(steps[1].output.is_some(), version >= 7, "version {version}");
    }
}

// ---------------------------------------------------------------------------
// Migration 13: a handle is unique among the units that hold one.
// ---------------------------------------------------------------------------

/// The unit a person makes after the migration, under the name that is free again.
const REMADE: &str = "01J8Z6H0000000000000000010";

/// The rows a registry held while the rule was stricter are kept, and nothing is
/// renamed to relax it.
///
/// The reclaimed unit keeps `worker-import`, and the unit beside it keeps the suffix
/// the old rule pushed it onto. Every version is checked, not only the one before the
/// migration: the rows cross it from wherever they start.
#[test]
fn the_units_a_registry_held_keep_their_handles() {
    for version in versions() {
        let (_directory, store) = opened(version);
        let conn = store.conn();

        let reclaimed =
            units::get(conn, RECLAIMED.parse().unwrap()).unwrap().expect("the archived unit");
        assert_eq!(reclaimed.slug.as_str(), "worker-import", "version {version}");
        assert_eq!(reclaimed.status, UnitStatus::Archived, "version {version}");
        assert_eq!(reclaimed.branch.as_str(), "nodal/worker-import", "version {version}");

        let suffixed =
            units::get(conn, SUFFIXED.parse().unwrap()).unwrap().expect("the suffixed unit");
        assert_eq!(suffixed.slug.as_str(), "worker-import-2", "version {version}");
    }
}

/// A unit of the fixture's project, for a row written after the migration.
fn made(id: &str, slug: &str, branch: &str) -> Unit {
    let at = Timestamp::parse("2026-09-06T11:00:00Z").unwrap();
    Unit {
        id: id.parse().unwrap(),
        project_id: PROJECT.parse().unwrap(),
        slug: Slug::parse(slug).unwrap(),
        objective: None,
        objective_epistemic: None,
        branch: BranchName::parse(branch).unwrap(),
        parent_branch: None,
        base_commit: None,
        status: UnitStatus::Open,
        created_at: at,
        updated_at: at,
    }
}

/// The name an archived unit carries is free for a new unit, and two units hold one
/// handle only when one of them has given it up.
///
/// The suffixed unit is reclaimed first, which is what a person does with the row the
/// old rule left them. Both names are then free, and the one they wanted is taken.
#[test]
fn a_name_an_archived_unit_carries_is_free_after_the_migration() {
    let (_directory, store) = opened(12);
    let now = Timestamp::parse("2026-09-06T11:00:00Z").unwrap();
    units::update_status(store.conn(), SUFFIXED.parse().unwrap(), UnitStatus::Archived, now)
        .unwrap();

    units::insert(store.conn(), &made(REMADE, "worker-import", "nodal/worker-import"))
        .expect("the archived unit holds no handle, so the name is free");

    // Two rows now carry `worker-import`, and the one that holds it is the open one.
    let found = units::find_by_slug(
        store.conn(),
        PROJECT.parse().unwrap(),
        &Slug::parse("worker-import").unwrap(),
    )
    .unwrap()
    .expect("the name reaches a unit");
    assert_eq!(found.id.to_string(), REMADE, "the unit that holds the handle answers first");

    // The rule that is left still holds: two units cannot hold one handle.
    let second = made("01J8Z6H0000000000000000013", "worker-import", "nodal/worker-import-again");
    assert!(
        units::insert(store.conn(), &second).is_err(),
        "a handle is unique among the units that hold one"
    );
}

// ---------------------------------------------------------------------------
// Opening again.
// ---------------------------------------------------------------------------

/// Migrating a registry twice is migrating it once.
#[test]
fn migrating_a_registry_twice_is_migrating_it_once() {
    for version in versions() {
        let directory = TempDir::new().unwrap();
        let path = frozen::registry(directory.path(), version);

        Store::open(&path).unwrap();
        let store = Store::open(&path).unwrap();

        let log = events::list_for_unit(store.conn(), UNIT.parse().unwrap()).unwrap();
        assert_eq!(log.len(), 13, "a second open of a version {version} registry repeated a step");
        assert_eq!(frozen::version_of(&path), SCHEMA_VERSION, "version {version}");
    }
}

// ---------------------------------------------------------------------------
// Writing a fixture for a new version.
// ---------------------------------------------------------------------------

/// Write a fixture for any version that has none, and leave every other file alone.
///
/// Ignored, because it is not an assertion: it is the one command that produces a
/// fixture, run by the person who appends a migration, after they have written what
/// that version can hold into `frozen/rows.rs`.
///
/// ```text
/// cargo test -p nodal-core --test upgrade -- --ignored the_frozen_fixtures
/// ```
#[test]
#[ignore = "writes into the repository; run it when a migration is appended"]
fn the_frozen_fixtures_are_written_for_a_version_that_has_none() {
    let written: Vec<PathBuf> = versions().filter_map(frozen::make::write).collect();
    for file in &written {
        println!("wrote {}", file.display());
    }
    assert_eq!(frozen::committed(), versions().collect::<Vec<u32>>());
}
