//! What a default `nodal uninstall` leaves behind, and whether Git can still use it.
//!
//! The promise is a qualified one, and the qualification is the whole of it. A default
//! uninstall removes what Nodal installed on the machine: the block in a shell start-up
//! file, the script that block loads, and the provider hooks. It does not remove the
//! state directory, and every unit home is inside it. So a person who removes Nodal
//! keeps their homes, and the claim worth asserting is not that the homes are gone but
//! that what is left is usable by somebody who no longer has the tool.
//!
//! `--state` is the other half, and it is a different promise: it removes the state
//! directory and the unit homes in it, it refuses while a home holds work that exists
//! nowhere else, and `--force` says what was accepted losing. That half is asserted in
//! `crates/nodal-cli/tests/uninstall.rs`, where the refusal and the forced removal are
//! two named tests.
//!
//! | property | what a break would look like | test |
//! |---|---|---|
//! | a home Git can still read | a home outlives the uninstall and `git log` in it fails | `a_home_that_outlived_an_uninstall_reads_its_own_history` |
//! | a home that borrows nothing | the objects are the base's, so a home is empty once the base goes | `a_home_that_outlived_an_uninstall_borrows_no_objects` |
//! | a home that needs no nodal | the configuration names a path only Nodal puts on a machine | `a_home_that_outlived_an_uninstall_needs_nothing_of_nodals` |
//! | homes are what `--state` is for | a default uninstall removes a home nobody asked it to | `a_default_uninstall_removes_no_home` |
//!
//! Every reading below is taken with `nodal` off the search path and with every
//! variable Nodal names out of the environment. A test that left the binary reachable
//! would prove that Git works on a machine that still has Nodal, which is not the
//! claim.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_safety::{Machine, git, stderr, stdout};
use tempfile::TempDir;

/// The name of the binary, which no search path in this file may resolve.
const BINARY: &str = "nodal";

/// The variables Nodal reads. A child that inherited one would be a child Nodal is
/// still on, whatever its search path says.
const NODAL_VARS: [&str; 4] =
    ["NODAL_HOME", "NODAL_SECRETS_FILE", "NODAL_HOOKS_FILE", "NODAL_CD_FILE"];

/// What the unit commits, so that the history read back afterwards is the unit's own
/// and not only the fixture's first commit.
const MESSAGE: &str = "the work this unit was made for";

/// A machine whose uninstall reads start-up files belonging to nobody.
///
/// The kit's own fixture names no home directory, because nothing else in the suite
/// writes under one. An uninstall reads `.bashrc` and the person's own Claude Code
/// settings, so a test that let it reach the real ones would edit the shell of whoever
/// ran the suite.
struct Person {
    /// The machine under test.
    machine: Machine,
    /// The temporary root the home directory is under, kept so it outlives the test.
    _root: TempDir,
}

impl Person {
    /// A machine with the shell integration installed and one unit made.
    ///
    /// Both steps go through the binary. A home a test arranged to look like a home
    /// would prove nothing about the homes `nodal new` makes.
    fn with_a_unit(slug: &str) -> (Self, PathBuf) {
        let root = TempDir::new().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let machine = Machine::new()
            .with_env(("HOME", home.to_str().unwrap()))
            .with_env(("USERPROFILE", home.to_str().unwrap()));
        let installed = machine.nodal(&["shell-init", "--install", "bash"]);
        assert!(installed.status.success(), "{}", stderr(&installed));
        let person = Self { machine, _root: root };
        let unit = person.machine.unit(slug);
        Self::commit_something(&unit);
        (person, unit)
    }

    /// Commit one file in the unit's home, as the work the unit was made for.
    fn commit_something(unit: &Path) {
        std::fs::write(unit.join("what-the-unit-did.txt"), "a commit of this unit's own\n")
            .unwrap();
        git(unit, &["add", "--", "what-the-unit-did.txt"]);
        git(unit, &["commit", "--quiet", "--message", MESSAGE]);
    }

    /// Take Nodal off this machine, leaving the state directory and the homes in it.
    fn uninstall(&self) {
        let removed = self.machine.nodal(&["uninstall", "--yes"]);
        assert!(removed.status.success(), "{}", stderr(&removed));
        assert!(
            !stdout(&removed).contains("nodal has installed nothing"),
            "there was nothing to remove, so the reading afterwards is about nothing"
        );
    }
}

/// A search path with every directory that holds a `nodal` taken out of it.
///
/// The binary under test is somewhere in the target directory, and a person's own copy
/// may be anywhere. Both are removed by asking what each directory holds rather than by
/// naming either.
fn without_nodal() -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let kept: Vec<PathBuf> = std::env::split_paths(&inherited)
        .filter(|directory| !directory.join(BINARY).exists())
        .collect();
    std::env::join_paths(kept).expect("a search path without the binary")
}

/// One `git` call inside a home, on a machine that no longer has Nodal.
///
/// The global and the system configuration are shut out for the reason the kit shuts
/// them out: a reading must not depend on what the person running the tests keeps in
/// theirs. What is added here is the search path and the variables, which are what make
/// this a reading of a home rather than of a home beside a working install.
fn git_without_nodal(home: &Path, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(home)
        .args(args)
        .env("PATH", without_nodal())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    for name in NODAL_VARS {
        command.env_remove(name);
    }
    command.output().expect("git runs")
}

/// The same, insisted upon, as trimmed text.
fn read(home: &Path, args: &[&str]) -> String {
    let output = git_without_nodal(home, args);
    assert!(output.status.success(), "git {args:?}: {}", stderr(&output));
    stdout(&output).trim_end().to_owned()
}

/// The search path this file builds really does resolve no `nodal`.
///
/// Every other test here is worth nothing if it does not, and a filter over the
/// inherited path is the kind of thing that quietly stops filtering.
#[test]
fn the_search_path_these_readings_use_resolves_no_nodal() {
    let path = without_nodal();
    let found: Vec<PathBuf> = std::env::split_paths(&path)
        .map(|directory| directory.join(BINARY))
        .filter(|candidate| candidate.exists())
        .collect();
    assert!(
        found.is_empty(),
        "the readings would still have run on a machine with nodal: {found:?}"
    );
}

/// A home that outlived a default uninstall is a repository Git can read: its own
/// commit is in the log, the tree is clean, and the object store is whole.
#[test]
fn a_home_that_outlived_an_uninstall_reads_its_own_history() {
    let (person, home) = Person::with_a_unit("survivor");

    person.uninstall();

    assert!(home.is_dir(), "the home went with the uninstall");
    let log = read(&home, &["log", "--format=%s"]);
    assert!(log.contains(MESSAGE), "the unit's own commit is not readable: {log}");
    assert_eq!(read(&home, &["status", "--porcelain"]), "", "the home is dirty at rest");
    let checked = git_without_nodal(&home, &["fsck", "--no-progress"]);
    assert!(checked.status.success(), "the object store is not whole: {}", stderr(&checked));
}

/// The home's objects are the home's. A repository that borrowed them would be a
/// repository that empties the day the tree it borrowed from goes.
#[test]
fn a_home_that_outlived_an_uninstall_borrows_no_objects() {
    let (person, home) = Person::with_a_unit("lender");
    let base = person.machine.base();

    person.uninstall();

    assert!(home.join(".git").is_dir(), "the home is a linked worktree, not a repository");
    let alternates = home.join(".git/objects/info/alternates");
    assert!(
        !alternates.exists(),
        "the home borrows objects: {}",
        std::fs::read_to_string(&alternates).unwrap_or_default()
    );
    let roots = read(&home, &["rev-parse", "--absolute-git-dir", "--show-toplevel"]);
    let inside = std::fs::canonicalize(&home).unwrap();
    for line in roots.lines() {
        let named = std::fs::canonicalize(line).unwrap_or_else(|_| PathBuf::from(line));
        assert!(named.starts_with(&inside), "git looks outside the home: {line}");
    }
    assert!(base.is_dir(), "the base is still there, so a borrow would not have been noticed");
    assert!(
        !inside.starts_with(&base),
        "the fixture put the home inside the base it is read against"
    );
}

/// Nothing in the home's Git configuration names a path only Nodal puts on a machine.
///
/// `gc.auto = 0` is set by the clone scrub and stays: it is a plain Git setting, it
/// names nothing, and a repository with automatic collection off is one any Git can
/// use. `remote.origin.url` names where the project came from, which is the person's
/// own and not Nodal's; the fixture has no remote, so it is their checkout here and the
/// URL they cloned from on a real machine. What must not be there is a hooks path, an
/// include or an object store outside the home, or any path under the state directory,
/// because each of those is something the person no longer has a tool to make.
#[test]
fn a_home_that_outlived_an_uninstall_needs_nothing_of_nodals() {
    let (person, home) = Person::with_a_unit("independent");
    let state = person.machine.state.clone();

    person.uninstall();

    let configured = read(&home, &["config", "--local", "--list"]);
    for key in ["core.hookspath", "include.path", "includeif", "core.alternaterefsprefixes"] {
        assert!(!configured.to_lowercase().contains(key), "{key} is set in a home: {configured}");
    }
    let root = state.to_string_lossy();
    assert!(
        !configured.contains(root.as_ref()),
        "the configuration names {root}, which is nodal's own directory: {configured}"
    );
    let origin = read(&home, &["config", "--local", "--get", "remote.origin.url"]);
    assert!(
        !PathBuf::from(&origin).starts_with(&state),
        "origin is a directory of nodal's rather than where the project came from: {origin}"
    );
}

/// A default uninstall is not `--state`. The home, the registry and the state directory
/// are all still there, and the person is told nothing about them going.
#[test]
fn a_default_uninstall_removes_no_home() {
    let (person, home) = Person::with_a_unit("kept");
    let state = person.machine.state.clone();

    person.uninstall();

    assert!(home.is_dir(), "a default uninstall removed a unit home");
    assert!(state.join("registry.db").is_file(), "a default uninstall removed the registry");
    assert!(
        person.machine.base().is_dir(),
        "a default uninstall removed the base the homes were made from"
    );
}
