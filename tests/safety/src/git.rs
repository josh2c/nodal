//! One `git` call in a directory, for a test that has to set a repository up or read
//! one back.
//!
//! Sixteen test files spawned `git` themselves, each with a runner of its own. The
//! runners agreed on what they did and disagreed on how they said it, so a fix to one
//! of them reached one suite. These are that runner, once.
//!
//! Every call here shuts the global and the system configuration out and refuses a
//! terminal prompt. A test must hold whatever the person running it keeps in their own
//! configuration, and a `git` that stops to ask for a password hangs a CI job rather
//! than failing it.
//!
//! One thing follows from that: a repository has no identity until a test gives it one.
//! [`init`] and [`commit`] write one, and [`identity`] is how a test that commits some
//! other way asks for one.

use std::path::Path;
use std::process::{Command, Output, Stdio};

/// The identity a test repository commits under.
///
/// A test never takes the identity of whoever is running it, and the address is under
/// `.invalid`, which is reserved and can reach nobody.
pub const IDENTITY: [(&str, &str); 2] =
    [("user.email", "unit@example.invalid"), ("user.name", "Test")];

/// One `git` command in a directory, as trimmed text, with the call insisted upon.
///
/// # Panics
///
/// If `git` is not there or refused the command.
pub fn git(directory: impl AsRef<Path>, args: &[&str]) -> String {
    git_text(directory, args).trim_end().to_owned()
}

/// The same, as exactly what the command printed.
///
/// A caller that reads one value asks [`git`], which takes the line ending off it. A
/// caller that compares the whole of what a command printed — the content of a file
/// `git show` wrote out, say — asks here, because the last newline is part of that.
///
/// # Panics
///
/// As [`git`].
pub fn git_text(directory: impl AsRef<Path>, args: &[&str]) -> String {
    let output = try_git(directory.as_ref(), args);
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        directory.as_ref().display(),
        crate::text::stderr(&output)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The same, for a caller that needs only the command to have worked.
///
/// Neither stream is kept, so a call that writes progress lines leaves the test output
/// readable.
///
/// # Panics
///
/// As [`git`].
pub fn git_ok(directory: impl AsRef<Path>, args: &[&str]) {
    let status = command(directory.as_ref(), args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} in {}", directory.as_ref().display());
}

/// The same, for a command that is asked because it may fail.
///
/// # Panics
///
/// If `git` is not there at all.
#[must_use]
pub fn try_git(directory: impl AsRef<Path>, args: &[&str]) -> Output {
    command(directory.as_ref(), args).output().expect("git runs")
}

/// A repository with one branch and this suite's identity, and nothing committed yet.
///
/// # Panics
///
/// As [`git`].
pub fn init(directory: impl AsRef<Path>, branch: &str) {
    let directory = directory.as_ref();
    git_ok(directory, &["init", "-q", "-b", branch]);
    identity(directory);
}

/// Give a repository the identity a test commits under.
///
/// # Panics
///
/// As [`git`].
pub fn identity(directory: impl AsRef<Path>) {
    let directory = directory.as_ref();
    for (key, value) in IDENTITY {
        git_ok(directory, &["config", "--local", key, value]);
    }
}

/// Commit everything a directory holds, as a person working in it would.
///
/// # Panics
///
/// As [`git`].
pub fn commit(directory: impl AsRef<Path>, message: &str) {
    let directory = directory.as_ref();
    identity(directory);
    git_ok(directory, &["add", "-A"]);
    git_ok(directory, &["commit", "-qm", message]);
}

/// The invocation itself, not yet run.
fn command(directory: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}
