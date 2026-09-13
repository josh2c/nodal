//! One writer per home, and an advisory lock that stops nothing else.
//!
//! Two engineers on one box are the case this exists for. Nodal's other answer to "who
//! is in this unit" is the process table, and a process scan reads `/proc`, which does
//! not cross Linux accounts: each of them sees their own processes and none of the
//! other's. So two people can drive one home through Nodal's own operations at the same
//! time and neither is ever told. The lock row is the record that crosses the accounts
//! the scan cannot.
//!
//! Five properties, and each one is a way the arrangement could go wrong:
//!
//! | property | what a break would look like |
//! |---|---|
//! | a second actor is told | two actors write one home and neither hears of the other |
//! | the refusal names a way out | a person is stopped with no way to carry on |
//! | the lock is advisory | Nodal stops an editor, a `git` call or a process it did not start |
//! | `--take` is deliberate and recorded | a hold moves with nothing written down |
//! | an idle hold lapses | a home nobody has entered for a day stays locked |
//!
//! ## The second actor, without a second account
//!
//! Switching uid needs privileges CI has not got, so the second person here is a second
//! value of `NODAL_ACTOR`. That is the seam the property is about: who a command is
//! decided by is the actor the environment names (`runtime::actor`), and a lock row
//! records exactly that. What it cannot show is the kernel refusing one account the
//! other's file, and [`UID_CLAIM`] says so out loud. `tests/shared_host.rs` records
//! the same limit.

#![allow(clippy::unwrap_used, reason = "a test fails by panicking")]

use std::path::Path;

use nodal_core::model::EventKind;
use nodal_core::store::locks;
use nodal_safety::machine::binary;
use nodal_safety::project::Workspace;
use nodal_safety::state::InState;
use nodal_safety::{git_ok, stderr, stdout};

/// The claim this suite does not make, because making it needs privileges CI has not
/// got: that two Linux accounts are two actors to the kernel. What is asserted instead
/// is the seam Nodal decides an actor from, which is the environment.
const UID_CLAIM: &str = "two accounts are two actors to the kernel; asserted here as the two \
                         values of NODAL_ACTOR that Nodal reads an actor from";

/// The name the workspace fixture's own commands run under.
///
/// Every command a `Workspace` makes is the first actor unless a test says otherwise,
/// so the unit those commands create is held by this name.
const FIRST: &str = "ada";

/// The second person on the box.
const SECOND: &str = "bo";

/// A workspace whose every command runs as `FIRST`.
fn workspace() -> Workspace {
    Workspace::new(binary()).with_env("NODAL_ACTOR", FIRST)
}

/// What `nodal <args>` says when the second actor runs it in `cwd`.
fn as_second(workspace: &Workspace, cwd: &Path, args: &[&str]) -> std::process::Output {
    workspace.command_in(cwd, args).env("NODAL_ACTOR", SECOND).output().unwrap()
}

/// A second actor entering a home the first holds is refused Nodal's write verbs, and
/// told who holds it and how to take it.
///
/// `cd` and `run` are the two a person meets first. Both are refused, both name the
/// holder, and neither leaves the second actor guessing what to do next: a refusal
/// without a way out is a refusal a person cannot act on.
#[test]
fn a_second_actor_is_refused_the_write_verbs_and_told_who_holds_the_unit() {
    println!("NOT ASSERTED: {UID_CLAIM}");
    let workspace = workspace();
    let home = workspace.unit("worker-import");

    for args in [&["cd", "worker-import"][..], &["run", "--", "true"][..]] {
        let refused = as_second(&workspace, &home, args);
        assert!(!refused.status.success(), "nodal {args:?} let a second actor in");
        let said = stderr(&refused);
        assert!(said.contains(FIRST), "the refusal does not name the holder: {said}");
        assert!(said.contains("--take"), "the refusal does not say how to take it: {said}");
        assert!(
            said.contains("advisory"),
            "the refusal does not say what it does not stop: {said}"
        );
    }
}

/// The first actor is never refused their own unit, and entering it again refreshes it.
///
/// The property that would break silently is the opposite of the one above: a lock that
/// refused the person who took it would make the whole feature unusable, and a lock that
/// did not refresh would release a home somebody is working in.
#[test]
fn the_holder_enters_their_own_unit_and_the_hold_is_refreshed() {
    let workspace = workspace();
    let home = workspace.unit("worker-import");
    let store = workspace.store();
    let unit = workspace.one_unit().id;
    let before = locks::get(store.conn(), unit).unwrap().unwrap();

    let again = workspace.nodal_in(&home, &["run", "--", "true"]);
    assert!(again.status.success(), "the holder was refused their own unit: {}", stderr(&again));

    let after = locks::get(store.conn(), unit).unwrap().unwrap();
    assert_eq!(after.actor, before.actor, "the holder changed on their own entry");
    assert_eq!(after.taken_at, before.taken_at, "a refresh moved the instant the hold began");
    assert!(after.refreshed_at >= before.refreshed_at, "an entry did not refresh the hold");
}

/// The lock is advisory: it refuses Nodal's write verbs and stops nothing else.
///
/// This is the property that makes the refusal above acceptable. A second actor who is
/// told the unit is held must still be able to open the file, run `git`, and start a
/// process in the directory — because none of those is Nodal, and a tool that took the
/// editor away from somebody would be doing damage rather than preventing it.
///
/// The read verbs are here for the same reason. A second actor who cannot list the
/// project or read the home's environment has been stopped, whatever the lock is called.
#[test]
fn the_lock_stops_no_editor_no_git_and_no_read() {
    let workspace = workspace();
    let home = workspace.unit("worker-import");

    // An editor: a plain write in the home, which nothing in Nodal is between.
    let scratch = home.join("app").join("note.txt");
    std::fs::write(&scratch, "the second actor typed this\n").unwrap();
    assert_eq!(std::fs::read_to_string(&scratch).unwrap(), "the second actor typed this\n");

    // The person's own Git, in the home the first actor holds.
    git_ok(&home, &["status", "--short"]);
    git_ok(&home, &["rev-parse", "HEAD"]);

    // Nodal's own read verbs, run as the second actor.
    for args in [&["ls"][..], &["show", "worker-import"][..], &["env", "--export"][..]] {
        let read = as_second(&workspace, &home, args);
        assert!(read.status.success(), "nodal {args:?} refused a read: {}", stderr(&read));
    }

    // The export the prompt hook runs still carries the home's variables, so a second
    // actor's shell is activated exactly as the holder's is.
    let exported = as_second(&workspace, &home, &["env", "--export"]);
    assert!(
        stdout(&exported).contains("NODAL_ROOT"),
        "the export lost the home: {}",
        stdout(&exported)
    );
}

/// `--take` moves the hold, and writes the hand-off where a person reads it.
///
/// Two halves, and the second is what makes the first safe. A hold that moved with
/// nothing written down would leave the person it was taken from with no way to find out
/// what happened, which is the same as losing it.
#[test]
fn take_moves_the_hold_and_records_the_hand_off() {
    let workspace = workspace();
    let home = workspace.unit("worker-import");
    let unit = workspace.one_unit().id;

    let taken = as_second(&workspace, &home, &["cd", "--take", "worker-import"]);
    assert!(taken.status.success(), "--take was refused: {}", stderr(&taken));

    let store = workspace.store();
    let held = locks::get(store.conn(), unit).unwrap().unwrap();
    assert_eq!(held.actor.unwrap().name.as_str(), SECOND, "--take did not move the hold");

    let handed: Vec<_> =
        workspace.events().into_iter().filter(|event| event.kind == EventKind::Handoff).collect();
    assert_eq!(handed.len(), 1, "a hand-off wrote {} events", handed.len());
    let body = &handed[0].body;
    assert!(body.contains(FIRST) && body.contains(SECOND), "the hand-off names nobody: {body}");

    // And the first actor is now the one who is told.
    let refused = workspace.nodal_in(&home, &["run", "--", "true"]);
    assert!(!refused.status.success(), "the hold did not move: the old holder still writes");
    assert!(stderr(&refused).contains(SECOND), "{}", stderr(&refused));
}

/// A hold nobody has refreshed for longer than the idle window is anybody's.
///
/// The window is a recipe key, so the test sets it to zero and asserts that the next
/// actor walks in. Zero is the honest way to test an idle window without sleeping: the
/// question is whether the clock is consulted at all, and a test that waited eight hours
/// would answer the same question a day later.
#[test]
fn an_idle_hold_lapses_and_the_next_actor_takes_it() {
    let workspace =
        Workspace::with_recipe(binary(), "[lock]\nidle_hours = 0\n").with_env("NODAL_ACTOR", FIRST);
    let home = workspace.unit("worker-import");
    let unit = workspace.one_unit().id;

    let entered = as_second(&workspace, &home, &["run", "--", "true"]);
    assert!(entered.status.success(), "an idle hold refused the next actor: {}", stderr(&entered));

    let store = workspace.store();
    let held = locks::get(store.conn(), unit).unwrap().unwrap();
    assert_eq!(held.actor.unwrap().name.as_str(), SECOND, "the lapsed hold did not move");

    // A hold that lapsed moves without a hand-off. Nothing was taken from anybody.
    assert!(
        !workspace.events().iter().any(|event| event.kind == EventKind::Handoff),
        "a lapsed hold was recorded as a hand-off",
    );
}

/// A reclaim gives up the unit's hold, so no row claims the writer of a home that is
/// gone.
#[test]
fn a_reclaim_releases_the_hold_it_held() {
    let workspace = workspace();
    let home = workspace.unit("worker-import");
    let unit = workspace.one_unit().id;
    assert!(locks::get(workspace.store().conn(), unit).unwrap().is_some());

    let gone = workspace.nodal_in(&home, &["reclaim", "worker-import", "--yes"]);
    assert!(gone.status.success(), "the reclaim failed: {}", stderr(&gone));
    assert_eq!(
        locks::get(workspace.store().conn(), unit).unwrap(),
        None,
        "a reclaimed unit still names a writer",
    );
}

/// A lock row that records no actor holds nobody, and refuses nobody.
///
/// This is the state migration 11 describes rather than invents. No product code ever
/// wrote such a row, so the registry is put into that state by hand here: the actor
/// columns are cleared on a row a create wrote. A second actor walking into that home
/// must be let in, and the row must come back naming them — because a refusal whose
/// reason is that Nodal does not know who holds it is a refusal a person cannot act on.
#[test]
fn a_lock_row_that_records_no_actor_refuses_nobody() {
    let workspace = workspace();
    let home = workspace.unit("worker-import");
    let unit = workspace.one_unit().id;
    {
        let store = workspace.store();
        store
            .conn()
            .execute(
                "UPDATE lock SET actor_kind = NULL, actor_name = NULL WHERE unit_id = ?",
                [unit.to_string()],
            )
            .unwrap();
        assert_eq!(locks::get(store.conn(), unit).unwrap().unwrap().actor, None);
    }

    let entered = as_second(&workspace, &home, &["run", "--", "true"]);
    assert!(entered.status.success(), "an actor-less row refused an actor: {}", stderr(&entered));

    let held = locks::get(workspace.store().conn(), unit).unwrap().unwrap();
    assert_eq!(held.actor.unwrap().name.as_str(), SECOND, "the row was not rewritten");
}

/// The list puts the writer before the process table, because only one of them crosses
/// accounts.
#[test]
fn the_list_names_the_writer_before_what_the_process_table_saw() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));

    let listed = workspace.nodal(&["ls"]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    let said = stdout(&listed);
    assert!(said.contains(&format!("{FIRST} holds")), "the list names no writer: {said}");

    let json = workspace.nodal(&["ls", "--json"]);
    let document: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    let row = &document["units"][0];
    assert_eq!(row["holder"]["actor"], FIRST, "--json carries no holder: {row}");
    assert!(row["holder"]["expires_at"].is_string(), "the holder carries no expiry: {row}");
    assert!(row["sessions"].is_array(), "--json lost the process attribution: {row}");
}
