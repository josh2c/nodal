//! What a home a Claude Code session was given looks like to Git.
//!
//! The provider hook makes a unit and then puts a `.claude/settings.json` in the home
//! it answers with, because Claude Code reads that file from the directory the session
//! works in and the project's copy is no longer in scope
//! ([`nodal_core::adapters::claude_code`]).
//!
//! A file Nodal writes into a home is a file no one else may ever see. The rule is the
//! one the activation files and the memory are held to: it is hidden from `git status`
//! through the home's own `.git/info/exclude`, and it is named in
//! [`nodal_core::lifecycle::uniqueness`]. Break either half and three things go at
//! once — the home is dirty the moment it exists, `nodal reclaim` refuses it because it
//! "holds work that is only here", and `nodal merge` commits Nodal's own file onto the
//! unit's branch and ships it to the target.
//!
//! The fixture's ignore file is rewritten here to hold one line, because the property
//! is about a project that says nothing at all about `.claude/`. A project that ignores
//! it would pass these tests whatever Nodal wrote, which is exactly the premise this
//! suite must not rest on.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use nodal_safety::InState as _;
use nodal_safety::{Machine, git, stderr, stdout};

/// The only rule the project under test has. It covers what the stub package manager
/// writes and nothing else, so a file Nodal leaves anywhere is a file `git status`
/// reports.
const ONLY_RULE: &str = "node_modules/\n";

/// The settings file, relative to a home.
const SETTINGS: &str = ".claude/settings.json";

/// A machine whose project ignores `node_modules/` and nothing else.
fn machine() -> Machine {
    let machine = Machine::new();
    std::fs::write(machine.source.join(".gitignore"), ONLY_RULE).unwrap();
    git(&machine.source, &["add", "--", ".gitignore"]);
    git(
        &machine.source,
        &["commit", "--quiet", "--message", "ignore only what the install writes"],
    );
    machine
}

/// Fire the provider hook in `cwd` and answer with what it printed.
fn worktree_create(machine: &Machine, cwd: &Path) -> (bool, String, String) {
    let payload = format!(
        "{{\"transcript_path\":\"\",\"cwd\":\"{}\",\"hook_event_name\":\"WorktreeCreate\",\
         \"name\":\"worker-import\"}}",
        cwd.display()
    );
    let mut child = machine
        .command(&["claude-code", "worktree-create"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    (output.status.success(), stdout(&output), stderr(&output))
}

/// The home a session started in the project would be given.
fn session_home(machine: &Machine) -> PathBuf {
    let source = machine.source.clone();
    let (ok, answered, said) = worktree_create(machine, &source);
    assert!(ok, "the provider refused to make a unit: {said}");
    let home = PathBuf::from(answered.trim());
    assert!(home.is_dir(), "{} was printed and is not there", home.display());
    home
}

#[test]
fn a_home_a_session_was_given_is_clean_the_moment_it_exists() {
    let machine = machine();
    let home = session_home(&machine);

    assert!(home.join(SETTINGS).is_file(), "the session was given no hooks to run");
    let status = git(&home, &["status", "--porcelain"]);
    assert!(status.is_empty(), "the home is dirty the moment a session was given it: {status}");
}

#[test]
fn a_home_a_session_was_given_can_be_reclaimed() {
    let machine = machine();
    let home = session_home(&machine);

    let reclaimed = machine.nodal(&["reclaim", "worker-import"]);
    assert!(
        reclaimed.status.success(),
        "a home holding nothing but nodal's own file was refused: {}",
        stderr(&reclaimed)
    );
    assert!(!home.exists(), "the reclaim reported success and left the home");
}

#[test]
fn a_merge_of_a_unit_that_did_no_work_commits_nothing() {
    let machine = machine();
    let home = session_home(&machine);
    let before = git(&home, &["rev-parse", "HEAD"]);

    let merged = machine.nodal(&["merge", "--yes", "worker-import"]);
    assert!(merged.status.success(), "{}", stderr(&merged));

    let after = git(&machine.source, &["rev-parse", "HEAD"]);
    assert_eq!(after, before, "a unit that did no work moved the branch everybody merges into");
    let told = format!("{}{}", stdout(&merged), stderr(&merged));
    assert!(!told.contains(SETTINGS), "nodal's own file was named in what the merge did: {told}");
}

#[test]
fn a_request_from_inside_a_home_answers_that_home_and_makes_nothing() {
    let machine = machine();
    let home = session_home(&machine);
    let before = machine.homes();

    let (ok, answered, said) = worktree_create(&machine, &home);

    assert!(ok, "a session already in a home was refused: {said}");
    assert_eq!(Path::new(answered.trim()), home, "it was sent somewhere other than where it is");
    assert_eq!(machine.homes(), before, "a unit of a unit was made");
}
