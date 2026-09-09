//! Integration tests for the Git facade, one per method, against temporary repositories.
//!
//! Nothing here touches a repository it did not create: every test builds its own under
//! a `tempfile` directory that is removed when the test ends.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};
use std::process::Command;

use nodal_core::error::Error;
use nodal_core::git::{Git, Oid, preflight, refs, scrub, status, tree, worktree};
use tempfile::TempDir;

/// A throwaway repository with a known history.
struct Repo {
    dir: TempDir,
}

impl Repo {
    /// An initialised repository on `main` with no commits.
    fn empty() -> Self {
        let repo = Self { dir: TempDir::new().unwrap() };
        repo.git(&["init", "--initial-branch=main"]);
        for (key, value) in [
            ("user.email", "tests@nodal.invalid"),
            ("user.name", "Nodal tests"),
            ("commit.gpgsign", "false"),
            ("gc.auto", "6700"),
        ] {
            repo.git(&["config", "--local", key, value]);
        }
        repo
    }

    /// An initialised repository with one commit adding `README.md`.
    fn seeded() -> Self {
        let repo = Self::empty();
        repo.write("README.md", "seed\n");
        repo.commit("seed");
        repo
    }

    /// The repository root.
    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The facade under test.
    fn git_facade(&self) -> Git {
        Git::open(self.path()).unwrap()
    }

    /// Run `git` directly, so the tests never depend on the code they exercise.
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
    }

    /// Write a file, creating parent directories.
    fn write(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// Stage everything and commit, returning the new commit id.
    fn commit(&self, message: &str) -> Oid {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--message", message]);
        Oid::parse(&self.git(&["rev-parse", "HEAD"])).unwrap()
    }
}

/// `Git::open` accepts a repository and refuses anything else.
#[test]
fn open_accepts_a_repository_and_refuses_a_plain_directory() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    assert_eq!(git.root(), repo.path());

    let plain = TempDir::new().unwrap();
    let error = Git::open(plain.path()).unwrap_err();
    assert!(matches!(error, Error::NotARepository { .. }), "{error:?}");
}

/// `Git::layout` and `Git::git_dir` classify the three shapes a checkout can have.
#[test]
fn layout_distinguishes_main_linked_and_bare_checkouts() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    let layout = git.layout().unwrap();
    assert_eq!(layout.kind, worktree::Kind::Main);
    assert!(!layout.is_linked());
    assert_eq!(git.git_dir().unwrap(), layout.git_dir);
    assert!(layout.git_dir.ends_with(".git"));

    let linked_path = repo.path().join("linked");
    repo.git(&["worktree", "add", "--detach", linked_path.to_str().unwrap()]);
    let linked = Git::open(&linked_path).unwrap().layout().unwrap();
    assert_eq!(linked.kind, worktree::Kind::Linked);
    assert!(linked.is_linked());
    assert_ne!(linked.git_dir, linked.common_dir);

    let bare_dir = TempDir::new().unwrap();
    let bare_path = bare_dir.path().join("bare.git");
    let status =
        Command::new("git").args(["init", "--bare", bare_path.to_str().unwrap()]).status().unwrap();
    assert!(status.success());
    assert_eq!(Git::open(&bare_path).unwrap().layout().unwrap().kind, worktree::Kind::Bare);
}

/// The layout of an ordinary checkout is the one Git reports, path for path.
///
/// `Git::layout` answers an ordinary checkout from the shape on disk and starts no
/// process. This is the lock on that: the answer is compared with what `git rev-parse`
/// says of the same checkout, reached directly and reached through a symbolic link,
/// because a home under a linked temporary directory is what CI runs.
#[test]
fn an_ordinary_checkout_is_laid_out_where_git_says_it_is() {
    let repo = Repo::seeded();
    let asked = run_git(repo.path(), &["rev-parse", "--path-format=absolute", "--git-dir"]);
    let layout = repo.git_facade().layout().unwrap();
    assert_eq!(layout.kind, worktree::Kind::Main);
    assert_eq!(layout.git_dir, PathBuf::from(&asked));
    assert_eq!(layout.common_dir, PathBuf::from(&asked));
    assert_eq!(run_git(repo.path(), &["rev-parse", "--is-bare-repository"]), "false");

    let elsewhere = TempDir::new().unwrap();
    let link = elsewhere.path().join("home");
    std::os::unix::fs::symlink(repo.path(), &link).unwrap();
    let through_link = Git::at(&link).layout().unwrap();
    assert_eq!(
        through_link.git_dir,
        PathBuf::from(run_git(&link, &["rev-parse", "--path-format=absolute", "--git-dir"]))
    );
    assert_eq!(through_link, layout);
}

/// A checkout that is not an ordinary one is laid out where Git says it is too.
///
/// The two shapes the fast path must decline: a linked worktree, whose Git directory
/// belongs to another repository, and a bare repository, which has no working tree.
/// Both answers are compared with `git rev-parse`, so a fast path that took either of
/// them would report a Git directory that is not the one Git names.
#[test]
fn a_linked_worktree_and_a_bare_repository_are_laid_out_where_git_says_they_are() {
    let repo = Repo::seeded();
    let linked_path = repo.path().join("linked");
    repo.git(&["worktree", "add", "--detach", linked_path.to_str().unwrap()]);
    let linked = Git::at(&linked_path).layout().unwrap();
    assert_eq!(linked.kind, worktree::Kind::Linked);
    assert_eq!(
        linked.git_dir,
        PathBuf::from(run_git(&linked_path, &["rev-parse", "--path-format=absolute", "--git-dir"]))
    );
    assert_eq!(
        linked.common_dir,
        PathBuf::from(run_git(
            &linked_path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"]
        ))
    );

    let bare_dir = TempDir::new().unwrap();
    let bare_path = bare_dir.path().join("bare.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare", bare_path.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let bare = Git::at(&bare_path).layout().unwrap();
    assert_eq!(bare.kind, worktree::Kind::Bare);
    assert_eq!(run_git(&bare_path, &["rev-parse", "--is-bare-repository"]), "true");

    let plain = TempDir::new().unwrap();
    assert!(Git::at(plain.path()).layout().is_err());
}

/// `Git::rev_parse` resolves what exists; `rev_parse_opt` reports what does not.
#[test]
fn rev_parse_resolves_revisions_and_reports_missing_ones() {
    let repo = Repo::seeded();
    let head = repo.commit_head();
    let git = repo.git_facade();
    assert_eq!(git.rev_parse("HEAD").unwrap(), head);
    assert_eq!(git.rev_parse("main").unwrap(), head);
    assert_eq!(git.rev_parse_opt("refs/heads/main").unwrap(), Some(head));
    assert_eq!(git.rev_parse_opt("no-such-branch").unwrap(), None);
    assert!(matches!(git.rev_parse("no-such-branch").unwrap_err(), Error::Git { .. }));
}

/// `Git::ls_tree` reads a commit's tree flat, recursively and limited to paths.
#[test]
fn ls_tree_reads_trees_flat_recursively_and_by_path() {
    let repo = Repo::seeded();
    repo.write("pkg/app/package.json", "{}\n");
    repo.write("pnpm-lock.yaml", "lock\n");
    repo.commit("tree");
    let git = repo.git_facade();

    let flat = git.ls_tree("HEAD", false, &[]).unwrap();
    let names: Vec<&Path> = flat.iter().map(|entry| entry.path.as_path()).collect();
    assert_eq!(names, [Path::new("README.md"), Path::new("pkg"), Path::new("pnpm-lock.yaml")]);
    assert_eq!(flat[1].kind, tree::Kind::Tree);

    let deep = git.ls_tree("HEAD", true, &[]).unwrap();
    assert!(deep.iter().any(|entry| entry.path == Path::new("pkg/app/package.json")));
    assert!(deep.iter().all(|entry| entry.kind == tree::Kind::Blob));

    let limited = git.ls_tree("HEAD", true, &["pnpm-lock.yaml"]).unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].mode, "100644");
    assert_eq!(git.rev_parse("HEAD:pnpm-lock.yaml").unwrap(), limited[0].oid);

    assert!(git.ls_tree("no-such-rev", false, &[]).is_err());
}

/// `Git::status` reads a clean tree, every entry shape, and divergence from an upstream.
#[test]
fn status_reports_head_entries_and_divergence() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    let clean = git.status().unwrap();
    assert_eq!(clean.head, status::Head::Branch("main".to_owned()));
    assert!(clean.is_clean());
    assert!(!clean.has_conflicts());

    repo.write("README.md", "changed\n");
    repo.write("new file.txt", "untracked\n");
    repo.git(&["add", "new file.txt"]);
    repo.write("another.txt", "also untracked\n");
    let dirty = git.status().unwrap();
    assert!(!dirty.is_clean());
    assert_eq!(dirty.uncommitted().count(), 3);
    let untracked =
        dirty.entries.iter().filter(|entry| entry.state == status::State::Untracked).count();
    assert_eq!(untracked, 1);
}

/// An unborn HEAD, a detached HEAD and a rename are all readable.
#[test]
fn status_reads_unborn_detached_and_renamed_states() {
    let empty = Repo::empty();
    assert_eq!(empty.git_facade().status().unwrap().head, status::Head::Unborn("main".to_owned()));

    let repo = Repo::seeded();
    let head = repo.commit_head();
    repo.git(&["checkout", "--detach"]);
    assert_eq!(repo.git_facade().status().unwrap().head, status::Head::Detached(head));

    repo.git(&["checkout", "main"]);
    repo.git(&["mv", "README.md", "READ ME.md"]);
    let renamed = repo.git_facade().status().unwrap();
    let entry = renamed.entries.first().unwrap();
    assert_eq!(entry.path, Path::new("READ ME.md"));
    assert_eq!(entry.origin.as_deref(), Some(Path::new("README.md")));
}

/// A conflicted merge shows unmerged paths.
#[test]
fn status_reports_unmerged_paths() {
    let repo = Repo::seeded();
    repo.git(&["switch", "--create", "other"]);
    repo.write("README.md", "theirs\n");
    repo.commit("theirs");
    repo.git(&["switch", "main"]);
    repo.write("README.md", "ours\n");
    repo.commit("ours");
    let merged =
        Command::new("git").arg("-C").arg(repo.path()).args(["merge", "other"]).output().unwrap();
    assert!(!merged.status.success());

    let summary = repo.git_facade().status().unwrap();
    assert!(summary.has_conflicts());
    assert!(!summary.is_clean());
}

/// Branch creation, switching, listing and deletion.
#[test]
fn branch_operations_create_switch_list_and_delete() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    assert_eq!(git.current_branch().unwrap().as_deref(), Some("main"));
    assert!(!git.branch_exists("nodal/fix-worker-import").unwrap());

    git.create_branch("nodal/plain", None).unwrap();
    assert!(git.branch_exists("nodal/plain").unwrap());
    assert!(git.create_branch("nodal/plain", None).is_err(), "a taken branch is refused");

    git.switch_new("nodal/fix-worker-import", Some("main")).unwrap();
    assert_eq!(git.current_branch().unwrap().as_deref(), Some("nodal/fix-worker-import"));

    let names: Vec<String> = git.branches().unwrap().into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["main", "nodal/fix-worker-import", "nodal/plain"]);
    assert_eq!(git.branches().unwrap()[0].oid, git.rev_parse("main").unwrap());

    git.switch("main").unwrap();
    assert_eq!(git.current_branch().unwrap().as_deref(), Some("main"));
    git.delete_branch("nodal/plain", false).unwrap();
    assert!(!git.branch_exists("nodal/plain").unwrap());
    assert!(git.switch("no-such-branch").is_err());

    repo.git(&["checkout", "--detach"]);
    assert_eq!(git.current_branch().unwrap(), None);
}

/// An unmerged branch needs `force` to go.
#[test]
fn deleting_an_unmerged_branch_needs_force() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    git.switch_new("nodal/work", None).unwrap();
    repo.write("work.txt", "unique\n");
    repo.commit("unique work");
    git.switch("main").unwrap();

    assert!(git.delete_branch("nodal/work", false).is_err());
    git.delete_branch("nodal/work", true).unwrap();
    assert!(!git.branch_exists("nodal/work").unwrap());
}

/// Refs round-trip through read, write, list and delete.
#[test]
fn refs_round_trip_in_the_nodal_namespace() {
    let repo = Repo::seeded();
    let head = repo.commit_head();
    let git = repo.git_facade();
    let wip = refs::wip("01JABCDEF");

    assert_eq!(git.read_ref(&wip).unwrap(), None);
    git.write_ref(&wip, &head, "test snapshot").unwrap();
    assert_eq!(git.read_ref(&wip).unwrap(), Some(head.clone()));
    git.write_ref(&wip, &head, "test snapshot again").unwrap();

    let listed = git.list_refs(refs::NAMESPACE).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, wip);
    assert_eq!(listed[0].oid, head);

    git.delete_ref(&wip).unwrap();
    assert_eq!(git.read_ref(&wip).unwrap(), None);
    git.delete_ref(&wip).unwrap();
    assert!(git.list_refs(refs::NAMESPACE).unwrap().is_empty());
    assert!(git.write_ref("refs/nodal/bad", &head, "reason").is_ok());
}

/// Remote containment: nothing is contained without a remote, everything is after a push.
#[test]
fn remote_containment_answers_whether_commits_exist_elsewhere() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    let alone = git.remote_containment("main").unwrap();
    assert!(alone.remotes.is_empty());
    assert!(!alone.is_contained());
    assert_eq!(alone.unpushed.len(), 1);

    let remote_dir = TempDir::new().unwrap();
    let remote_path = remote_dir.path().join("origin.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare", remote_path.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    repo.git(&["remote", "add", "origin", remote_path.to_str().unwrap()]);
    repo.git(&["push", "origin", "main"]);

    let pushed = git.remote_containment("main").unwrap();
    assert_eq!(pushed.remotes, ["origin"]);
    assert!(pushed.is_contained());

    repo.write("local.txt", "only here\n");
    let local = repo.commit("local only");
    let diverged = git.remote_containment("main").unwrap();
    assert_eq!(diverged.unpushed, [local]);
    assert!(!diverged.is_contained());
    assert!(git.remote_containment("no-such-rev").is_err());
}

/// Preflight is clear on a quiet repository and names every state it finds.
#[test]
fn preflight_names_in_progress_operations() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    assert!(git.preflight().unwrap().is_clear());
    git.ensure_no_operation_in_progress().unwrap();

    repo.git(&["switch", "--create", "other"]);
    repo.write("README.md", "theirs\n");
    repo.commit("theirs");
    repo.git(&["switch", "main"]);
    repo.write("README.md", "ours\n");
    repo.commit("ours");
    let _ = Command::new("git").arg("-C").arg(repo.path()).args(["merge", "other"]).output();

    let report = git.preflight().unwrap();
    assert_eq!(report.states, [preflight::State::Merge]);
    assert!(!report.is_clear());
    assert_eq!(preflight::State::Merge.marker(), "MERGE_HEAD");
    let error = git.ensure_no_operation_in_progress().unwrap_err();
    assert!(matches!(error, Error::GitInProgress { .. }), "{error:?}");
}

/// A lock file left by another process is an in-progress state too.
#[test]
fn preflight_refuses_a_locked_index() {
    let repo = Repo::seeded();
    let git = repo.git_facade();
    std::fs::write(git.git_dir().unwrap().join("index.lock"), "").unwrap();
    assert_eq!(git.preflight().unwrap().states, [preflight::State::IndexLock]);
}

/// The post-clone scrub removes inherited state and is safe to repeat.
#[test]
fn scrub_removes_inherited_state_and_repeats_cleanly() {
    let source = Repo::seeded();
    source.git(&["worktree", "add", "--detach", source.path().join("wt").to_str().unwrap()]);
    source.git(&["checkout", "--detach"]);
    let clone = clone_including_git(&source);
    let git = Git::open(clone.path()).unwrap();
    assert!(git.git_dir().unwrap().join("worktrees").exists(), "the clone inherits them");
    assert_eq!(git.current_branch().unwrap(), None, "and the source's detached HEAD");

    let options = scrub::Options { head_branch: Some("main".to_owned()) };
    let report = git.scrub(&options).unwrap();
    assert!(report.worktrees_removed);
    assert_eq!(report.head_set.as_deref(), Some("main"));
    assert!(report.gc_auto_disabled);
    assert!(!git.git_dir().unwrap().join("worktrees").exists());
    assert_eq!(run_git(clone.path(), &["config", "--get", "gc.auto"]), "0");
    assert_eq!(git.current_branch().unwrap().as_deref(), Some("main"));

    assert_eq!(git.scrub(&options).unwrap(), scrub::Report::default(), "idempotent");
    assert!(source.path().join("wt").exists(), "the source keeps its worktree");
}

/// The scrub drops a hooks path pointing outside the clone and keeps a relative one.
#[test]
fn scrub_only_clears_a_hooks_path_that_points_outside() {
    let source = Repo::seeded();
    let outside = source.path().join(".husky").to_str().unwrap().to_owned();
    source.git(&["config", "--local", "core.hooksPath", &outside]);
    let clone = clone_including_git(&source);
    let git = Git::open(clone.path()).unwrap();
    let cleared = git.scrub(&scrub::Options::default()).unwrap().hooks_path_cleared;
    assert_eq!(cleared, Some(PathBuf::from(&outside)));
    assert_eq!(run_git(clone.path(), &["config", "--get", "core.hooksPath"]), "");

    let inside = Repo::seeded();
    inside.git(&["config", "--local", "core.hooksPath", ".husky"]);
    let kept = clone_including_git(&inside);
    let report = Git::open(kept.path()).unwrap().scrub(&scrub::Options::default()).unwrap();
    assert_eq!(report.hooks_path_cleared, None);
    assert_eq!(run_git(kept.path(), &["config", "--get", "core.hooksPath"]), ".husky");
}

/// The scrub refuses anything it could damage: linked worktrees, in-progress state, and
/// a head branch that does not exist.
#[test]
fn scrub_refuses_unsafe_repositories() {
    let source = Repo::seeded();
    let linked = source.path().join("linked");
    source.git(&["worktree", "add", "--detach", linked.to_str().unwrap()]);
    let error = Git::open(&linked).unwrap().scrub(&scrub::Options::default()).unwrap_err();
    assert!(matches!(error, Error::GitLinkedWorktree { .. }), "{error:?}");

    let clone = clone_including_git(&source);
    let git = Git::open(clone.path()).unwrap();
    let unknown =
        git.scrub(&scrub::Options { head_branch: Some("nodal/absent".to_owned()) }).unwrap_err();
    assert!(matches!(unknown, Error::GitUnknownBranch { .. }), "{unknown:?}");

    std::fs::write(git.git_dir().unwrap().join("MERGE_HEAD"), "").unwrap();
    let busy = git.scrub(&scrub::Options::default()).unwrap_err();
    assert!(matches!(busy, Error::GitInProgress { .. }), "{busy:?}");
}

impl Repo {
    /// The commit HEAD points at.
    fn commit_head(&self) -> Oid {
        Oid::parse(&self.git(&["rev-parse", "HEAD"])).unwrap()
    }
}

/// Copy a repository's directory wholesale, `.git` included: what a copy-on-write clone
/// of a base produces, without needing a copy-on-write filesystem.
fn clone_including_git(source: &Repo) -> TempDir {
    let destination = TempDir::new().unwrap();
    copy_tree(source.path(), destination.path());
    destination
}

/// Recursive directory copy, symlinks aside; the fixtures contain none.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Run `git` in a directory that is not a [`Repo`], returning trimmed output or empty.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim_end().to_owned()
}
