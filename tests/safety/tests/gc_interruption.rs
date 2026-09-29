//! A `gc` killed part-way leaves a state the next `gc` finishes.
//!
//! `gc` is the one operation in Nodal that deletes a directory somebody worked in. It
//! has no plan and no undo, because removing a tree has none. What it has instead is an
//! order in which a kill between any two steps is safe, and one mark on the disk.
//!
//! Removing a tree is many operations of the filesystem. A process killed part-way
//! through one leaves a directory holding part of a home, and no reading can be made of
//! that: Git refuses a repository whose object store is half there. Under the home's own
//! name, such a directory is read again by every later sweep, refused every time, and
//! stays for ever, with a row that points at it and never goes. That is a home that is
//! neither kept nor cleanly removed.
//!
//! So the home is renamed first. A rename is one operation of one filesystem — the trash
//! is one directory — so the home is whole under its own name or wholly under the
//! removing name, and there is no instant between the two. The removing name says the
//! tree was read, the reading found no last copy, and the removal has started.
//!
//! A person can still make the one shape the ordering does not: a home under both names
//! at once, by renaming or copying a directory inside the trash, or by restoring one out
//! of a backup. Nothing in the sweep guards against it. The rename does, because a move
//! onto a name a tree already holds fails, so the sweep keeps that home, keeps its row
//! and names the directory. A sweep that trusted the mark's name instead would print the
//! home as removed and leave the whole of it standing with nothing pointing at it.
//!
//! | property | test |
//! |---|---|
//! | a real sweep, killed while it removes, leaves a state the next one finishes | `a_killed_sweep_leaves_a_state_the_next_sweep_finishes` |
//! | a whole home under the removing name is taken away | `a_home_under_the_removing_name_is_taken_away` |
//! | a part-removed home under it is finished, and never read again | `a_part_removed_home_is_finished_and_never_read_again` |
//! | a tree that went with the row still there is forgotten | `a_tree_that_went_before_the_row_is_forgotten` |
//! | a home the reading keeps survives a killed sweep of another | `a_home_the_reading_keeps_survives_a_killed_sweep_of_another` |
//! | a home part removed under its own name is named and kept | `a_home_part_removed_under_its_own_name_is_named_and_kept` |
//! | a home under both names is kept, and a second sweep finishes it | `a_home_under_both_names_is_kept_and_the_next_sweep_finishes_it` |
//!
//! Every property here reads the filesystem and the registry, which both hosts answer,
//! so each one asserts the same thing on Linux and on macOS.
//!
//! The suite spells the removing suffix itself rather than reading it off the product.
//! A state is then built the same way whichever release wrote the sweep that was killed,
//! which is what `tests/base_resume.rs` does for a base a build was killed in.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nodal_safety::InState as _;
use nodal_safety::git::{COPY, sibling_holding};
use nodal_safety::{Machine, git, stderr, stdout};

/// What a sweep adds to a home's name while it removes it.
const REMOVING: &str = ".removing";

/// The unit every property here reclaims. It is one of the fixture's own handles.
const SLUG: &str = "worker-import";

/// The unit the last property keeps, while the sweep finishes the first.
const OTHER: &str = "payroll-export";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const ONLY: &str = "only-here.txt";

/// What the report tells a person to do about a directory it can neither read nor
/// finish. The sentence is the contract, so the suite spells it.
const TOLD: &str = "remove the directory yourself and the next sweep forgets the row";

/// A second clone of the project, beside the checkout, where a person keeps a copy.
const SIBLING: &str = "sibling";

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// How many files the killed sweep is given to remove, so that the mark it writes is
/// on the disk for long enough for this test to read it.
///
/// They go into the trashed directory after the reclaim, so the reclaim itself pays
/// nothing for them. A tree this size takes tens of milliseconds to remove on the
/// runner and longer on a filesystem that is slower per file; the poll below reads
/// every millisecond and waits for the whole of the sweep either way.
const BULK: (usize, usize) = (100, 200);

/// A machine whose trash keeps nothing.
///
/// What a killed sweep leaves is the whole of what these properties are about, and a
/// fortnight of waiting is not part of it ([`Machine::keep_no_trash`]).
fn machine() -> Machine {
    let machine = Machine::new().with_env(ONLY_LOCAL);
    machine.keep_no_trash();
    machine
}

/// Reclaim the unit and answer where its home now is.
fn reclaimed(machine: &Machine, slug: &str) -> PathBuf {
    let done = machine.nodal(&["reclaim", slug]);
    assert!(done.status.success(), "the reclaim refused: {}", stderr(&done));
    let mut trashed = machine.trashed();
    trashed.retain(|path| !named_removing(path));
    trashed.into_iter().next().expect("the reclaim left nothing in the trash")
}

/// The name a home is under while a sweep removes it.
fn removing(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(REMOVING);
    PathBuf::from(name)
}

/// Whether this is a directory a sweep had started to remove.
fn named_removing(path: &Path) -> bool {
    path.to_string_lossy().ends_with(REMOVING)
}

/// Put the home under the removing name, as a sweep does before it removes it.
fn mark(path: &Path) -> PathBuf {
    let going = removing(path);
    std::fs::rename(path, &going).unwrap();
    going
}

/// Take away the part of the home a killed removal had already reached.
///
/// The object store, because that is the part whose loss stops any later reading: Git
/// answers "not a git repository" for a directory whose `.git` is half there, so a
/// sweep that read this again would refuse it and keep it for ever.
fn part_removed(going: &Path) {
    let objects = going.join(".git/objects");
    assert!(objects.is_dir(), "the home has no object store to take away");
    std::fs::remove_dir_all(&objects).unwrap();
}

/// A file tree big enough that a sweep removing it can be caught doing so.
fn bulk(path: &Path) {
    let (directories, files) = BULK;
    for directory in 0..directories {
        let at = path.join(format!("bulk-{directory}"));
        std::fs::create_dir_all(&at).unwrap();
        for file in 0..files {
            std::fs::write(at.join(format!("{file}")), "x").unwrap();
        }
    }
}

/// The trash rows the registry still holds.
fn rows(machine: &Machine) -> Vec<PathBuf> {
    let store = machine.store();
    nodal_core::store::trash::list(store.conn())
        .unwrap()
        .into_iter()
        .map(|entry| entry.path)
        .collect()
}

/// The invariant every property ends on: the trash holds nothing a sweep was part-way
/// through, and no row points at a directory that is not there.
///
/// A row whose directory is gone is the state a kill between the tree and the row
/// leaves, and it is recoverable rather than wrong. This is asserted after a sweep that
/// was allowed to finish, where neither is left.
fn settled(machine: &Machine) {
    let left: Vec<PathBuf> = machine.trashed().into_iter().filter(|p| named_removing(p)).collect();
    assert!(left.is_empty(), "a finished sweep left a home part-way removed: {left:?}");
    for row in rows(machine) {
        assert!(row.is_dir(), "a row points at a directory that is not there: {}", row.display());
    }
}

/// Sweep, insisting that the sweep itself did not fail.
fn sweep(machine: &Machine) -> String {
    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    stdout(&swept)
}

/// A real sweep, killed while it is removing a home, leaves the disk and the registry
/// in a state the next sweep finishes.
///
/// The kill is real and the sweep is real: the test starts `nodal gc`, waits for the
/// mark the sweep writes before it touches the tree, and kills the process. Waiting for
/// the mark rather than for a length of time is what makes this deterministic; the tree
/// is made big enough that the mark stands for long enough to be read.
///
/// Two things are asserted of the state the kill left. The home is never under its own
/// name and part removed, which is the whole point of the rename. And the next sweep
/// takes the rest of it away and drops the row.
#[test]
fn a_killed_sweep_leaves_a_state_the_next_sweep_finishes() {
    let machine = machine();
    machine.unit(SLUG);
    let trash = reclaimed(&machine, SLUG);
    bulk(&trash);

    let mut running = machine.command(&["gc"]).spawn().expect("the sweep did not start");
    let going = removing(&trash);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !going.exists() && running.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    let caught = going.exists();
    running.kill().unwrap();
    running.wait().unwrap();
    assert!(caught, "the sweep removed the home without ever marking it");

    // The rename is one operation, so the home is under one name and not both, and
    // nothing is part removed under the name a later sweep would read again.
    assert!(!trash.exists(), "the home is under its own name as well as the removing one");
    assert_eq!(rows(&machine).len(), 1, "the killed sweep took the row with it");

    sweep(&machine);
    assert!(!going.exists(), "the next sweep did not finish the removal");
    assert!(rows(&machine).is_empty(), "the next sweep kept the row of a home that has gone");
    settled(&machine);
}

/// A kill between the rename and the first file removed leaves a whole home under the
/// removing name. The next sweep takes it away.
#[test]
fn a_home_under_the_removing_name_is_taken_away() {
    let machine = machine();
    machine.unit(SLUG);
    let going = mark(&reclaimed(&machine, SLUG));
    assert!(going.join(".git").is_dir(), "the state this starts from is a whole home");

    sweep(&machine);

    assert!(!going.exists(), "the home under the removing name stayed");
    assert!(rows(&machine).is_empty(), "its row stayed");
    settled(&machine);
}

/// A kill part-way through the removal leaves a directory no reading can be made of.
/// The next sweep finishes it, because the mark says the reading was already made.
///
/// This is the state the mark exists for. Under the home's own name the same directory
/// is read again, refused, and kept for ever; under the removing name it is what is
/// left of a home that was read and let go.
#[test]
fn a_part_removed_home_is_finished_and_never_read_again() {
    let machine = machine();
    machine.unit(SLUG);
    let going = mark(&reclaimed(&machine, SLUG));
    part_removed(&going);

    let report = sweep(&machine);

    assert!(!going.exists(), "what was left of the home stayed");
    assert!(rows(&machine).is_empty(), "its row stayed");
    assert!(!report.contains("not a git repository"), "the sweep read it again: {report}");
    settled(&machine);
}

/// A kill between the last file and the row leaves a row pointing at nothing. The next
/// sweep drops it, and says nothing a person has to act on.
#[test]
fn a_tree_that_went_before_the_row_is_forgotten() {
    let machine = machine();
    machine.unit(SLUG);
    let trash = reclaimed(&machine, SLUG);
    std::fs::remove_dir_all(&trash).unwrap();

    sweep(&machine);

    assert!(rows(&machine).is_empty(), "the row of a home that has gone stayed");
    settled(&machine);
}

/// A home the fresh reading keeps is not endangered by a killed sweep of another one.
///
/// The sweep of the part-removed home is finished in the same run that reads the second
/// home again, finds the last copy of a commit in it, and keeps both the directory and
/// the row. One home being part-way removed must not make the sweep act on, or skip,
/// any other.
#[test]
fn a_home_the_reading_keeps_survives_a_killed_sweep_of_another() {
    let machine = machine();
    machine.unit(SLUG);
    let going = mark(&reclaimed(&machine, SLUG));
    part_removed(&going);

    let home = machine.unit(OTHER);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    let sibling = sibling_holding(machine.root(), SIBLING, &home, &tip);
    let kept = reclaimed(&machine, OTHER);
    git(&sibling, &["update-ref", "-d", &format!("refs/heads/{COPY}")]);
    git(&sibling, &["reflog", "expire", "--expire=now", "--all"]);

    let report = sweep(&machine);

    assert!(!going.exists(), "the part-removed home stayed");
    assert!(report.contains(&format!("kept: {}", &tip[..8])), "{report}");
    assert!(kept.is_dir(), "the sweep removed the home it said it kept");
    assert_eq!(git(&kept, &["cat-file", "-t", &tip]), "commit", "the work is not readable");
    assert_eq!(rows(&machine), vec![kept], "the kept home is the one row left");
    settled(&machine);
}

/// A shape older than the mark: a home part removed under its own name.
///
/// A release before this one removed the tree at the home's own name, so a sweep it
/// killed could leave one of these. No reading can be made of it and no mark says a
/// reading was made, so the sweep keeps it and names it. That is the honest answer, and
/// it is why the mark exists: the directory is a leftover for a person to remove, and
/// nothing on this machine can now say what it still holds.
#[test]
fn a_home_part_removed_under_its_own_name_is_named_and_kept() {
    let machine = machine();
    machine.unit(SLUG);
    let trash = reclaimed(&machine, SLUG);
    part_removed(&trash);

    let report = sweep(&machine);

    assert!(trash.is_dir(), "the sweep removed a directory it could not read");
    assert_eq!(rows(&machine), vec![trash.clone()], "and kept the row with it");
    assert!(report.contains(&trash.display().to_string()), "the report names it: {report}");

    // And tells the person the one move that finishes it. "not a git repository" alone
    // reads as a fault and says nothing to do; this directory is kept for ever until
    // somebody removes it, so the line that names it says so.
    assert!(report.contains(TOLD), "the line says nothing to do: {report}");
}

/// A home under its own name and under the removing name at once is kept, and the sweep
/// says so rather than reporting the home as removed.
///
/// A person makes this shape: they rename or copy a directory inside the trash after a
/// killed sweep, or restore one out of a backup. It is the one state the ordering does
/// not reach on its own, and the dangerous reading of it is the report's: a sweep that
/// trusted the mark's name would remove what is under the removing name, drop the row,
/// print the home under REMOVED, and leave the whole home standing under its own name
/// with nothing pointing at it — the directory nothing knows about that this module's
/// order exists to prevent.
///
/// Nothing guards against it. The rename does. The home is there, so it is read again;
/// the move onto a name a tree already occupies fails; the sweep names the directory
/// and keeps the row. What that proves is that no reading, no mark and no row decides
/// this — one filesystem operation that cannot half succeed does.
///
/// A second sweep, after the person has resolved the two directories, finishes normally.
#[test]
fn a_home_under_both_names_is_kept_and_the_next_sweep_finishes_it() {
    let machine = machine();
    machine.unit(SLUG);
    let trash = reclaimed(&machine, SLUG);
    let going = removing(&trash);

    // The home under both names, each a whole home: the person's copy is the one under
    // the removing name, so what a trusting sweep would remove holds everything.
    copied(&trash, &going);
    assert!(trash.join(".git").is_dir(), "the home under its own name is whole");
    assert!(going.join(".git").is_dir(), "the home under the removing name is whole");

    let report = sweep(&machine);

    assert!(trash.is_dir(), "the sweep took the home out from under its own name");
    assert!(going.is_dir(), "the sweep removed the tree under the removing name");
    assert_eq!(rows(&machine), vec![trash.clone()], "the sweep dropped the row");
    assert!(report.contains(&trash.display().to_string()), "the report names it: {report}");
    // The table of removed homes is not in the report at all, which is the honesty this
    // property is about: a sweep that trusted the mark would have printed this home in it.
    assert!(!report.contains("REMOVED"), "the sweep called a home it kept removed: {report}");

    // The person resolves it, which is the one move the report can ask for: the
    // directory a killed sweep was removing goes, and the home stays.
    std::fs::remove_dir_all(&going).unwrap();

    sweep(&machine);

    assert!(!trash.exists(), "the second sweep did not finish the removal");
    assert!(rows(&machine).is_empty(), "the second sweep kept the row of a home that has gone");
    settled(&machine);
}

/// Copy a directory tree, which is how a person makes a second copy of a trashed home.
fn copied(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let at = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copied(&entry.path(), &at);
        } else {
            std::fs::copy(entry.path(), &at).unwrap();
        }
    }
}
