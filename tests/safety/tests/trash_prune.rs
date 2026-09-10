//! Trash prune: the trash keeps unique work and drops what a tool writes again.
//!
//! A reclaim moves a home rather than deleting it, and keeps it for the retention the
//! project set. That promise is what makes reclaim safe to type, and it is also what
//! made the trash thirteen gigabytes a unit: a built home carries its `target` and its
//! `node_modules` into the trash and nothing ever reads either again.
//!
//! So a reclaim now takes the build output and the installed dependencies out of the
//! copy, and the whole risk of that is one sentence: it must never be the operation
//! that loses something. Three tests hold the three halves of it.
//!
//! * What a tool writes again goes, and the report says how much went.
//! * What an ignore rule covers and the table does not call regenerable stays, and the
//!   report names it, because local state a person goes back into the trash for is the
//!   reason the trash exists.
//! * A path the project tracks stays whatever its name is. The candidates come from
//!   `git ls-files --others`, so a committed `dist` is not on the list at all, and this
//!   is the test that says so out loud.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::store::trash;
use nodal_safety::{InState as _, Machine, git, stderr, stdout};

/// The unit every test here reclaims.
const UNIT: &str = "worker-import";

/// How big the fake build output is. A hundred megabytes is small next to a real
/// `target` and large enough that no other content in the home can account for it.
const BUILD_BYTES: usize = 100 * 1024 * 1024;

/// The local state a person goes back into the trash for.
const LOCAL: [&str; 2] = [".env.local", "dev.sqlite"];

/// The rules a person's own machine adds, which is where a local database file and a
/// local environment file are usually covered.
///
/// Written into the home's `info/exclude` rather than the project's `.gitignore`,
/// because the prune reads Git's answer about a path and not one file of rules.
const EXCLUDE: &str = "target/\n.env.local\ndev.sqlite\n";

/// A home with a hundred megabytes of build output in it and local state beside it.
fn loaded(machine: &Machine) -> PathBuf {
    let home = machine.unit(UNIT);
    let exclude = git(&home, &["rev-parse", "--git-dir"]);
    let exclude = home.join(exclude.trim()).join("info").join("exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    let mut rules = std::fs::read_to_string(&exclude).unwrap_or_default();
    rules.push_str(EXCLUDE);
    std::fs::write(&exclude, rules).unwrap();

    std::fs::create_dir_all(home.join("target/debug")).unwrap();
    std::fs::write(home.join("target/debug/app"), vec![0_u8; BUILD_BYTES]).unwrap();
    std::fs::write(home.join(LOCAL[0]), "TOKEN=only-in-this-home\n").unwrap();
    std::fs::write(home.join(LOCAL[1]), vec![0_u8; 4096]).unwrap();
    home
}

/// Reclaim the unit, insisting that it went through, and answer with what it printed.
fn reclaimed(machine: &Machine) -> String {
    let output = machine.nodal(&["reclaim", UNIT]);
    assert!(output.status.success(), "the reclaim failed: {}", stderr(&output));
    stdout(&output)
}

/// The one directory in the trash.
fn trashed(machine: &Machine) -> PathBuf {
    let mut entries = machine.trashed();
    assert_eq!(entries.len(), 1, "the reclaim put one home in the trash");
    entries.pop().unwrap()
}

/// What a directory holds, as the sum of the files under it.
fn bytes_of(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            total += bytes_of(&entry.path());
        } else if kind.is_file() {
            total += entry.metadata().map_or(0, |data| data.len());
        }
    }
    total
}

#[test]
fn the_trash_holds_the_home_without_its_build_output() {
    let machine = Machine::new();
    let home = loaded(&machine);
    let before = bytes_of(&home);
    assert!(before > BUILD_BYTES as u64, "the home holds the build output the test wrote");

    reclaimed(&machine);
    let trash = trashed(&machine);
    assert!(!trash.join("target").exists(), "the trash kept the build output");
    assert!(!trash.join("node_modules").exists(), "the trash kept the installed dependencies");
    assert!(
        bytes_of(&trash) < before - BUILD_BYTES as u64,
        "the trashed copy is not smaller than the home was by what was dropped"
    );
    assert!(trash.join("apps/web/app/page.tsx").is_file(), "the work is in the trash");
}

#[test]
fn the_local_state_stays_in_the_trash_and_the_report_names_it() {
    let machine = Machine::new();
    loaded(&machine);
    let told = reclaimed(&machine);
    let trash = trashed(&machine);

    for path in LOCAL {
        assert!(trash.join(path).is_file(), "the prune took {path}, which no tool writes again");
    }
    assert_eq!(
        std::fs::read_to_string(trash.join(LOCAL[0])).unwrap(),
        "TOKEN=only-in-this-home\n",
        "the file in the trash is the one the home held"
    );
    assert!(
        told.contains("local state kept in the trash"),
        "the report does not say what the trash kept: {told}"
    );
    assert!(told.contains(LOCAL[0]), "the report does not name {}: {told}", LOCAL[0]);
}

#[test]
fn the_row_records_what_was_dropped() {
    let machine = Machine::new();
    loaded(&machine);
    let told = reclaimed(&machine);

    let rows = trash::list(machine.store().conn()).unwrap();
    assert_eq!(rows.len(), 1, "one home was reclaimed");
    assert!(
        rows[0].pruned_bytes >= BUILD_BYTES as u64,
        "the row says {} bytes were dropped, which is less than the build output",
        rows[0].pruned_bytes
    );
    assert!(
        told.contains("dropped") && told.contains("target"),
        "the report does not say what went: {told}"
    );
}

#[test]
fn a_directory_the_project_tracks_is_kept_whatever_its_name_is() {
    let machine = Machine::new();
    let home = loaded(&machine);

    // A build directory the project commits into. The name is on the exclusion table;
    // the commit is the project's own statement that the content is work, and a commit
    // beats a name.
    std::fs::create_dir_all(home.join("dist")).unwrap();
    std::fs::write(home.join("dist/report.html"), "a baseline somebody committed\n").unwrap();
    git(&home, &["add", "--force", "--", "dist/report.html"]);
    git(&home, &["commit", "--quiet", "--message", "commit the baseline report"]);
    // The commit has to exist somewhere else, or the uniqueness check refuses the
    // reclaim and this test is about the prune rather than about that refusal.
    git(&machine.source, &["fetch", "--quiet", home.to_str().unwrap(), "HEAD"]);

    reclaimed(&machine);
    let trash = trashed(&machine);
    assert_eq!(
        std::fs::read_to_string(trash.join("dist/report.html")).unwrap(),
        "a baseline somebody committed\n",
        "the prune took a directory the project tracks"
    );
    assert!(!trash.join("target").exists(), "the untracked build output still went");
}
