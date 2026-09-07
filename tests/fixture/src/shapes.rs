//! Git histories of the shapes a project's branches are really in.
//!
//! One origin repository holds one branch per shape, and every shape is made the way a
//! team makes it: a branch that never left the base, a branch with work on top of it, a
//! branch the base moved under, a branch merged with a merge commit, a branch squashed
//! into one commit on the base, and a branch that changed a file the base changed too.
//!
//! The squash shape carries two commits, because a squash merge of one commit is the
//! same object as a cherry-pick and would pass a test that reads commits rather than
//! trees. Only the tree comparison finds this one.
//!
//! The working-tree shapes are made in a clone, not here, because a repository has one
//! working tree and a unit has one home each: [`home`] makes the clone, and [`dirty`],
//! [`detach`] and [`nest`] put it in the state the shape is named for.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built fails the test it was built for"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The branch every other branch of the fixture merges into.
pub const BASE: &str = "main";

/// The file the base and one branch both change, which is what makes them conflict.
const SHARED: &str = "shared.txt";

/// One branch of the origin repository, and what a list must say about it.
///
/// The verdicts are stated here rather than in a test so that the fixture and the
/// expectation move together: a shape whose history changes has to change this line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Branch {
    /// The branch name in the origin repository.
    pub name: &'static str,
    /// What `nodal ls` must report in its integration column.
    pub verdict: &'static str,
    /// Whether the base has moved under the branch.
    pub behind: bool,
}

/// Every branch the origin repository carries, in the order [`origin`] makes them.
pub const BRANCHES: &[Branch] = &[
    Branch { name: "follows-base", verdict: "done (ancestor)", behind: true },
    Branch { name: "behind", verdict: "open", behind: true },
    Branch { name: "merged", verdict: "done (ancestor)", behind: true },
    Branch { name: "squashed", verdict: "done (absorbed)", behind: true },
    Branch { name: "conflicting", verdict: "conflict", behind: true },
    Branch { name: "ahead", verdict: "open", behind: false },
];

/// Build the origin repository at `root` and return the path.
///
/// # Panics
///
/// If `git` is not on the path, or refuses a command, which means no test that needs a
/// repository can run at all.
#[must_use]
pub fn origin(root: impl AsRef<Path>) -> PathBuf {
    let root = root.as_ref().to_path_buf();
    std::fs::create_dir_all(&root).expect("the origin directory is created");
    git(&root, &["init", "--quiet", "--initial-branch", BASE]);
    for (key, value) in SETTINGS {
        git(&root, &["config", "--local", key, value]);
    }
    write(&root, SHARED, "one\n");
    write(&root, "base.txt", "base\n");
    commit(&root, "the first commit");
    branches(&root);
    root
}

/// The settings that make a history the same on every machine.
const SETTINGS: [(&str, &str); 4] = [
    ("user.email", "fixture@nodal.invalid"),
    ("user.name", "Nodal fixture"),
    ("commit.gpgsign", "false"),
    ("gc.auto", "0"),
];

/// Make every branch, then move the base under the ones that must be behind it.
fn branches(root: &Path) {
    for name in ["follows-base", "behind", "merged", "squashed", "conflicting"] {
        git(root, &["switch", "--quiet", "--create", name, BASE]);
    }
    add_commit(root, "behind", "behind.txt", "work under a moving base\n");
    add_commit(root, "merged", "merged.txt", "work that lands as a merge\n");
    add_commit(root, "squashed", "squashed-one.txt", "the first half\n");
    add_commit(root, "squashed", "squashed-two.txt", "the second half\n");
    add_commit(root, "conflicting", SHARED, "theirs\n");

    git(root, &["switch", "--quiet", BASE]);
    write(root, "moved.txt", "the base moved on\n");
    commit(root, "the base moves on");
    git(root, &["merge", "--quiet", "--no-ff", "--message", "merge merged", "merged"]);
    git(root, &["merge", "--quiet", "--squash", "squashed"]);
    commit(root, "squashed: both halves at once");
    write(root, SHARED, "ours\n");
    commit(root, "the base changes the shared file");

    git(root, &["switch", "--quiet", "--create", "ahead", BASE]);
    write(root, "ahead.txt", "work on top of the base\n");
    commit(root, "work ahead of the base");
    git(root, &["switch", "--quiet", BASE]);
}

/// Commit one file on one branch.
fn add_commit(root: &Path, branch: &str, path: &str, contents: &str) {
    git(root, &["switch", "--quiet", branch]);
    write(root, path, contents);
    commit(root, &format!("{branch}: {path}"));
}

/// Clone `origin` into `home` and check `branch` out there.
///
/// A unit's home is a clone and not a worktree, so this is what the list reads.
///
/// # Panics
///
/// As [`origin`].
#[must_use]
pub fn home(origin: impl AsRef<Path>, home: impl AsRef<Path>, branch: &str) -> PathBuf {
    let (origin, home) = (origin.as_ref(), home.as_ref().to_path_buf());
    let parent = home.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).expect("the parent of the home is created");
    git(parent, &["clone", "--quiet", "--", text(origin), text(&home)]);
    for (key, value) in SETTINGS {
        git(&home, &["config", "--local", key, value]);
    }
    git(&home, &["switch", "--quiet", branch]);
    home
}

/// Create `name` at the home's HEAD and check it out.
///
/// Two open units cannot hold one branch, so a fixture that puts many units on one shape
/// gives each of them a branch of its own at the same commit.
///
/// # Panics
///
/// As [`origin`].
pub fn take_branch(home: impl AsRef<Path>, name: &str) {
    git(home.as_ref(), &["switch", "--quiet", "--create", name]);
}

/// Leave one changed path, one staged path and one untracked path in a home.
///
/// # Panics
///
/// As [`origin`].
pub fn dirty(home: impl AsRef<Path>) {
    let home = home.as_ref();
    write(home, "base.txt", "changed and not staged\n");
    write(home, "staged.txt", "staged and not committed\n");
    git(home, &["add", "--", "staged.txt"]);
    write(home, "untracked.txt", "not tracked at all\n");
}

/// Put a home on a commit rather than on a branch.
///
/// # Panics
///
/// As [`origin`].
pub fn detach(home: impl AsRef<Path>) {
    git(home.as_ref(), &["checkout", "--quiet", "--detach", "HEAD"]);
}

/// Register a linked worktree inside a home, the way an agent's own tooling does.
///
/// # Panics
///
/// As [`origin`].
#[must_use]
pub fn nest(home: impl AsRef<Path>, name: &str) -> PathBuf {
    let home = home.as_ref();
    let nested = home.join(name);
    git(home, &["worktree", "add", "--quiet", "--detach", text(&nested)]);
    nested
}

/// Write a file, creating parent directories.
fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent directory is created");
    }
    std::fs::write(path, contents).expect("the file is written");
}

/// Stage everything and commit.
fn commit(root: &Path, message: &str) {
    git(root, &["add", "--all"]);
    git(root, &["commit", "--quiet", "--message", message]);
}

/// A path as an argument.
fn text(path: &Path) -> &str {
    path.to_str().expect("a fixture path is UTF-8")
}

/// Run one `git` command and fail loudly when it does not work.
fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {arguments:?} in {}: {}",
        root.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim_end().to_owned()
}
