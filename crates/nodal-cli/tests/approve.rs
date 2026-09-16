//! `nodal approve`: the verb that writes the approval record and nothing else.
//!
//! A project whose recipe declares a `post_new` hook refused every `nodal new` until the
//! hook was approved, and the only command that wrote an approval was `nodal init`.
//! `nodal init` refuses when `nodal.toml` is there, so the one route through was
//! `nodal init --force`, which rewrites the recipe and replaces every comment in it with
//! the template's own. A person answering "yes, run this line" lost the notes they had
//! written.
//!
//! What this suite holds to: approving runs the hook, and the recipe is byte for byte
//! the file the person wrote.

#![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

mod state;

use std::path::Path;
use std::process::{Command, Output};

use state::Machine;

/// A project with a recipe a person wrote, comments and all, and a hook in it.
///
/// The hook writes a file, because what proves an approval is that the command ran.
///
/// The project names no package manager and carries no lockfile, so a base build of it
/// runs no install. This suite is about the approval record, and a fixture that made
/// `nodal new` reach for `pnpm` would need that program on every host the suite runs on.
fn write_project(root: &Path) {
    std::fs::write(root.join("README.md"), "# the project\n").unwrap();
    std::fs::write(root.join("nodal.toml"), RECIPE).unwrap();
    git(root, &["init", "--initial-branch", "main"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", "the project"]);
}

/// The recipe the person wrote. The comment is the thing `init --force` would take.
const RECIPE: &str = "# ours: the post_new hook installs the python half\n\
                      backend = \"native\"\n\
                      \n\
                      [hooks]\n\
                      post_new = \"touch hook-ran\"\n";

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(root).output().unwrap();
    assert!(status.status.success(), "git {args:?}: {status:?}");
}

fn nodal(machine: &Machine, root: &Path, args: &[&str]) -> Output {
    machine.nodal().args(args).current_dir(root).output().unwrap()
}

/// The whole loop in one test: the refusal, the verb it names, the record that verb
/// writes, and the recipe that is untouched afterwards.
#[test]
fn approving_a_hook_runs_it_and_leaves_the_recipe_byte_for_byte() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);

    // The refusal names the verb that answers it, and that verb is not `nodal init`.
    let refused = nodal(&machine, root, &["new", "--name", "one"]);
    let said = String::from_utf8(refused.stderr).unwrap();
    assert!(said.contains("is not approved"), "{said}");
    assert!(said.contains("nodal approve"), "the refusal names the verb: {said}");
    assert!(!said.contains("nodal init"), "and does not send a person to init: {said}");

    let before = std::fs::read_to_string(root.join("nodal.toml")).unwrap();
    let approved = nodal(&machine, root, &["approve", "--yes"]);
    assert!(approved.status.success(), "{approved:?}");
    let report = String::from_utf8(approved.stdout).unwrap();
    assert!(report.contains("post_new"), "{report}");
    assert!(report.contains("touch hook-ran"), "the command is read before it is run: {report}");

    let after = std::fs::read_to_string(root.join("nodal.toml")).unwrap();
    assert_eq!(before, after, "approving rewrote the recipe");

    let made = nodal(&machine, root, &["new", "--name", "two"]);
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let home = home_of(&machine, root, "two");
    assert!(home.join("hook-ran").exists(), "the approved hook did not run in {}", home.display());
}

/// The record is the one file this verb writes, and the commands reach the person before
/// it is written.
#[test]
fn the_record_names_the_project_and_the_commands_reach_the_person_first() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);

    let approved = nodal(&machine, root, &["approve", "--yes"]);
    assert!(approved.status.success(), "{approved:?}");

    let asked = String::from_utf8(approved.stderr).unwrap();
    assert!(asked.contains("touch hook-ran"), "the commands were not shown: {asked}");
    assert!(asked.contains("run on your account"), "{asked}");

    let record = std::fs::read_to_string(machine.path().join("hooks.toml")).unwrap();
    assert!(record.contains("post_new"), "the record does not name the phase: {record}");
    assert!(
        record.contains(root.to_string_lossy().trim()),
        "the record is not keyed by the project: {record}"
    );
}

/// Approval is re-made from scratch, so a command a person edits is refused until they
/// read the new text and accept it.
#[test]
fn a_command_that_changed_is_refused_until_it_is_approved_again() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);
    assert!(nodal(&machine, root, &["approve", "--yes"]).status.success());

    std::fs::write(root.join("nodal.toml"), RECIPE.replace("hook-ran", "something-else")).unwrap();
    let refused = nodal(&machine, root, &["new", "--name", "one"]);
    let said = String::from_utf8(refused.stderr).unwrap();
    assert!(said.contains("is not approved"), "{said}");
    assert!(said.contains("something-else"), "the refusal shows the new text: {said}");

    assert!(nodal(&machine, root, &["approve", "--yes"]).status.success());
    let made = nodal(&machine, root, &["new", "--name", "two"]);
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
}

/// `--print` is the reading with no decision in it: the commands, and no record.
#[test]
fn print_shows_the_commands_and_approves_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);

    let printed = nodal(&machine, root, &["approve", "--print", "--json"]);
    assert!(printed.status.success(), "{printed:?}");
    let value: serde_json::Value = serde_json::from_slice(&printed.stdout).unwrap();
    assert_eq!(value["commands"][0]["phase"], "post_new");
    assert_eq!(value["commands"][0]["command"], "touch hook-ran");
    assert!(!machine.path().join("hooks.toml").exists(), "--print wrote a record");

    let refused = nodal(&machine, root, &["new", "--name", "one"]);
    assert!(String::from_utf8(refused.stderr).unwrap().contains("is not approved"));
}

/// The record is never written by a run that nothing could answer. A test harness, a
/// pipe and a hook of another tool all reach this path, and none of them read the
/// commands, so the run is refused rather than asked — the rule `nodal merge` and
/// `nodal uninstall` already hold to.
#[test]
fn a_run_that_cannot_be_asked_records_nothing_and_names_the_flag() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);

    let refused = nodal(&machine, root, &["approve"]);
    assert!(!refused.status.success(), "it approved without being answered: {refused:?}");
    let said = String::from_utf8(refused.stderr).unwrap();
    assert!(said.contains("touch hook-ran"), "the commands are still shown: {said}");
    assert!(said.contains("--yes"), "the flag that answers is not named: {said}");
    assert!(said.contains("nothing here can answer"), "{said}");
    assert!(!machine.path().join("hooks.toml").exists(), "a record was written anyway");
}

/// A project that declares no hook has nothing to put to a person, so nothing is asked
/// and the answer is the empty report.
#[test]
fn a_project_with_no_hook_asks_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);
    std::fs::write(root.join("nodal.toml"), "backend = \"native\"\n").unwrap();

    let approved = nodal(&machine, root, &["approve"]);
    assert!(approved.status.success(), "{approved:?}");
    assert!(String::from_utf8(approved.stdout).unwrap().contains("declares no hook"));
}

/// The home of a unit, read out of the list the way every other suite reads it.
fn home_of(machine: &Machine, root: &Path, slug: &str) -> std::path::PathBuf {
    let listed = nodal(machine, root, &["show", slug, "--json"]);
    assert!(listed.status.success(), "{listed:?}");
    let value: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let home = value["unit"]["environment"]["home"].as_str().unwrap();
    std::path::PathBuf::from(home)
}
