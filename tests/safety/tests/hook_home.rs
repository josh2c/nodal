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
use nodal_safety::{Machine, git, stderr, stdout, tree};

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

/// Fire the provider hook in `cwd` with a payload that names no directory at all.
///
/// The desktop application sends an absolute `cwd`; nothing promises every version of
/// every client will. An empty one means the directory the hook is running in, and that
/// directory may be a home.
fn worktree_create_without_cwd(machine: &Machine, cwd: &Path) -> (bool, String, String) {
    let payload = "{\"hook_event_name\":\"WorktreeCreate\",\"name\":\"worker-import\"}";
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

/// A payload with no `cwd` is a request from the directory the hook runs in, and that
/// directory is a home. Reading it as "no path at all" made the guard miss, and the
/// session got a unit of a unit.
#[test]
fn a_request_from_a_home_with_no_cwd_in_the_payload_answers_that_home() {
    let machine = machine();
    let home = session_home(&machine);
    let before = machine.homes();

    let (ok, answered, said) = worktree_create_without_cwd(&machine, &home);

    assert!(ok, "a session already in a home was refused: {said}");
    assert_eq!(Path::new(answered.trim()), home, "it was sent somewhere other than where it is");
    assert_eq!(machine.homes(), before, "a unit of a unit was made");
}

/// Many projects commit an `.envrc`: direnv and Nix users do. Rewriting it puts every
/// home of the project in `git status` before the session starts, which is a home
/// `nodal reclaim`, `nodal done` and `nodal gc` all refuse.
#[test]
fn a_project_that_commits_its_envrc_gets_a_clean_home() {
    let machine = machine();
    let theirs = "use flake\n";
    std::fs::write(machine.source.join(".envrc"), theirs).unwrap();
    git(&machine.source, &["add", "--", ".envrc"]);
    git(&machine.source, &["commit", "--quiet", "--message", "the project's own direnv file"]);

    let home = session_home(&machine);

    assert_eq!(
        std::fs::read_to_string(home.join(".envrc")).unwrap(),
        theirs,
        "a file git tracks was overwritten, so the home is modified from birth"
    );
    let status = git(&home, &["status", "--porcelain"]);
    assert!(status.is_empty(), "the home is dirty the moment a session was given it: {status}");
    assert!(
        home.join(".nodal").join("env").is_file(),
        "the values went with the file that was left alone"
    );

    let reclaimed = machine.nodal(&["reclaim", "worker-import"]);
    assert!(reclaimed.status.success(), "the home could not be let go of: {}", stderr(&reclaimed));
}

/// A file in a home that Git does not track is Nodal's to hide whether or not Nodal
/// wrote its bytes. A base build or a `post_new` hook can leave one, and a settings
/// file nobody hid is a home the uniqueness check calls dirty.
#[test]
fn an_untracked_settings_file_a_home_already_had_is_hidden_and_kept() {
    let machine = machine();
    let home = session_home(&machine);
    let path = home.join(SETTINGS);
    let theirs = "{\n  \"permissions\": {\n    \"deny\": [\"Bash(rm:*)\"]\n  }\n}\n";
    unhide_settings(&home);
    std::fs::write(&path, theirs).unwrap();

    let shown = machine.nodal(&["show", "worker-import"]);
    assert!(shown.status.success(), "{}", stderr(&shown));

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        theirs,
        "a file nodal did not write was overwritten"
    );
    let status = git(&home, &["status", "--porcelain"]);
    assert!(status.is_empty(), "a file nobody hid is work the uniqueness check refuses: {status}");
}

/// A directory carrying a `.nodal/id` no row matches is not a home this machine may
/// hand to a session: nothing here can list it, merge it or reclaim it. One restored
/// from a backup or copied out of a trash folder still carries its marker.
#[test]
fn a_marked_directory_the_registry_does_not_know_is_not_answered_with() {
    let machine = machine();
    let restored = machine.source.parent().unwrap().join("restored");
    tree::copy(&machine.source, &restored);
    std::fs::create_dir_all(restored.join(".nodal")).unwrap();
    std::fs::write(restored.join(".nodal").join("id"), "01J8Z6H000000000000000001\n").unwrap();

    let (ok, answered, said) = worktree_create(&machine, &restored);

    assert!(ok, "the provider refused rather than making a unit: {said}");
    assert_ne!(
        Path::new(answered.trim()),
        restored,
        "a directory no row knows was handed to a session as a home"
    );
    assert!(Path::new(answered.trim()).join(".nodal").join("id").is_file());
}

/// A `.nodal/id` anywhere above the project used to end the session: the read error
/// travelled out of the hook and Claude got no path. A marker that cannot be read says
/// nothing about whose home this is, so it is not one.
///
/// The copy sits under a directory of its own, away from the state directory, because
/// a marker above the state directory is a different refusal ([`nodal_core`]'s
/// placement guard) and this test is about the read.
#[test]
fn an_unreadable_marker_above_the_project_does_not_end_the_session() {
    let machine = machine();
    let elsewhere = machine.source.parent().unwrap().join("elsewhere");
    let copy = elsewhere.join("project");
    tree::copy(&machine.source, &copy);
    std::fs::create_dir_all(elsewhere.join(".nodal")).unwrap();
    std::fs::write(elsewhere.join(".nodal").join("id"), "not a unit identifier\n").unwrap();

    let (ok, answered, said) = worktree_create(&machine, &copy);

    assert!(ok, "a marker nobody can read, above the project, ended the session: {said}");
    assert!(Path::new(answered.trim()).is_dir(), "{answered}");
}

/// Take Nodal's line for the settings file back out of the home's exclude file, so that
/// the next command has to put it there again.
fn unhide_settings(home: &Path) {
    let exclude = git(home, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let path = Path::new(exclude.trim()).join("info").join("exclude");
    let held = std::fs::read_to_string(&path).unwrap_or_default();
    let kept: Vec<&str> =
        held.lines().filter(|line| line.trim_end() != "/.claude/settings.json").collect();
    std::fs::write(&path, format!("{}\n", kept.join("\n"))).unwrap();
}
