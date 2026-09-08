//! Acceptance for the branch section of `nodal doctor`: the work no directory holds.
//!
//! A measured machine held 317 local branches. 306 of them had no worktree anywhere, so
//! no source anchored to a directory could report one of them, and 22 of those 306 held
//! fifty commits that exist on no remote. The report said the worktrees were clean,
//! which was true, and it was the only thing that machine's unbacked-up work was not in.
//!
//! The machine here is planted with one branch of each shape the audit has to tell
//! apart:
//!
//! | branch | shape |
//! |---|---|
//! | `main` | the checked-out branch; the worktree section already reports it |
//! | `shipped` | merged into the default branch |
//! | `review/api` | not merged, every commit on a remote |
//! | `importer/retry` | three commits that exist on no remote |
//! | `orphan` | one commit on no remote, and an upstream somebody deleted |
//!
//! Three claims are made against it: the buckets are right, the default rendering is
//! loud about one bucket and quiet about the other two, and `--all` opens them without
//! changing what was found. A fourth test builds three hundred refs and times the audit,
//! because one `rev-list` per ref is the cost this design accepts and a number is the
//! only way to hold it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime};

use nodal_core::doctor::branches;
use nodal_core::model::Timestamp;
use nodal_core::output::Render;
use nodal_core::output::view::doctor::{BranchRow, Branches, Standing};

/// A checkout, its remote, and the branches of both.
struct Planted {
    /// Holds the whole machine; dropping it removes it.
    directory: tempfile::TempDir,
    /// The checkout the audit is run in.
    checkout: PathBuf,
}

impl Planted {
    /// The audit of this machine.
    fn audit(&self) -> Branches {
        branches::find(&self.checkout, Timestamp::now()).expect("a checkout doctor can read")
    }
}

/// One branch of each shape the audit tells apart.
fn plant() -> Planted {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    let checkout = root.join("code/app");
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&checkout).unwrap();
    git(root, &["init", "--quiet", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
    git(&checkout, &["init", "--quiet", "--initial-branch=main", "."]);
    git(&checkout, &["remote", "add", "origin", remote.to_str().unwrap()]);
    write(&checkout.join("README.md"), "# app\n");
    commit(&checkout, "the project");
    git(&checkout, &["push", "--quiet", "origin", "main"]);

    // Merged into the default branch, and the default branch is on the remote.
    branch(&checkout, "shipped", 1);
    git(&checkout, &["switch", "--quiet", "main"]);
    git(&checkout, &["merge", "--quiet", "--no-ff", "--no-edit", "shipped"]);
    git(&checkout, &["push", "--quiet", "origin", "main"]);

    // Not merged, and every commit of it is on the remote.
    branch(&checkout, "review/api", 1);
    git(&checkout, &["push", "--quiet", "origin", "review/api"]);

    // Three commits that exist on no remote: the shape the audit is loud about.
    branch(&checkout, "importer/retry", 3);

    // One commit on no remote, and an upstream somebody deleted. `fetch --prune` is
    // what removes the remote-tracking ref, and after it nothing on this machine says
    // where that work is.
    branch(&checkout, "orphan", 1);
    git(&checkout, &["push", "--quiet", "--set-upstream", "origin", "orphan"]);
    git(&checkout, &["push", "--quiet", "origin", "--delete", "orphan"]);
    git(&checkout, &["fetch", "--quiet", "--prune", "origin"]);

    git(&checkout, &["switch", "--quiet", "main"]);
    Planted { directory, checkout }
}

/// A branch with `commits` commits of its own, left checked out.
fn branch(checkout: &Path, name: &str, commits: usize) {
    git(checkout, &["switch", "--quiet", "--create", name, "main"]);
    for index in 0..commits {
        write(&checkout.join(format!("{}-{index}.md", name.replace('/', "-"))), "work\n");
        commit(checkout, &format!("{name} {index}"));
    }
}

/// The one row of a name.
fn one<'a>(audit: &'a Branches, name: &str) -> &'a BranchRow {
    audit.rows.iter().find(|row| row.name == name).unwrap_or_else(|| {
        panic!("no row for {name}: {:#?}", audit.rows);
    })
}

#[test]
fn every_branch_is_in_the_bucket_its_commits_put_it_in() {
    let machine = plant();

    let audit = machine.audit();

    assert_eq!(one(&audit, "shipped").standing, Standing::Merged);
    assert_eq!(one(&audit, "review/api").standing, Standing::OnRemote);
    assert_eq!(one(&audit, "importer/retry").standing, Standing::Unpushed);
    assert_eq!(one(&audit, "importer/retry").unpushed, 3);
    assert_eq!(audit.base.as_deref(), Some("origin/main"), "{audit:#?}");
}

/// The branch a `fetch --prune` has already stopped explaining.
#[test]
fn a_branch_whose_upstream_was_deleted_says_so_and_is_loud() {
    let machine = plant();

    let orphan = machine.audit();
    let orphan = one(&orphan, "orphan");

    assert!(orphan.upstream_gone, "{orphan:?}");
    assert_eq!(orphan.standing, Standing::Unpushed, "its commit is on no remote: {orphan:?}");
    assert_eq!(orphan.unpushed, 1);
}

/// The branch a worktree has checked out is not a row here.
///
/// The worktree section already reports it, with its size, its dirty count and the
/// intent of the session that made it. A branch in both sections is a report that
/// counts itself twice.
#[test]
fn a_branch_a_worktree_has_checked_out_is_left_to_the_worktree_section() {
    let machine = plant();
    git(&machine.checkout, &["worktree", "add", "--quiet", "../side", "review/api"]);

    let audit = machine.audit();

    assert!(
        !audit.rows.iter().any(|row| row.name == "review/api"),
        "a branch with a worktree is the worktree section's row: {:#?}",
        audit.rows
    );
    assert!(!audit.rows.iter().any(|row| row.name == "main"), "{:#?}", audit.rows);
    assert!(audit.rows.iter().any(|row| row.name == "importer/retry"), "{:#?}", audit.rows);
}

/// The rendering claim: 295 safe rows must not bury 22 dangerous ones.
#[test]
fn the_default_rendering_is_loud_about_one_bucket_and_counts_the_other_two() {
    let machine = plant();
    let text = report(machine.audit(), false);

    assert!(text.contains("importer/retry"), "{text}");
    assert!(text.contains("orphan"), "{text}");
    assert!(text.contains("gone"), "the upstream that is not there: {text}");
    assert!(text.contains("1 merged into origin/main"), "{text}");
    assert!(text.contains("1 unmerged, every commit on a remote"), "{text}");
    assert!(!text.contains("shipped"), "a safe branch is a count, not a row: {text}");
    assert!(!text.contains("review/api"), "{text}");
}

#[test]
fn all_opens_the_safe_buckets_and_finds_nothing_new() {
    let machine = plant();
    let audit = machine.audit();
    let rows = audit.rows.len();

    let opened = report(audit, true);

    assert!(opened.contains("shipped"), "{opened}");
    assert!(opened.contains("review/api"), "{opened}");
    assert!(opened.contains("importer/retry"), "the loud bucket stays: {opened}");
    assert_eq!(rows, machine.audit().rows.len(), "--all is a rendering, not a second answer");
}

/// The whole report, rendered, with the safe buckets open or closed.
fn report(branches: Branches, expand: bool) -> String {
    let doctor = nodal_core::output::view::doctor::Doctor {
        now: Timestamp::now(),
        checkout: None,
        here: Vec::new(),
        elsewhere: Vec::new(),
        branches: Branches { expand, ..branches },
        notes: Vec::new(),
    };
    doctor.doc().to_string()
}

/// The audit reads and never writes, like every other source in doctor.
///
/// This is the source with the most reason to be checked: `for-each-ref` and `rev-list`
/// are reads, but a Git command run in a repository can refresh an index or write a
/// commit graph. The facade runs every one of them with `GIT_OPTIONAL_LOCKS=0`, and
/// this is what says so about the machine rather than about the flag.
#[test]
fn the_audit_writes_nothing_anywhere() {
    let machine = plant();
    let before = snapshot(machine.directory.path());

    let audit = machine.audit();
    assert!(!audit.rows.is_empty(), "the machine was read");

    let after = snapshot(machine.directory.path());
    let missing: Vec<&PathBuf> = before.keys().filter(|path| !after.contains_key(*path)).collect();
    assert!(missing.is_empty(), "the audit removed {missing:#?}");
    let added: Vec<&PathBuf> = after.keys().filter(|path| !before.contains_key(*path)).collect();
    assert!(added.is_empty(), "the audit wrote {added:#?}");
    for (path, was) in &before {
        assert_eq!(after.get(path), Some(was), "the audit changed {}", path.display());
    }
}

/// How many refs the timing fixture builds, near the 317 a measured machine held.
const MANY: usize = 300;

/// The cost this design accepts, measured.
///
/// The audit is two `for-each-ref` calls for the whole repository and one `rev-list`
/// per ref. The second is one process per branch and cannot be fewer, because
/// `--not --remotes` is a question about one tip. A measured machine answered 317 refs
/// in about twenty seconds, and this holds the shape: three hundred refs, three hundred
/// and two processes, and the elapsed time printed for a person to read.
#[test]
fn three_hundred_refs_cost_one_rev_list_each() {
    let machine = plant();
    let mut refs = String::new();
    let head = text(&machine.checkout, &["rev-parse", "refs/heads/importer/retry"]);
    for index in 0..MANY {
        writeln!(refs, "create refs/heads/audit/{index:04} {head}").unwrap();
    }
    update_refs(&machine.checkout, &refs);

    let started = Instant::now();
    let audit = machine.audit();
    let took = started.elapsed();

    assert_eq!(audit.rows.len(), MANY + 4, "every ref is a row: {}", audit.rows.len());
    eprintln!(
        "{MANY} refs audited in {took:.2?} ({:.1?} per ref)",
        took / u32::try_from(MANY).unwrap()
    );
}

/// Write many refs in one process, so that the fixture is not what is being timed.
fn update_refs(dir: &Path, commands: &str) {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["update-ref", "--stdin"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("git runs");
    child.stdin.take().expect("a pipe").write_all(commands.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success(), "git update-ref failed");
}

/// Every path under `root`, with the size and the modification time of each.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
    let mut found = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                queue.push(path.clone());
            }
            found.insert(path, (metadata.len(), metadata.modified().ok()));
        }
    }
    found
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

/// What a Git command printed, without its trailing newline.
fn text(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "--all"]);
    git(dir, &["commit", "--quiet", "--message", message]);
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().expect("a parent")).unwrap();
    std::fs::write(path, body).unwrap();
}
