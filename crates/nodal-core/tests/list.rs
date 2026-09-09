//! Acceptance for the list: `nodal ls` over real repositories.
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
use nodal_core::model::{Digest, HostName, Project, ProjectId, ProjectName, Timestamp, UnitId};
use nodal_core::runtime::processes::{Processes, Running};
use nodal_core::runtime::{entry, ls};
use nodal_core::store::{Store, projects};
use nodal_fixture::shapes::{self, Branch};
use nodal_safety::rows;
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
            let id = record(&store, &project, branch.name, index, &home);
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

/// Record a unit and its home, and return the unit's identity.
///
/// The host is this fixture's own rather than the machine's: these tests are about what
/// the list says of a unit that stands somewhere else.
fn record(store: &Store, project: &Project, branch: &str, index: usize, home: &Path) -> UnitId {
    let row = rows::Row { index, slug: branch, branch, home, host: host() };
    rows::record(store, project.id, &row).id
}

/// The verdict a branch's clone gets, read straight from the Git facade.
fn verdict(home: &Path) -> String {
    Git::open(home).unwrap().standing(BASE).unwrap().integration.label()
}

/// A project is found from inside it whatever name the path was given.
///
/// The regression this pins: macOS hands a running process the resolved name of the
/// directory it is in, so a project recorded under `/var/folders/…` was invisible to a
/// command run in what the person called `/var/folders/…` and the kernel called
/// `/private/var/folders/…`. Nodal records the resolved form and looks the resolved form
/// up. The second half of the test is the other direction: a row an earlier build
/// recorded through a link is still reachable.
#[cfg(unix)]
#[test]
fn a_project_is_found_from_inside_it_whatever_name_the_path_was_given() {
    let directory = tempfile::tempdir().unwrap();
    let real = directory.path().join("project");
    std::fs::create_dir_all(real.join("packages").join("web")).unwrap();
    let link = directory.path().join("by-another-name");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let resolved = Store::open(directory.path().join("resolved.db")).unwrap();
    projects::insert(resolved.conn(), &project_at_root(&real.canonicalize().unwrap(), '1'))
        .unwrap();
    let found = entry::project_at(resolved.conn(), &link.join("packages").join("web"))
        .unwrap()
        .expect("a project recorded resolved is found through a link");
    assert_eq!(found.root, real.canonicalize().unwrap());

    let as_given = Store::open(directory.path().join("as-given.db")).unwrap();
    projects::insert(as_given.conn(), &project_at_root(&link, '2')).unwrap();
    let found = entry::project_at(as_given.conn(), &link)
        .unwrap()
        .expect("a project recorded through a link is still found by that name");
    assert_eq!(found.root, link);
}

/// A project row rooted at one path, with a fixed identity.
#[cfg(unix)]
fn project_at_root(root: &Path, tag: char) -> Project {
    Project {
        id: ProjectId::parse(format!("01ARZ3NDEKTSV4RRFFQ69G5FA{tag}")).unwrap(),
        root: root.to_path_buf(),
        name: ProjectName::parse("fixture").unwrap(),
        recipe_hash: Digest::parse("0".repeat(64)).unwrap(),
        created_at: Timestamp::now(),
    }
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
