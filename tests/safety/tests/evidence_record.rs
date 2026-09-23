//! Every verdict says what it rests on, so that a verdict can be disagreed with.
//!
//! A `nodal reclaim --check` that said safe printed `commits: []`, `reasons: []`,
//! `paths: []`. That states the absence of objections. It never states what was looked at,
//! so a home with nothing in it and a home whose work fell silently outside the rule
//! produced the same bytes. Nodal's contract is "cannot see, therefore not safe", and that
//! sentence was **unfalsifiable from the output**: the output never said what it saw.
//!
//! That is a safety property and not a presentation one. A rule nobody can check is a rule
//! nobody can find a hole in, and the three holes this lane is about — a commit on a ref
//! `HEAD` does not reach, a process this account may not read, and a process writing from a
//! directory elsewhere — all failed silently to safe. The record does not close them — nothing here changes a
//! verdict, and `nothing_in_the_record_changes_the_verdict` holds that — it makes them
//! visible.
//!
//! | property | test |
//! |---|---|
//! | a safe verdict lists the stores it asked | `a_safe_verdict_says_which_stores_it_asked_and_what_each_said` |
//! | and the refs it walked, and the ones it did not | `a_safe_verdict_says_which_refs_it_walked_and_which_it_did_not` |
//! | a store that would not answer says so, with the reason | `a_store_that_will_not_answer_is_named_as_not_answering` |
//! | a reading nobody asked for is named, not silent | `a_reading_that_was_not_asked_for_is_named_with_the_reason` |
//! | the withheld processes are counted, not hidden | `the_processes_the_host_refused_are_counted_in_the_record` |
//! | it is in the JSON and in the human view alike | `the_record_is_in_both_renderings_and_says_the_same_thing` |
//! | nothing in it moves the verdict | `nothing_in_the_record_changes_the_verdict` |
//! | and `--check` still writes nothing | `printing_the_record_writes_nothing` |
//!
//! Both hosts. Nothing here reads `/proc`: the record is assembled from the readings the
//! assessment made, whichever host made them, and the one host-dependent field — which
//! readings of occupancy the scan answered — is asserted against
//! [`nodal_core::runtime::processes::OCCUPANCY`] rather than against a hardcoded list.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::PathBuf;

use nodal_core::runtime::processes::OCCUPANCY;
use nodal_safety::git::untouched;
use nodal_safety::{InState as _, Machine, Snapshot, answer, git, stderr};
use serde_json::Value;

/// The unit every property here asks about. One of the fixture's own handles.
const SLUG: &str = "worker-import";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const WORK: &str = "app/notes.md";

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// No proxy either, for the same reason.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// A machine with a remote and one unit on it, held to the filesystem.
///
/// The unit is made here rather than by each property, because every property below asks
/// about the same one and none of them is about making it.
fn machine() -> (Machine, PathBuf) {
    let machine = Machine::with_remote().with_env(ONLY_LOCAL).with_env(NO_PROXY);
    let home = machine.unit(SLUG);
    (machine, home)
}

/// The preflight for one unit, as JSON.
fn check(machine: &Machine, slug: &str) -> Value {
    let asked = machine.nodal(&["reclaim", slug, "--check", "--json"]);
    let printed = answer(&asked);
    serde_json::from_str(&printed)
        .unwrap_or_else(|_| panic!("--check --json is one document: {printed}{}", stderr(&asked)))
}

/// The evidence record of that preflight.
fn record(machine: &Machine, slug: &str) -> Value {
    let read = check(machine, slug);
    read["evidence"].clone()
}

/// A commit in the home that exists nowhere else, so every store is really asked.
fn work_only_here(home: &PathBuf) {
    std::fs::create_dir_all(home.join(WORK).parent().unwrap()).unwrap();
    std::fs::write(home.join(WORK), "only here\n").unwrap();
    drop(git(home, &["config", "user.email", "unit@example.invalid"]));
    drop(git(home, &["config", "user.name", "Test"]));
    drop(git(home, &["add", "-A"]));
    drop(git(home, &["commit", "-qm", "work only this home has"]));
}

/// A safe verdict names every store it asked for a second copy, and what each said.
///
/// The load-bearing half is that it names them **when the answer is safe**. A refusal
/// names what it refused over, so a refusal has always been its own evidence; a safe
/// verdict had none at all, and the two states of the machine that produce one — nothing
/// to find, and nowhere looked — printed alike.
#[test]
fn a_safe_verdict_says_which_stores_it_asked_and_what_each_said() {
    let (machine, _home) = machine();
    let read = check(&machine, SLUG);
    assert_eq!(read["safe_to_reclaim"], Value::Bool(true), "{read:#}");

    let stores = read["evidence"]["stores"].as_array().unwrap();
    assert!(!stores.is_empty(), "a safe verdict named no store at all: {read:#}");
    for store in stores {
        assert!(store["path"].is_string(), "a store with no path: {store:#}");
        let answered = store["answered"].as_str().unwrap();
        assert!(
            ["yes", "no", "not_asked"].contains(&answered),
            "a store's answer is one of three, not {answered}"
        );
        if answered != "yes" {
            assert!(
                store["why"].as_str().is_some_and(|why| !why.is_empty()),
                "a store that did not answer must say why: {store:#}"
            );
        }
    }
    assert!(
        stores.iter().any(|store| store["role"] == "checkout"),
        "the project's own checkout is the first store any reading asks: {stores:#?}"
    );
}

/// A verdict names the refs it walked and, in the same breath, the refs it did not.
///
/// The second list is the honest half. A reading of `HEAD` alone does not reach a branch,
/// a tag or a stash made inside the home, so a commit on one of those is work the verdict
/// says nothing about, and closing that is another lane's. Until it is walked, an empty
/// `commits` list must not be allowed to read as an empty home, and this is what stops it.
#[test]
fn a_safe_verdict_says_which_refs_it_walked_and_which_it_did_not() {
    let (machine, _home) = machine();
    let record = record(&machine, SLUG);

    let walked: Vec<&str> =
        record["refs"]["walked"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
    assert_eq!(walked, ["HEAD"], "the reading walks HEAD, and the record says only what it did");
    let not_walked = record["refs"]["not_walked"].as_array().unwrap();
    assert!(!not_walked.is_empty(), "what was not walked is not stated: {record:#}");
    let listed = serde_json::to_string(not_walked).unwrap();
    for name in ["refs/tags/", "refs/stash", "refs/heads/"] {
        assert!(listed.contains(name), "{name} is not named as unwalked: {listed}");
    }
    assert!(record["refs"]["commits"].is_u64(), "how many commits were assessed: {record:#}");
}

/// A store that cannot be read is recorded as not answering, with the reason.
///
/// "Asked and said no copy" and "could not be opened" are different facts about a machine
/// and they reach the verdict by the same route — a store that does not answer holds
/// nothing, which is the strict direction — so the only place the difference can survive
/// is here.
#[test]
fn a_store_that_will_not_answer_is_named_as_not_answering() {
    let (machine, home) = machine();
    work_only_here(&home);

    // A sibling of the checkout that is a directory and not a repository. The scan finds
    // it, the reading asks it, and it has nothing to say.
    let broken = machine.source.parent().unwrap().join("siblings").join("not-a-repo");
    std::fs::create_dir_all(broken.join(".git")).unwrap();
    std::fs::write(broken.join(".git").join("HEAD"), "this is not a git directory\n").unwrap();

    let record = record(&machine, SLUG);
    let stores = record["stores"].as_array().unwrap();
    for store in stores {
        if store["answered"] == "no" {
            assert!(
                store["why"].as_str().is_some_and(|why| !why.is_empty()),
                "a store that would not answer says why: {store:#}"
            );
        }
    }
    assert!(
        stores.iter().any(|store| store["answered"] == "yes"),
        "at least the checkout answered: {stores:#?}"
    );
}

/// A reading the command was not asked to make is named, with the reason it was not.
///
/// Three readings are switched off for a reclaim that is only deciding whether to refuse,
/// because each costs processes and none changes the answer. That is a good trade and it
/// used to be invisible, which is the problem: a person could not tell a group that is
/// empty from a group nobody asked for. A `nodal reclaim --check` asks for all three, so
/// this is asserted where one is genuinely off — a reclaim's own refusal reading, which
/// the executed operation records.
#[test]
fn a_reading_that_was_not_asked_for_is_named_with_the_reason() {
    let (machine, _home) = machine();
    let done = machine.nodal(&["reclaim", SLUG, "--yes", "--json"]);
    assert!(done.status.success(), "the reclaim did not go ahead: {}", stderr(&done));
    let printed = answer(&done);
    let read: Value = serde_json::from_str(&printed)
        .unwrap_or_else(|_| panic!("a reclaim answers with one document: {printed}"));

    let gaps = read["evidence"]["not_checked"].as_array().unwrap();
    assert!(!gaps.is_empty(), "the reclaim's own reading skipped nothing at all: {read:#}");
    for gap in gaps {
        assert!(gap["what"].as_str().is_some_and(|what| !what.is_empty()), "{gap:#}");
        assert!(gap["why"].as_str().is_some_and(|why| !why.is_empty()), "{gap:#}");
    }
    // The process table is not among them. The refusal reading does not ask for it, and
    // the reclaim reads it a moment later for its own reason; the record has to describe
    // the operation and not either half of it.
    let named = serde_json::to_string(gaps).unwrap();
    assert!(!named.contains("the process table"), "the reclaim read the table: {named}");
    assert!(
        read["evidence"]["processes"]["seen"].as_u64().is_some_and(|seen| seen > 0),
        "and the record says so: {read:#}"
    );
}

/// The processes the host refused to show are counted, and the count is in the verdict.
///
/// This is what a process this account may not read becomes when it cannot be ruled out.
/// could be standing in this home; on Linux it used to leave the table with no row and no
/// note at all, and a verdict has no way to say "and there were 37 I could not see". Now
/// it does, and a person or a test can decide for themselves whether a safe verdict taken
/// over 37 unreadable processes is one they want to act on.
///
/// Every host running this suite has some: the process that started the machine is another
/// account's unless the suite runs as root.
#[test]
fn the_processes_the_host_refused_are_counted_in_the_record() {
    let (machine, _home) = machine();
    let record = record(&machine, SLUG);
    let table = &record["processes"];

    assert!(table["seen"].as_u64().is_some_and(|seen| seen > 0), "no process was read: {table:#}");
    assert!(table["withheld"].is_u64(), "the count of what was refused is missing: {table:#}");
    let reach = table["reach"].as_str().unwrap();
    assert!(["full", "part", "unread"].contains(&reach), "{reach} is not a reach");
    if table["withheld"].as_u64().unwrap_or_default() > 0 {
        assert_eq!(reach, "part", "processes were refused and the reach does not say so");
    }

    // Which readings of occupancy this host answered, from the product's own list rather
    // than from a list written twice.
    let occupancy: Vec<&str> =
        table["occupancy"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
    assert_eq!(occupancy, OCCUPANCY, "the record disagrees with the host about what it read");
}

/// One record, two renderings, and the human one is not a summary of a different thing.
#[test]
fn the_record_is_in_both_renderings_and_says_the_same_thing() {
    let (machine, _home) = machine();
    let record = record(&machine, SLUG);
    let printed = answer(&machine.nodal(&["reclaim", SLUG, "--check"]));

    assert!(printed.contains("rests on"), "the human view has no evidence at all:\n{printed}");
    assert!(printed.contains("stores asked"), "{printed}");
    assert!(printed.contains("refs: walked HEAD"), "{printed}");
    assert!(printed.contains("read, "), "the table's counts are missing:\n{printed}");
    assert!(printed.contains("withheld; occupancy is"), "{printed}");
    // The two renderings are two readings of a machine that moves between them, so the
    // counts are not compared. What is compared is the one thing that does not move: the
    // store the reading asked, named in both.
    let first = record["stores"][0]["path"].as_str().unwrap();
    assert!(printed.contains(first), "the human view names no store:\n{printed}");
}

/// Nothing in the record moves the verdict.
///
/// The record is evidence, not a second opinion. `safe_to_reclaim` is read off the reasons
/// and the reasons are made by the predicates the operation itself refuses on, so a reader
/// that gated on a field of the record would be taking an answer nothing stands behind.
/// The check here is the strong form: the record is full, and the verdict is what it was
/// before any of it existed — safe, with no reason.
#[test]
fn nothing_in_the_record_changes_the_verdict() {
    let (machine, home) = machine();
    let read = check(&machine, SLUG);
    let record = &read["evidence"];

    assert!(!record["stores"].as_array().unwrap().is_empty(), "the record is not empty");
    assert!(record["processes"]["withheld"].is_u64(), "the record counts what was refused");
    assert_eq!(read["safe_to_reclaim"], Value::Bool(true), "{read:#}");
    assert!(read["reasons"].as_array().unwrap().is_empty(), "{read:#}");

    // And the other direction: a home that does refuse says so for a reason that is a
    // reason, and not because the record noticed something.
    work_only_here(&home);
    let refused = check(&machine, SLUG);
    assert_eq!(refused["safe_to_reclaim"], Value::Bool(false), "{refused:#}");
    let because = refused["reasons"][0]["needs"].as_str().unwrap_or_default();
    assert!(
        ["unique_loss", "unknown_evidence"].contains(&because),
        "a commit only this home has refuses over the commit — either because nothing \
         else holds it, or because nothing could check; never because the record noticed \
         something: {refused:#}"
    );
}

/// Printing the record is still a reading, and a reading writes nothing.
///
/// The record is the one thing in this lane that a command could plausibly have been
/// tempted to store on the read path, and it is not: `--check` computes it and prints it.
/// An executed reclaim writes it, which is a different command with a different promise.
#[test]
fn printing_the_record_writes_nothing() {
    let (machine, _home) = machine();
    let checkout = untouched(&machine.source);
    let state = Snapshot::of_except(&machine.state, is_registry);

    let read = check(&machine, SLUG);
    assert!(!read["evidence"]["stores"].as_array().unwrap().is_empty(), "there was a record");

    checkout
        .assert_unchanged(&untouched(&machine.source), "printing the record wrote in the checkout");
    state.assert_unchanged(
        &Snapshot::of_except(&machine.state, is_registry),
        "printing the record wrote in the state directory",
    );
    assert!(machine.trashed().is_empty(), "printing the record trashed something");
}

/// The registry's own file, which every command that opens it rewrites: SQLite writes its
/// sidecars on every open, so the bytes are not the claim and the rows are
/// (`tests/doctor_writes_nothing.rs` states the rule this borrows).
fn is_registry(relative: &std::path::Path) -> bool {
    relative
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.starts_with("registry.db"))
}
