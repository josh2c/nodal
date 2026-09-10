//! The machine survey: three clones of one remote, grouped, and the walk writes nothing.
//!
//! One clone is clean and pushed, one holds an unpushed commit, one is dirty. They sit
//! under a temporary root with the bare remote. The survey names one group of three,
//! the unpushed count, the dirty count, and it leaves every path as it found it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use nodal_core::doctor::{self, machine};
use nodal_core::model::Timestamp;
use nodal_core::output::view::machine::MachineReport;
use nodal_core::store::Store;
use nodal_safety::git::{self, git_ok as git};
use nodal_safety::tree::Snapshot;

/// A planted machine: three clones of one bare remote, and one clone with no remote.
struct Planted {
    directory: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
    store: Store,
    clean: PathBuf,
    unpushed: PathBuf,
    dirty: PathBuf,
    lone: PathBuf,
}

impl Planted {
    fn report(&self) -> MachineReport {
        let request = machine::Request {
            roots: std::slice::from_ref(&self.root),
            depth: 6,
            state_dir: &self.state,
        };
        machine::survey(&doctor::Registry::Open(self.store.conn()), &request, Timestamp::now())
            .expect("a machine the survey can read")
    }
}

fn plant() -> Planted {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().to_path_buf();
    let clones = root.join("clones");
    let state = root.join("state");
    std::fs::create_dir_all(&clones).unwrap();
    std::fs::create_dir_all(&state).unwrap();

    let remote = clones.join("remote.git");
    let seed = clones.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git::init(&seed, "main");
    std::fs::write(seed.join("README.md"), "# app\n").unwrap();
    std::fs::write(seed.join(".gitignore"), "build/\n").unwrap();
    git::commit(&seed, "start");
    git(&clones, &["clone", "--quiet", "--bare", seed.to_str().unwrap(), remote.to_str().unwrap()]);
    std::fs::remove_dir_all(&seed).unwrap();

    let clean = clone(&clones, &remote, "clean");
    let unpushed = clone(&clones, &remote, "unpushed");
    let dirty = clone(&clones, &remote, "dirty");

    std::fs::write(unpushed.join("extra.md"), "only here\n").unwrap();
    git::commit(&unpushed, "work nobody else has");

    std::fs::write(dirty.join("dirty.md"), "uncommitted\n").unwrap();
    std::fs::create_dir_all(clean.join("build")).unwrap();
    std::fs::write(clean.join("build/big.bin"), vec![0_u8; 4096]).unwrap();

    let lone = clones.join("lone");
    std::fs::create_dir_all(&lone).unwrap();
    git::init(&lone, "main");
    std::fs::write(lone.join("README.md"), "alone\n").unwrap();
    git::commit(&lone, "start");

    let store = Store::open(state.join("registry.db")).expect("a registry");
    Planted { directory, root, state, store, clean, unpushed, dirty, lone }
}

fn clone(root: &Path, remote: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    git(root, &["clone", "--quiet", remote.to_str().unwrap(), path.to_str().unwrap()]);
    git::identity(&path);
    path
}

/// Name, size and modification time of every path under `root`.
fn is_registry(relative: &Path) -> bool {
    relative
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.starts_with("registry.db"))
}

fn fingerprint(root: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
    let mut found = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap();
            if is_registry(relative) {
                continue;
            }
            let metadata = entry.metadata().unwrap();
            found.insert(path.clone(), (metadata.len(), metadata.modified().ok()));
            if metadata.is_dir() {
                queue.push(path);
            }
        }
    }
    found
}

#[test]
fn three_clones_of_one_remote_are_one_group() {
    let planted = plant();
    let report = planted.report();

    let group = report
        .groups
        .iter()
        .find(|group| group.clones == 3)
        .expect("the three clones share a group");
    assert_eq!(group.dirty, 1, "{group:?}");
    assert_eq!(group.unpushed, 1, "{group:?}");
    assert!(!group.nothing_unique, "{group:?}");
    assert!(group.repositories.iter().any(|row| row.path == planted.clean));
    assert!(group.repositories.iter().any(|row| row.path == planted.unpushed && row.unpushed == 1));
    assert!(group.repositories.iter().any(|row| row.path == planted.dirty && row.dirty > 0));
    assert!(
        group.ignored.iter().any(|dir| dir.path == "build" && dir.bytes >= 4096),
        "ignored directories: {:?}",
        group.ignored
    );
}

#[test]
fn a_clone_with_no_remote_is_its_own_group_named_by_path() {
    let planted = plant();
    let report = planted.report();
    let lone = report
        .groups
        .iter()
        .find(|group| group.repositories.iter().any(|row| row.path == planted.lone))
        .expect("the clone with no remote");
    assert_eq!(lone.clones, 1, "{lone:?}");
    assert_eq!(lone.name, planted.lone.display().to_string());
}

#[test]
fn the_state_directory_is_skipped_even_when_it_holds_a_repository() {
    let planted = plant();
    let hidden = planted.state.join("hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    git::init(&hidden, "main");
    std::fs::write(hidden.join("secret.md"), "no\n").unwrap();
    git::commit(&hidden, "hidden");

    let report = planted.report();
    assert!(
        !report.groups.iter().any(|group| group.repositories.iter().any(|row| row.path == hidden)),
        "{report:?}"
    );
    assert!(
        report.skipped.iter().any(|skip| skip.why.contains("state directory")),
        "{:?}",
        report.skipped
    );
}

#[test]
fn the_survey_writes_nothing() {
    let planted = plant();
    let before = fingerprint(planted.directory.path());
    let snapshot = Snapshot::of_except(planted.directory.path(), is_registry);
    let report = planted.report();
    assert!(!report.groups.is_empty(), "the survey found nothing to leave alone");
    snapshot.assert_unchanged(
        &Snapshot::of_except(planted.directory.path(), is_registry),
        "the survey wrote",
    );
    assert_eq!(before, fingerprint(planted.directory.path()), "a path changed size or mtime");
}
