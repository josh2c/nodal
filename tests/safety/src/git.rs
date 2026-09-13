//! One `git` call in a directory, for a test that has to set a repository up or read
//! one back.
//!
//! Sixteen test files spawned `git` themselves, each with a runner of its own. The
//! runners agreed on what they did and disagreed on how they said it, so a fix to one
//! of them reached one suite. These are that runner, once.
//!
//! [`untouched`] is the other direction over the same seam: the three readings of a
//! repository that together say it was only read.
//!
//! Every call here shuts the global and the system configuration out and refuses a
//! terminal prompt. A test must hold whatever the person running it keeps in their own
//! configuration, and a `git` that stops to ask for a password hangs a CI job rather
//! than failing it.
//!
//! Every call also turns automatic maintenance off. Git decides after a commit that the
//! repository could be tidied, and it does that in a **background process that outlives
//! the command**: it writes `.git/objects/maintenance.lock`, works, and removes it. A
//! test that reads or copies the repository the moment the commit returns therefore
//! races a process it never started, and lists a file that is gone before it can be
//! opened. No test in this workspace wants that work done, so no repository here asks
//! for it.
//!
//! One thing follows from that: a repository has no identity until a test gives it one.
//! [`init`] and [`commit`] write one, and [`identity`] is how a test that commits some
//! other way asks for one.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The identity a test repository commits under.
///
/// A test never takes the identity of whoever is running it, and the address is under
/// `.invalid`, which is reserved and can reach nobody.
pub const IDENTITY: [(&str, &str); 2] =
    [("user.email", "unit@example.invalid"), ("user.name", "Test")];

/// What every call sets, so that a test repository does no work a test did not ask for.
///
/// Both keys turn off the background pass Git starts on its own after a commit. The
/// module note says what that pass does and what it races.
const QUIET: [&str; 2] = ["gc.auto=0", "maintenance.auto=false"];

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
    command.arg("-C").arg(directory);
    for setting in QUIET {
        command.arg("-c").arg(setting);
    }
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}

/// Everything about a checkout that an operation which only *reads* it must leave alone.
///
/// Three readings, because "the checkout is unchanged" is three claims and a status line
/// states none of them. The tree is held byte for byte and mode for mode
/// ([`crate::tree::Snapshot`]), because a status line says nothing about a file no commit
/// tracks. The index is held as its own bytes, because what is staged is work and a
/// command that refreshed the stat cache has written to the file a person's `git` is
/// using. Every ref is held with the commit it stands at, because a ref that moved is a
/// ref that moved whatever the working tree looks like.
///
/// `nodal new --carry` is what this was written for: it reads a checkout and reproduces
/// what it holds somewhere else, and the whole of its promise is that the checkout comes
/// out of it as it went in.
pub struct Untouched {
    /// Everything outside `.git`, with the permission bits and the link targets.
    tree: crate::tree::Snapshot,
    /// `.git/index`, byte for byte. Empty where the repository has none yet.
    index: Vec<u8>,
    /// Every ref and the object it names.
    refs: String,
    /// The repository, for a message.
    root: PathBuf,
}

/// Take the three readings of `repo`.
///
/// # Panics
///
/// If the repository could not be read, or `git` refused `for-each-ref`.
#[must_use]
pub fn untouched(repo: impl AsRef<Path>) -> Untouched {
    let repo = repo.as_ref();
    Untouched {
        tree: crate::tree::Snapshot::of_except(repo, |path| {
            path.file_name().is_some_and(|name| name == ".git")
        }),
        index: std::fs::read(repo.join(".git").join("index")).unwrap_or_default(),
        refs: git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]),
        root: repo.to_path_buf(),
    }
}

impl Untouched {
    /// Insist that all three readings are what they were, and say which one is not.
    ///
    /// # Panics
    ///
    /// Naming the reading that differs, and for the tree every path it disagrees about.
    pub fn assert_unchanged(&self, later: &Self, claim: &str) {
        self.tree.assert_unchanged(&later.tree, claim);
        assert_eq!(
            self.index,
            later.index,
            "{claim}: the index of {} was written to",
            self.root.display()
        );
        assert_eq!(self.refs, later.refs, "{claim}: a ref of {} moved", self.root.display());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use tempfile::TempDir;

    use super::{QUIET, git, init};

    /// The settings that keep Git from starting work of its own reach the command. A
    /// repository that runs a background pass writes and removes a lock file under
    /// `.git/objects`, and every test that reads or copies a repository races it.
    #[test]
    fn a_test_repository_runs_no_pass_of_its_own() {
        let directory = TempDir::new().unwrap();
        init(directory.path(), "main");

        assert_eq!(git(directory.path(), &["config", "--get", "gc.auto"]), "0");
        assert_eq!(git(directory.path(), &["config", "--get", "maintenance.auto"]), "false");
        assert_eq!(QUIET.len(), 2, "a setting was added without a reading for it");
    }
}
