//! What a command said, as text a test can assert on.
//!
//! Every suite in the workspace read the two streams of a finished command the same
//! way, and every one of them wrote the reading out again. The four readings here are
//! that code, once. They differ only in what they insist upon: [`stdout`] insists the
//! command succeeded, [`answer`] does not, because a refusal Nodal reports on standard
//! output with a non-zero code is an answer and not a fault.

use std::process::Output;

/// Standard output as text, with the command insisted upon.
///
/// # Panics
///
/// If the command failed, printing what it said about that.
#[must_use]
pub fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{}", stderr(output));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Standard output as text, whatever the exit code.
///
/// This is what a command that reports a refusal rather than raising it is read with.
#[must_use]
pub fn answer(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Standard error as text.
#[must_use]
pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The document a `--json` command answered with, with the command insisted upon.
///
/// # Panics
///
/// If the command failed, or did not answer with one JSON document.
#[must_use]
pub fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("--json is one document")
}
