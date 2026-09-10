//! `nodal doctor --machine` reads clones under a root and leaves every path as it was.
//!
//! The checkout survey already has a byte-for-byte proof. The machine scan is a second
//! walk, over trees the registry does not name, so it needs its own. A read that wrote
//! would be worse than one that refused: the person ran it to see whether a clone is
//! safe to delete, not to change it.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::store::{Store, environments, projects, units};
use nodal_safety::InState as _;
use nodal_safety::{Machine, Snapshot, git, stdout};

/// Whether a path under the state directory is the registry, which every command writes.
fn is_registry(relative: &Path) -> bool {
    relative
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.starts_with("registry.db"))
}

/// Three clones of one bare remote under `root`.
fn plant(root: &Path) -> PathBuf {
    let clones = root.join("clones");
    std::fs::create_dir_all(&clones).unwrap();
    let remote = clones.join("remote.git");
    let seed = clones.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git::init(&seed, "main");
    std::fs::write(seed.join("README.md"), "# app\n").unwrap();
    git::commit(&seed, "start");
    git(&clones, &["clone", "--quiet", "--bare", seed.to_str().unwrap(), remote.to_str().unwrap()]);
    std::fs::remove_dir_all(&seed).unwrap();
    for name in ["clean", "unpushed", "dirty"] {
        let path = clones.join(name);
        git(&clones, &["clone", "--quiet", remote.to_str().unwrap(), path.to_str().unwrap()]);
        git::identity(&path);
    }
    let unpushed = clones.join("unpushed");
    std::fs::write(unpushed.join("extra.md"), "only here\n").unwrap();
    git::commit(&unpushed, "work nobody else has");
    std::fs::write(clones.join("dirty/dirty.md"), "uncommitted\n").unwrap();
    clones
}

/// Every row of the registry a survey could touch, as text.
fn rows(machine: &Machine) -> String {
    let store: Store = machine.store();
    let mut lines = Vec::new();
    for project in projects::list(store.conn()).unwrap() {
        lines.push(format!("{} {} {}", project.id, project.name, project.root.display()));
        for unit in units::list(store.conn(), project.id).unwrap() {
            lines.push(format!("  {} {} {:?}", unit.id, unit.slug, unit.status));
            for row in environments::list_for_unit(store.conn(), unit.id).unwrap() {
                lines.push(format!("    {} {} {:?}", row.id, row.home.display(), row.state));
            }
        }
    }
    lines.join("\n")
}

#[test]
fn doctor_machine_leaves_the_clones_and_the_state_directory_as_it_found_them() {
    let machine = Machine::new();
    machine.unit("worker-import");
    let parent = machine.source.parent().expect("the machine root");
    let clones = plant(parent);

    let planted = Snapshot::of(&clones);
    let source = Snapshot::of(&machine.source);
    let state = Snapshot::of_except(&machine.state, is_registry);
    let before = rows(&machine);
    assert!(!planted.is_empty(), "there is nothing here to leave alone");

    let root = clones.to_str().unwrap();
    let report = stdout(&machine.nodal(&["doctor", "--machine", root]));
    assert!(report.contains("clones"), "the survey reported nothing:\n{report}");
    assert!(report.contains("nothing unique") || report.contains("UNPUSHED"), "{report}");
    assert!(report.contains("it removed nothing"), "{report}");

    planted.assert_unchanged(&Snapshot::of(&clones), "doctor --machine wrote in the clones");
    source
        .assert_unchanged(&Snapshot::of(&machine.source), "doctor --machine wrote in the checkout");
    state.assert_unchanged(
        &Snapshot::of_except(&machine.state, is_registry),
        "doctor --machine wrote in the state directory",
    );
    assert_eq!(before, rows(&machine), "doctor --machine changed a row of the registry");
}

#[test]
fn doctor_machine_json_leaves_the_clones_alone_as_well() {
    let machine = Machine::new();
    let parent = machine.source.parent().expect("the machine root");
    let clones = plant(parent);
    let planted = Snapshot::of(&clones);
    let root = clones.to_str().unwrap();
    let report = stdout(&machine.nodal(&["doctor", "--machine", root, "--json"]));
    assert!(report.contains("repositories"), "the JSON answer reported nothing:\n{report}");
    planted.assert_unchanged(&Snapshot::of(&clones), "doctor --machine --json wrote in the clones");
}
