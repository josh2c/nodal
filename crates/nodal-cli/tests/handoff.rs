//! Acceptance for `nodal handoff`: the note one session leaves for the next.
//!
//! Two claims, both read back out of the registry rather than out of what the command
//! printed. A handoff is recorded as stated, by the actor that stated it, and it reaches
//! the unit's memory, which is the only reason it is worth recording. A handoff that
//! says nothing is refused, because an empty note in a log is worse than no note: the
//! next session reads it as the last thing anybody said.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use nodal_safety::Workspace;
use nodal_safety::text::{stderr, stdout};

fn workspace() -> Workspace {
    Workspace::new(state::BINARY)
}

#[test]
fn a_stated_handoff_is_on_the_log_and_in_the_memory() {
    let workspace = workspace();
    let home = workspace.unit("worker-import");

    let said = stdout(&workspace.nodal(&[
        "handoff",
        "--unit",
        "worker-import",
        "the legacy parser still fails on two-digit years",
    ]));
    assert!(said.contains("two-digit years"), "the command does not say what it wrote: {said}");

    let shown = stdout(&workspace.nodal(&["show", "worker-import", "--json"]));
    let detail: serde_json::Value = serde_json::from_str(&shown).unwrap();
    let handoff = detail["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "handoff")
        .expect("the log carries the handoff");
    assert_eq!(handoff["epistemic"], "stated");
    assert!(handoff["body"].as_str().unwrap().contains("two-digit years"));

    let memory = std::fs::read_to_string(home.join("WORKUNIT.md")).unwrap();
    assert!(memory.contains("two-digit years"), "the memory does not carry it: {memory}");
}

#[test]
fn a_handoff_that_says_nothing_is_refused() {
    let workspace = workspace();
    drop(workspace.unit("worker-import"));

    let refused = workspace.nodal(&["handoff", "--unit", "worker-import", "   "]);

    assert!(!refused.status.success(), "an empty handoff was recorded");
    assert!(stderr(&refused).contains("says nothing"), "{}", stderr(&refused));
    let shown = stdout(&workspace.nodal(&["show", "worker-import", "--json"]));
    let detail: serde_json::Value = serde_json::from_str(&shown).unwrap();
    assert!(
        !detail["history"].as_array().unwrap().iter().any(|event| event["kind"] == "handoff"),
        "an empty handoff reached the log"
    );
}
