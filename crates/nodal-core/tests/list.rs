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
use nodal_core::model::{
    Actor, ActorKind, ActorName, Digest, HostName, Lock, Project, ProjectId, ProjectName,
    Timestamp, UnitId,
};
use nodal_core::output::Render;
use nodal_core::output::view::{Disk, HolderState, Unknowable, Unmeasured};
use nodal_core::runtime::processes::{Processes, Running};
use nodal_core::runtime::{entry, ls, show};
use nodal_core::store::{Store, locks, projects, units};
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
            remote_url: None,
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
        remote_url: None,
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
        work.base, "main",
        "the base is named as the person names it, not as the ref Nodal measured it from"
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

// ------------------------------------------------- the holder, read and not assumed

/// Take a hold on a unit, by an actor on a host, by a process.
///
/// Written straight to the table rather than through `runtime::lock`, because these
/// tests are about what a report says of a row that is already there: a row the process
/// that wrote it did not outlive.
fn hold(fixture: &Fixture, branch: &str, host: HostName, pid: Option<u32>) -> UnitId {
    let unit = fixture.units[branch].0;
    let now = Timestamp::now();
    let lock = Lock {
        unit_id: unit,
        host,
        actor: Some(Actor {
            kind: ActorKind::Agent,
            name: ActorName::parse("claude-code").unwrap(),
        }),
        pid,
        taken_at: now,
        refreshed_at: now,
        expires_at: Timestamp::from_unix_seconds(now.unix_seconds() + 28_800).unwrap(),
    };
    locks::take(fixture.store.conn(), &lock, now, lock.idle_deadline(8)).unwrap();
    unit
}

/// The WHO cell of one unit, as a person reads it.
fn who(list: &nodal_core::output::view::UnitList, branch: &str) -> String {
    list.doc().lines().into_iter().find(|line| line.contains(branch)).expect("the unit has a row")
}

/// The finding this pins is F-A of the three-day proof: an agent was killed and the
/// report still said `claude-code holds 8 h`, with the dead process named beside it.
/// Nothing in the answer told the next session that nobody was there.
#[test]
fn a_hold_whose_process_is_gone_is_never_reported_as_held() {
    let fixture = Fixture::build();
    let (unit, home) = fixture.units["ahead"].clone();
    hold(&fixture, "ahead", HostName::current(), Some(4_294_967_000));
    // Somebody else's shell stands in the home. Nothing of the actor that holds it does.
    let table = Table(vec![running(11, &unit, &home, &[("USER", "josh")])]);

    let list = fixture.list(&table);
    let row = list.units.iter().find(|row| row.slug.as_str() == "ahead").unwrap();
    let holder = row.holder.as_ref().expect("the row carries the holder");

    assert_eq!(holder.state, HolderState::Gone);
    assert_eq!(holder.pid, Some(4_294_967_000), "the row still says which process it was");
    let cell = who(&list, "ahead");
    assert!(cell.contains("claude-code gone"), "{cell}");
    assert!(!cell.contains("holds"), "a dead holder is reported as holding: {cell}");
    drop(fixture.directory);
}

/// The other side of the same reading: a process that is there is reported as there,
/// in the word the column always used.
#[test]
fn a_hold_whose_process_is_running_is_reported_as_held() {
    let fixture = Fixture::build();
    let (unit, home) = fixture.units["ahead"].clone();
    let pid = 4_120;
    hold(&fixture, "ahead", HostName::current(), Some(pid));
    let table = Table(vec![running(pid, &unit, &home, &[("CLAUDECODE", "1")])]);

    let list = fixture.list(&table);
    let row = list.units.iter().find(|row| row.slug.as_str() == "ahead").unwrap();

    assert_eq!(row.holder.as_ref().unwrap().state, HolderState::Live);
    assert!(who(&list, "ahead").contains("claude-code holds"), "{}", who(&list, "ahead"));
    drop(fixture.directory);
}

/// A hold belongs to an actor, not to one process of theirs. The identifier a lock row
/// carries is the command that entered the home, and that command ends; the session it
/// belonged to does not. So a hold whose recorded process is gone is live while a process
/// of the same actor is still in the home.
#[test]
fn a_hold_is_live_while_the_actor_is_in_the_home_whatever_became_of_the_process() {
    let fixture = Fixture::build();
    let (unit, home) = fixture.units["ahead"].clone();
    hold(&fixture, "ahead", HostName::current(), Some(4_294_967_000));
    let table = Table(vec![running(11, &unit, &home, &[("CLAUDECODE", "1")])]);

    let list = fixture.list(&table);
    let row = list.units.iter().find(|row| row.slug.as_str() == "ahead").unwrap();

    assert_eq!(row.holder.as_ref().unwrap().state, HolderState::Live);
    assert!(who(&list, "ahead").contains("claude-code holds"), "{}", who(&list, "ahead"));
    drop(fixture.directory);
}

/// A hold from another machine, and a host with no readable process table, are both
/// unknown and neither is gone. A reading that could not be taken proves nothing.
#[test]
fn a_reading_that_cannot_be_taken_is_unknown_and_never_gone() {
    let fixture = Fixture::build();
    hold(&fixture, "ahead", host(), Some(4_120));
    hold(&fixture, "behind", HostName::current(), Some(4_121));

    let list = fixture.list(&Unreadable);
    let state = |slug: &str| {
        list.units.iter().find(|row| row.slug.as_str() == slug).unwrap().holder.clone().unwrap()
    };

    assert_eq!(state("ahead").state, HolderState::Unknown { why: Unknowable::AnotherHost });
    assert_eq!(state("behind").state, HolderState::Unknown { why: Unknowable::NoProcessTable });
    // And it reads the way the lock row states it. This is the rendering a host with no
    // readable process table gets for every hold it has — macOS today — and it is
    // asserted here rather than only there, because a suite that runs on one host must
    // still hold the other host's words.
    for slug in ["ahead", "behind"] {
        let cell = who(&list, slug);
        assert!(cell.contains("claude-code holds"), "{cell}");
        assert!(!cell.contains("gone"), "a reading nobody took was printed as gone: {cell}");
    }
    drop(fixture.directory);
}

// ------------------------------------------------------- what a home is said to hold

/// The finding this pins is F-B of the three-day proof: `disk` was an em dash for a home
/// holding 108 MiB, which reads as "nothing here". A list still does not walk a home —
/// that is the detail's cost — but it says so, and `nodal show` measures.
#[test]
fn a_list_says_why_a_home_was_not_measured_and_a_detail_measures_it() {
    let fixture = Fixture::build();
    let (_, home) = fixture.units["ahead"].clone();
    std::fs::write(home.join("weight.bin"), vec![7_u8; 40_000]).unwrap();

    let list = fixture.list(&Table(Vec::new()));
    let row = list.units.iter().find(|row| row.slug.as_str() == "ahead").unwrap();
    let listed = row.environment.as_ref().unwrap().disk.clone();

    assert_eq!(listed, Disk::Unmeasured { why: Unmeasured::NotAsked });

    let unit = units::get(fixture.store.conn(), fixture.units["ahead"].0).unwrap().unwrap();
    let detail = show::detail(fixture.store.conn(), list, &unit).unwrap();
    let measured = detail.unit.environment.as_ref().unwrap().disk.clone();

    let Disk::Measured { bytes } = measured else { panic!("a detail did not measure the home") };
    assert!(bytes.apparent >= 40_000, "{bytes:?}");
    assert!(bytes.complete);
    assert!(!bytes.exclusive_unknown.is_empty(), "a figure says what it is not");
    drop(fixture.directory);
}
