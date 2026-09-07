//! Acceptance for the list (T1.11b): `nodal ls` over real repositories.
//!
//! The fixture builds one origin repository whose branches are the shapes real work is
//! in — a branch that never left the base, a branch with work on it, a branch the base
//! moved under, a branch merged, a branch squashed onto the base, and a branch that
//! would conflict — and one clone per unit, the way a home is made. Every claim below
//! is read out of those repositories rather than out of a fake.
//!
//! Four claims:
//!
//! 1. Every shape gets the verdict the fixture states, the squash merge included, which
//!    is the one no reading of the commit history finds.
//! 2. The list counts the working tree by side: changed, staged and untracked.
//! 3. The list puts the units the base has moved under first, and the finished ones
//!    last.
//! 4. A signal that cannot run is a note, and a home that is not there is a note, and
//!    neither stops the other rows from printing.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nodal_core::git::Git;
use nodal_core::model::{
    BranchName, Digest, EnvId, EnvState, Environment, HostName, Ports, Project, ProjectId,
    ProjectName, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use nodal_core::runtime::ls;
use nodal_core::runtime::processes::{Processes, Running};
use nodal_core::store::{Store, environments, projects, units};
use nodal_fixture::shapes::{self, Branch};
use tempfile::TempDir;

/// The revision every home is measured against.
const BASE: &str = "refs/remotes/origin/main";

/// A process table a test supplies instead of a machine.
struct Table(Vec<Running>);

impl Processes for Table {
    fn scan(&self) -> nodal_core::Result<Vec<Running>> {
        Ok(self.0.clone())
    }
}

/// A process table this host cannot read.
struct Unreadable;

impl Processes for Unreadable {
    fn scan(&self) -> nodal_core::Result<Vec<Running>> {
        Err(nodal_core::Error::ProcessScanUnsupported { host: "workstation" })
    }
}

fn host() -> HostName {
    HostName::parse("workstation").unwrap()
}

fn running(pid: u32, unit: &UnitId, home: &Path, pairs: &[(&str, &str)]) -> Running {
    let mut vars: BTreeMap<String, String> =
        pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
    vars.insert(String::from("NODAL_ID"), unit.to_string());
    vars.insert(String::from("NODAL_ROOT"), home.display().to_string());
    Running::new(pid, vars)
}

/// A project whose units are one clone per shape.
struct Fixture {
    /// The directory everything is under; removed when the test ends.
    directory: TempDir,
    /// The registry.
    store: Store,
    /// The project row.
    project: Project,
    /// The unit of each branch, by branch name.
    units: BTreeMap<&'static str, (UnitId, PathBuf)>,
}

impl Fixture {
    /// One unit per shape, each in a clone of the origin repository.
    fn build() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let origin = shapes::origin(directory.path().join("origin"));
        let now = Timestamp::now();
        let project = Project {
            id: ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
            root: origin.clone(),
            name: ProjectName::parse("fixture").unwrap(),
            recipe_hash: Digest::parse("0".repeat(64)).unwrap(),
            created_at: now,
        };
        let store = Store::open(directory.path().join("registry.db")).unwrap();
        projects::insert(store.conn(), &project).unwrap();
        let mut units = BTreeMap::new();
        for (index, branch) in shapes::BRANCHES.iter().enumerate() {
            let home = shapes::home(
                &origin,
                directory.path().join("homes").join(branch.name),
                branch.name,
            );
            let id =
                record(&store, &project, &Row { branch: branch.name, index, home: &home }, now);
            units.insert(branch.name, (id, home));
        }
        Self { directory, store, project, units }
    }

    /// The list, as the producer answers it.
    fn list(&self, processes: &dyn Processes) -> nodal_core::output::view::UnitList {
        ls::list(self.store.conn(), processes, &self.project, Timestamp::now()).unwrap()
    }

    /// The home of one shape.
    fn home(&self, branch: &str) -> &Path {
        &self.units[branch].1
    }
}

/// One unit of the fixture: which shape it is, which one of them, and where it lives.
struct Row<'a> {
    /// The branch of the origin repository the home is on.
    branch: &'static str,
    /// Which unit this is, so identifiers stay fixed.
    index: usize,
    /// The clone the unit is materialised in.
    home: &'a Path,
}

/// Record a unit and its home, and return the unit's identity.
fn record(store: &Store, project: &Project, row: &Row<'_>, now: Timestamp) -> UnitId {
    let (branch, index, home) = (row.branch, row.index, row.home);
    let unit = Unit {
        id: UnitId::parse(format!("01ARZ3NDEKTSV4RRFFQ69G5F{index:02}")).unwrap(),
        project_id: project.id,
        slug: Slug::parse(branch).unwrap(),
        objective: None,
        branch: BranchName::parse(branch).unwrap(),
        parent_branch: None,
        status: UnitStatus::Open,
        created_at: now,
        updated_at: now,
    };
    let environment = Environment {
        id: EnvId::parse(format!("01ARZ3NDEKTSV4RRFFQ69G5E{index:02}")).unwrap(),
        unit_id: unit.id,
        attempt: 1,
        home: home.to_path_buf(),
        managed: true,
        base_id: None,
        ws_fp_materialized: None,
        schema_fp_materialized: None,
        host: host(),
        db_name: None,
        ports: Ports::default(),
        fixed_port: None,
        state: EnvState::Stopped,
        created_at: now,
        last_active: now,
    };
    units::insert(store.conn(), &unit).unwrap();
    environments::insert(store.conn(), &environment).unwrap();
    unit.id
}

/// The verdict a branch's clone gets, read straight from the Git facade.
fn verdict(home: &Path) -> String {
    Git::open(home).unwrap().standing(BASE).unwrap().integration.label()
}

#[test]
fn every_shape_gets_the_verdict_the_fixture_states() {
    let fixture = Fixture::build();
    for Branch { name, verdict: expected, behind } in shapes::BRANCHES.iter().copied() {
        let home = fixture.home(name);
        assert_eq!(verdict(home), expected, "{name}");
        let standing = Git::open(home).unwrap().standing(BASE).unwrap();
        assert_eq!(standing.divergence.is_behind(), behind, "{name} behind");
    }
    drop(fixture.directory);
}

#[test]
fn a_squash_merge_counts_as_done_although_no_commit_of_it_is_on_the_base() {
    let fixture = Fixture::build();
    let home = fixture.home("squashed");
    let standing = Git::open(home).unwrap().standing(BASE).unwrap();

    assert!(standing.integration.is_integrated(), "{standing:?}");
    assert!(standing.divergence.ahead > 0, "the branch keeps its own commits");
    assert!(!standing.integration.would_conflict());
    drop(fixture.directory);
}

#[test]
fn a_branch_that_changed_what_the_base_changed_would_conflict() {
    let fixture = Fixture::build();
    let standing = Git::open(fixture.home("conflicting")).unwrap().standing(BASE).unwrap();

    assert!(standing.integration.would_conflict(), "{standing:?}");
    assert!(!standing.integration.is_integrated());
    drop(fixture.directory);
}

#[test]
fn a_detached_head_and_a_nested_worktree_leave_the_answer_standing() {
    let fixture = Fixture::build();
    let home = fixture.home("behind").to_path_buf();
    let before = Git::open(&home).unwrap().standing(BASE).unwrap();
    let _nested = shapes::nest(&home, "nested");
    shapes::detach(&home);

    let after = Git::open(&home).unwrap().standing(BASE).unwrap();
    assert_eq!(after.divergence, before.divergence);
    assert_eq!(after.integration, before.integration);

    let list = fixture.list(&Table(Vec::new()));
    let row = list.units.iter().find(|row| row.slug.as_str() == "behind").unwrap();
    let work = row.work.as_ref().unwrap();
    assert!(work.detached, "a home on a commit says so");
    assert_eq!(work.untracked, 1, "the nested worktree is one untracked path");
    drop(fixture.directory);
}

#[test]
fn the_list_counts_the_working_tree_by_side() {
    let fixture = Fixture::build();
    shapes::dirty(fixture.home("ahead"));

    let list = fixture.list(&Table(Vec::new()));
    let row = list.units.iter().find(|row| row.slug.as_str() == "ahead").unwrap();
    let work = row.work.as_ref().unwrap();

    assert_eq!((work.dirty, work.staged, work.untracked), (1, 1, 1), "{work:?}");
    assert_eq!(work.uncommitted(), 3);
    assert!(!work.detached);
    assert_eq!(
        work.base, "refs/remotes/origin/HEAD",
        "the project's own default branch is what a clone records"
    );
    drop(fixture.directory);
}

#[test]
fn the_units_the_base_has_moved_under_come_first_and_the_finished_ones_last() {
    let fixture = Fixture::build();
    let list = fixture.list(&Table(Vec::new()));
    let order: Vec<&str> = list.units.iter().map(|row| row.slug.as_str()).collect();

    let integrated = |slug: &str| {
        list.units
            .iter()
            .find(|row| row.slug.as_str() == slug)
            .unwrap()
            .work
            .as_ref()
            .unwrap()
            .integration
            .is_integrated()
    };
    let first_done = order.iter().position(|slug| integrated(slug)).unwrap();
    assert!(
        order.iter().skip(first_done).all(|slug| integrated(slug)),
        "finished units are not all at the end: {order:?}"
    );
    let behind = |slug: &str| {
        list.units
            .iter()
            .find(|row| row.slug.as_str() == slug)
            .unwrap()
            .work
            .as_ref()
            .unwrap()
            .main
            .behind
    };
    let live: Vec<u32> = order.iter().take(first_done).map(|slug| behind(slug)).collect();
    assert!(live.windows(2).all(|pair| pair[0] >= pair[1]), "not most behind first: {live:?}");
    drop(fixture.directory);
}

#[test]
fn the_tools_attached_to_a_home_are_counted_by_name() {
    let fixture = Fixture::build();
    let (unit, home) = fixture.units["ahead"].clone();
    let table = Table(vec![
        running(11, &unit, &home, &[("CLAUDECODE", "1")]),
        running(12, &unit, &home, &[("CLAUDECODE", "1")]),
        running(13, &unit, &home, &[("USER", "josh")]),
    ]);

    let list = fixture.list(&table);
    let row = list.units.iter().find(|row| row.slug.as_str() == "ahead").unwrap();
    let counted: Vec<(String, u32)> =
        row.sessions.iter().map(|each| (each.tool.to_string(), each.count)).collect();

    assert_eq!(counted, vec![(String::from("claude-code"), 2), (String::from("josh"), 1)]);
    assert!(list.notes.is_empty(), "{:?}", list.notes);
    drop(fixture.directory);
}

#[test]
fn a_signal_that_cannot_run_is_a_note_and_the_rows_still_print() {
    let fixture = Fixture::build();
    std::fs::remove_dir_all(fixture.home("ahead")).unwrap();

    let list = fixture.list(&Unreadable);

    assert_eq!(list.units.len(), shapes::BRANCHES.len(), "every unit still has a row");
    assert!(list.units.iter().all(|row| row.sessions.is_empty()));
    assert!(list.notes.iter().any(|note| note.starts_with("who: ")), "{:?}", list.notes);
    assert!(list.notes.iter().any(|note| note.starts_with("ahead: ")), "{:?}", list.notes);
    let gone = list.units.iter().find(|row| row.slug.as_str() == "ahead").unwrap();
    assert!(gone.work.is_none(), "a home that is not there has no Git answer");
    drop(fixture.directory);
}
