//! A checkout Nodal holds nothing about, and the two readings a verdict takes of one.
//!
//! The verdict is the answer a bare `nodal` gives in a repository it has never seen, so
//! every suite about it starts from the same shape: a plain repository with no recipe and
//! no registry row, one worktree planted inside it and one beside it. That shape was
//! written out once per suite, and this is it once.
//!
//! Two things a verdict reads are not in the repository, and both are made here rather
//! than by each suite, because both are read from files whose layout is the product's
//! business and not a test's:
//!
//! | reading | what this writes |
//! |---|---|
//! | what a worktree was made for | one Claude Code session record, under the directory Claude Code would put it in |
//! | how old a BEHIND reading is | the log line Git writes when it moves a ref, dated as far back as the test wants |

use std::path::{Path, PathBuf};

use crate::{Machine, git};

/// The worktree planted inside the checkout.
pub const NESTED: &str = "nested-work";

/// The worktree planted beside it. This is the shape a real machine had: named by the
/// repository, and not underneath it.
pub const BESIDE: &str = "project-beside";

/// A checkout with no recipe and no registry row, and the worktrees it names.
pub struct Plain {
    /// The checkout itself.
    pub checkout: PathBuf,
    /// The worktree under it, left holding a commit no remote has.
    pub nested: PathBuf,
    /// The worktree beside it.
    pub beside: PathBuf,
}

/// Plant a checkout Nodal holds nothing about: no recipe in it and no row for it.
///
/// It is built rather than borrowed from the fixture project, because the fixture ships
/// a `nodal.toml` and a directory with one of those is a project a person has already
/// declared. The whole claim the verdict makes is about the directory of somebody who
/// has declared nothing.
///
/// The nested worktree is left holding a commit no remote has, so the table has a row
/// that says a directory holds the only copy of something. A machine where every row is
/// safe would exercise none of the reading that matters.
///
/// # Panics
///
/// If the repository could not be made, which is a machine no property can be asserted
/// on.
#[must_use]
pub fn plain(machine: &Machine) -> Plain {
    let root = machine.source.parent().expect("the machine root");
    let checkout = root.join("plain-checkout");
    std::fs::create_dir_all(&checkout).expect("the checkout directory");
    git(&checkout, &["init", "--quiet", "--initial-branch", "main"]);
    git(&checkout, &["config", "--local", "user.email", "safety@nodal.invalid"]);
    git(&checkout, &["config", "--local", "user.name", "Nodal safety suite"]);
    let origin = root.join("origin.git");
    git(&checkout, &["remote", "add", "origin", printable(&origin)]);
    std::fs::write(checkout.join("README"), "a checkout nodal has never seen")
        .expect("the first file");
    git(&checkout, &["add", "--all"]);
    git(&checkout, &["commit", "--quiet", "--message", "the first commit"]);

    git(&checkout, &["worktree", "add", "--quiet", "-b", NESTED, NESTED]);
    let nested = checkout.join(NESTED);
    std::fs::write(nested.join("only-here.txt"), "work that exists nowhere else")
        .expect("the only copy of something");
    git(&nested, &["add", "--all"]);
    git(&nested, &["commit", "--quiet", "--message", "work that is only here"]);

    let beside = root.join(BESIDE);
    git(&checkout, &["worktree", "add", "--quiet", "--detach", printable(&beside)]);
    assert!(!checkout.join("nodal.toml").exists(), "this checkout declares a project");
    Plain { checkout, nested, beside }
}

/// Give the checkout the `origin/main` a clone has, and the `origin/HEAD` that makes it
/// the branch every row is measured against.
///
/// A repository this suite builds has neither, so BEHIND is measured against the local
/// `main`. An ordinary clone is the other shape, and it is the one the age of a reading
/// is about: a remote-tracking ref only a fetch can move.
///
/// # Panics
///
/// If `git` refused, which means the fixture is not the repository it says it is.
pub fn tracking_origin(checkout: &Path) {
    let head = git(checkout, &["rev-parse", "HEAD"]);
    git(checkout, &["update-ref", "refs/remotes/origin/main", &head]);
    git(checkout, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
}

/// Date the last time this checkout moved `reference` at `days` ago.
///
/// This writes the line Git writes when it moves a ref, because that line is what the
/// age of a BEHIND reading is read from. Only a fetch moves a remote-tracking ref, and
/// Nodal makes no network call of its own, so a test cannot make a stale checkout by
/// waiting or by asking: it states the date the checkout already carries.
///
/// # Panics
///
/// If the ref has no log, which means the fixture never moved the ref it is dating.
pub fn last_moved_days_ago(checkout: &Path, reference: &str, days: i64) {
    let path = checkout.join(".git/logs").join(reference);
    let log = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} has no log to date", path.display()));
    let last = log.lines().next_back().expect("a log with a line in it");
    let (head, reason) = last.split_once('\t').unwrap_or((last, "fetch origin"));
    let mut fields: Vec<&str> = head.split_whitespace().collect();
    let seconds = (now() - days * 86_400).to_string();
    let zone = fields.len() - 1;
    fields[zone - 1] = &seconds;
    std::fs::write(&path, format!("{}\t{reason}\n", fields.join(" "))).expect("the dated log");
}

/// Record what a worktree was made for, as Claude Code records it.
///
/// One session file, in the directory Claude Code names after the working directory the
/// session ran in, holding the one record Nodal reads out of such a file: the first
/// prompt of the session. `machine.config_dir()` is where this machine's commands look
/// for those, so nothing here goes near the records of whoever is running the tests.
///
/// # Panics
///
/// If the record could not be written.
pub fn record_intent(machine: &Machine, worktree: &Path, prompt: &str) {
    // The name Claude Code gives the directory comes from the path the session ran in,
    // which is the resolved one. On a host whose temporary directory is reached through
    // a link, the name a test made the worktree with is not that path.
    let worktree = &std::fs::canonicalize(worktree).unwrap_or_else(|_| worktree.to_path_buf());
    let directory = machine.config_dir().join("projects").join(encode(worktree));
    std::fs::create_dir_all(&directory).expect("the session directory");
    let record = serde_json::json!({
        "type": "user",
        "cwd": worktree,
        "timestamp": "2026-01-01T00:00:00.000Z",
        "message": {"role": "user", "content": prompt},
    });
    std::fs::write(directory.join("session.jsonl"), format!("{record}\n"))
        .expect("the session record");
}

/// The name Claude Code gives the session directory of a working directory.
///
/// Written out rather than called, on purpose. The product has this rule as well
/// (`nodal_core::doctor::intent::encode`), and a test that shared the rule with the code
/// under test would agree with it however wrong it was.
fn encode(directory: &Path) -> String {
    directory
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

/// A path as an argument, for a fixture where a path that is not printable is a broken
/// fixture rather than a property.
fn printable(path: &Path) -> &str {
    path.to_str().expect("a printable path")
}

/// The clock, in whole seconds since the Unix epoch.
fn now() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970");
    i64::try_from(since.as_secs()).expect("a clock before the year 292 billion")
}
