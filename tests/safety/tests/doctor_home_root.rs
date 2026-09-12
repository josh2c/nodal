//! `nodal doctor --machine` with no path reaches the repositories under the home.
//!
//! The help, the contract and the README all say the same thing: given no path, the
//! machine scan walks the home directory. A home is not a clone, and it holds whatever
//! a person has left in it — including an entry named `.git` that no repository is read
//! out of. Taking that name for a repository ended the walk at the home, so the scan
//! answered that there was nothing under a home holding dozens of clones. The property
//! is that the walk goes on through such a directory, and the explicit-root form and the
//! no-path form report the same repositories from the same root.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::{Machine, Snapshot, git, stdout};

/// A home holding one repository below an ordinary first-level directory.
///
/// The `.git` at the top of the home is the part that matters. It is a directory wearing
/// the name and nothing else, which is what an abandoned `git init` leaves behind, and
/// no repository can be read out of it.
fn plant_home(root: &Path) -> PathBuf {
    let home = root.join("person");
    let repo = home.join("Projects/app");
    std::fs::create_dir_all(&repo).unwrap();
    git::init(&repo, "main");
    git::identity(&repo);
    std::fs::write(repo.join("README.md"), "# app\n").unwrap();
    git::commit(&repo, "start");
    std::fs::create_dir_all(home.join(".git")).unwrap();
    // The report names every root by the path the filesystem uses, and a temporary
    // directory is reached through a symbolic link on some hosts, so the home the
    // assertions are written against is the resolved one.
    std::fs::canonicalize(&home).unwrap()
}

#[test]
fn the_no_path_machine_scan_reports_a_repository_under_the_home() {
    let machine = Machine::new();
    let root = machine.source.parent().expect("the machine root");
    let home = plant_home(root);
    let scan = machine.with_env(("HOME", home.to_str().unwrap()));

    let report = stdout(&scan.nodal(&["doctor", "--machine"]));

    assert!(report.contains(home.to_str().unwrap()), "the home was not the root:\n{report}");
    assert!(
        report.contains("Projects/app"),
        "the repository under the home was not reported:\n{report}"
    );
    assert!(!report.contains("no repository under these roots"), "{report}");
    assert!(!report.contains("is not a Git repository"), "{report}");
}

#[test]
fn the_no_path_form_and_the_explicit_root_form_report_the_same_repository() {
    let machine = Machine::new();
    let root = machine.source.parent().expect("the machine root");
    let home = plant_home(root);
    let scan = machine.with_env(("HOME", home.to_str().unwrap()));

    let implicit = stdout(&scan.nodal(&["doctor", "--machine"]));
    let explicit = stdout(&scan.nodal(&["doctor", "--machine", home.to_str().unwrap()]));

    assert_eq!(rows(&implicit), rows(&explicit), "implicit:\n{implicit}\nexplicit:\n{explicit}");
}

#[test]
fn the_no_path_scan_leaves_the_home_as_it_found_it() {
    let machine = Machine::new();
    let root = machine.source.parent().expect("the machine root");
    let home = plant_home(root);
    let scan = machine.with_env(("HOME", home.to_str().unwrap()));

    let planted = Snapshot::of(&home);
    assert!(!planted.is_empty(), "there is nothing here to leave alone");

    let report = stdout(&scan.nodal(&["doctor", "--machine"]));
    assert!(report.contains("it removed nothing"), "{report}");

    planted.assert_unchanged(&Snapshot::of(&home), "doctor --machine wrote in the home");
}

/// The group lines of a report, which is the part the two forms must agree on.
///
/// The walked count and the milliseconds differ between two runs of the same scan, so
/// they are not part of the comparison.
fn rows(report: &str) -> Vec<&str> {
    report
        .lines()
        .map(str::trim_end)
        .filter(|line| line.contains("Projects/app") || line.contains("GROUP"))
        .collect()
}
