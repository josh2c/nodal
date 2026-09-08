//! The Git facade: every `git` invocation Nodal makes goes through this type.
//!
//! Nodal talks to Git through its command line rather than a library, so refs, hooks and
//! config behave exactly as the user's own tools see them. A unit is an independent
//! repository, never a worktree, so this facade has no worktree operations — only
//! detection, for adopting an existing checkout in place.

pub mod cmd;
pub mod integration;
pub mod merge;
pub mod oid;
pub mod preflight;
pub mod refs;
pub mod remote;
pub mod scrub;
pub mod snapshot;
pub mod status;
pub mod tree;
pub mod worktree;

use std::path::{Path, PathBuf};

pub use self::integration::{Divergence, Integration, Standing};
pub use self::oid::Oid;
use crate::error::{Error, Result};

/// A branch and the commit it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short name, without `refs/heads/`.
    pub name: String,
    /// The commit at its tip.
    pub oid: Oid,
}

/// A repository Nodal reads and writes: a unit's home, a base, or a user's checkout.
#[derive(Debug, Clone)]
pub struct Git {
    /// The directory `git` is run in.
    root: PathBuf,
}

impl Git {
    /// Open a repository at `root`.
    ///
    /// # Errors
    /// [`Error::NotARepository`] when `root` is not inside a Git repository,
    /// [`Error::GitSpawn`] when `git` could not be started.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let git = Self { root: root.into() };
        if cmd::run(&git.root, &["rev-parse", "--git-dir"])?.ok() {
            Ok(git)
        } else {
            Err(Error::NotARepository { path: git.root })
        }
    }

    /// Open a repository at `root` without asking Git whether it is one.
    ///
    /// [`Git::open`] spends a `git` invocation to answer a question the next invocation
    /// answers anyway. A reader that is about to run a command, and that reports what
    /// that command said, uses this instead: the list asks about ten homes and pays for
    /// one process each rather than two.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory every invocation runs in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where this checkout keeps its Git state, and whether it shares it with another.
    ///
    /// # Errors
    /// [`Error::Git`] when `git rev-parse` failed.
    pub fn layout(&self) -> Result<worktree::Layout> {
        let paths = cmd::run_ok(
            &self.root,
            &["rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir"],
        )?;
        let lines = paths.lines()?;
        let [git_dir, common_dir] = lines.as_slice() else {
            return Err(Error::GitParse {
                args: paths.args.clone(),
                record: paths.text()?.to_owned(),
            });
        };
        let bare = cmd::run_ok(&self.root, &["rev-parse", "--is-bare-repository"])?;
        Ok(worktree::classify(
            PathBuf::from(git_dir),
            PathBuf::from(common_dir),
            bare.text()? == "true",
        ))
    }

    /// The root of this checkout's working tree.
    ///
    /// A command run in a subdirectory of a project is a command about the project, so
    /// every operation resolves the directory it was given to this before it acts.
    ///
    /// # Errors
    /// [`Error::Git`] when the checkout is bare and has no working tree.
    pub fn top_level(&self) -> Result<PathBuf> {
        let output = cmd::run_ok(&self.root, &["rev-parse", "--show-toplevel"])?;
        Ok(PathBuf::from(output.text()?))
    }

    /// The Git directory of this checkout.
    ///
    /// # Errors
    /// As [`Git::layout`].
    pub fn git_dir(&self) -> Result<PathBuf> {
        Ok(self.layout()?.git_dir)
    }

    /// Resolve a revision to a full object id.
    ///
    /// # Errors
    /// [`Error::Git`] when the revision is unknown, [`Error::GitOid`] on unreadable output.
    pub fn rev_parse(&self, rev: &str) -> Result<Oid> {
        let output = cmd::run_ok(&self.root, &["rev-parse", "--verify", "--end-of-options", rev])?;
        Oid::parse(output.text()?)
    }

    /// Resolve a revision, `None` when it does not exist.
    ///
    /// # Errors
    /// [`Error::GitOid`] when a resolved id could not be read.
    pub fn rev_parse_opt(&self, rev: &str) -> Result<Option<Oid>> {
        let output =
            cmd::run(&self.root, &["rev-parse", "--verify", "--quiet", "--end-of-options", rev])?;
        if !output.ok() {
            return Ok(None);
        }
        Ok(Some(Oid::parse(output.text()?)?))
    }

    /// List the tree at a revision. `recursive` walks subtrees; `paths` limits the walk.
    ///
    /// One call of this is what a workspace fingerprint is built from
    /// (`docs/contracts.md`, fingerprint inputs).
    ///
    /// # Errors
    /// [`Error::Git`] when the revision is unknown, [`Error::GitParse`] on an unreadable
    /// record.
    pub fn ls_tree(&self, rev: &str, recursive: bool, paths: &[&str]) -> Result<Vec<tree::Entry>> {
        let mut args = vec!["ls-tree", "-z", "--full-tree"];
        if recursive {
            args.push("-r");
        }
        args.push(rev);
        args.push("--");
        args.extend_from_slice(paths);
        let output = cmd::run_ok(&self.root, &args)?;
        tree::parse(&output.args, &output.records()?)
    }

    /// Read the working tree's status, untracked files included, ignored files excluded.
    ///
    /// # Errors
    /// [`Error::Git`] when `git status` failed, [`Error::GitParse`] on an unreadable record.
    pub fn status(&self) -> Result<status::Summary> {
        let output = cmd::run_ok(
            &self.root,
            &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"],
        )?;
        status::parse(&output.args, &output.records()?)
    }

    /// Where the checked-out branch stands against `base`: how far it has moved, and
    /// what merging it into `base` would do.
    ///
    /// This is what a list reports as a unit's integration. It reads history and trees
    /// and writes nothing a `git gc` does not collect.
    ///
    /// # Errors
    /// [`Error::Git`] when `base` is not a revision this repository has,
    /// [`Error::GitParse`] when a count or a tree identifier could not be read.
    pub fn standing(&self, base: &str) -> Result<integration::Standing> {
        integration::standing(&self.root, base)
    }

    /// The branch HEAD names, `None` when HEAD is detached.
    ///
    /// # Errors
    /// [`Error::GitEncoding`] when the name is not UTF-8.
    pub fn current_branch(&self) -> Result<Option<String>> {
        let output = cmd::run(&self.root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        if !output.ok() {
            return Ok(None);
        }
        Ok(Some(output.text()?.to_owned()))
    }

    /// Every local branch, sorted by name.
    ///
    /// # Errors
    /// [`Error::Git`] when `git for-each-ref` failed.
    pub fn branches(&self) -> Result<Vec<Branch>> {
        let listed = refs::list(&self.root, "refs/heads/")?;
        Ok(listed
            .into_iter()
            .map(|reference| Branch {
                name: reference.name.trim_start_matches("refs/heads/").to_owned(),
                oid: reference.oid,
            })
            .collect())
    }

    /// Whether a local branch exists.
    ///
    /// # Errors
    /// [`Error::Git`] when `git` failed for a reason other than a missing ref.
    pub fn branch_exists(&self, name: &str) -> Result<bool> {
        Ok(refs::read(&self.root, &format!("refs/heads/{name}"))?.is_some())
    }

    /// Create a branch at `start`, or at HEAD when `start` is `None`. Not idempotent:
    /// Git refuses a branch that already exists, which is how unit branches stay unique
    /// inside a repository.
    ///
    /// # Errors
    /// [`Error::Git`] when the branch exists or the start point is unknown.
    pub fn create_branch(&self, name: &str, start: Option<&str>) -> Result<()> {
        let mut args = vec!["branch", "--", name];
        args.extend(start);
        cmd::run_ok(&self.root, &args)?;
        Ok(())
    }

    /// Create a branch and check it out, the `nodal new` step after a clone is scrubbed.
    ///
    /// # Errors
    /// [`Error::Git`] when the branch exists or the working tree would be overwritten.
    pub fn switch_new(&self, name: &str, start: Option<&str>) -> Result<()> {
        let mut args = vec!["switch", "--create", name];
        args.extend(start);
        cmd::run_ok(&self.root, &args)?;
        Ok(())
    }

    /// Check out an existing branch.
    ///
    /// # Errors
    /// [`Error::Git`] when the branch is unknown or the working tree would be overwritten.
    pub fn switch(&self, name: &str) -> Result<()> {
        cmd::run_ok(&self.root, &["switch", "--", name])?;
        Ok(())
    }

    /// Delete a local branch. `force` deletes one whose commits are not merged.
    ///
    /// # Errors
    /// [`Error::Git`] when the branch is checked out, unknown, or unmerged without `force`.
    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        let flag = if force { "-D" } else { "-d" };
        cmd::run_ok(&self.root, &["branch", flag, "--", name])?;
        Ok(())
    }

    /// Read a full ref name, `None` when it does not exist.
    ///
    /// # Errors
    /// [`Error::Git`] when `git` failed for a reason other than a missing ref.
    pub fn read_ref(&self, name: &str) -> Result<Option<Oid>> {
        refs::read(&self.root, name)
    }

    /// Point a ref at an object, creating it if needed. `reason` goes in the reflog.
    ///
    /// # Errors
    /// [`Error::Git`] when `git update-ref` refused the name or the object.
    pub fn write_ref(&self, name: &str, oid: &Oid, reason: &str) -> Result<()> {
        refs::write(&self.root, name, oid, reason)
    }

    /// Delete a ref; deleting one that does not exist succeeds.
    ///
    /// # Errors
    /// [`Error::Git`] when `git update-ref -d` failed.
    pub fn delete_ref(&self, name: &str) -> Result<()> {
        refs::delete(&self.root, name)
    }

    /// Every ref under a prefix, sorted by name.
    ///
    /// # Errors
    /// [`Error::Git`] when `git for-each-ref` failed.
    pub fn list_refs(&self, prefix: &str) -> Result<Vec<refs::Ref>> {
        refs::list(&self.root, prefix)
    }

    /// Which commits of a revision exist on no remote. The Git half of the uniqueness
    /// check `nodal reclaim` runs before removing anything.
    ///
    /// # Errors
    /// [`Error::Git`] when the revision is unknown.
    pub fn remote_containment(&self, rev: &str) -> Result<remote::Containment> {
        remote::containment(&self.root, rev)
    }

    /// The URL a remote fetches from, `None` when there is no such remote.
    ///
    /// # Errors
    /// [`Error::GitSpawn`] when `git` could not be started, [`Error::GitEncoding`]
    /// when the URL is not UTF-8.
    pub fn remote_url(&self, name: &str) -> Result<Option<String>> {
        remote::url(&self.root, name)
    }

    /// Fetch every branch and tag a remote has.
    ///
    /// # Errors
    /// [`Error::Git`] when the remote could not be reached or does not exist.
    pub fn fetch(&self, remote: &str) -> Result<()> {
        cmd::run_ok(&self.root, &["fetch", "--quiet", "--", remote])?;
        Ok(())
    }

    /// Fetch one revision out of a repository on this machine, by path.
    ///
    /// Objects only: nothing of the other repository's working tree, index or refs
    /// comes across. This is how a base reaches a commit its remote does not have yet.
    ///
    /// # Errors
    /// [`Error::Git`] when the path is not a repository or does not have the revision,
    /// [`Error::InvalidValue`] when the path is not UTF-8.
    pub fn fetch_from(&self, source: &Path, rev: &str) -> Result<()> {
        cmd::run_ok(&self.root, &["fetch", "--quiet", "--no-tags", "--", text_of(source)?, rev])?;
        Ok(())
    }

    /// Put the working tree at a revision, with HEAD detached at it.
    ///
    /// Detached rather than on a branch because a base is a substrate and not a piece
    /// of work: the branch is made in the unit home that is cloned from it.
    ///
    /// # Errors
    /// [`Error::Git`] when the revision is unknown or the checkout failed.
    pub fn checkout_detached(&self, rev: &str) -> Result<()> {
        cmd::run_ok(&self.root, &["checkout", "--force", "--detach", "--end-of-options", rev])?;
        Ok(())
    }

    /// How many commits separate two revisions: the size of their symmetric
    /// difference, which is the distance a neighbour is chosen by.
    ///
    /// # Errors
    /// [`Error::Git`] when either revision is unknown, [`Error::GitParse`] when the
    /// count could not be read.
    pub fn distance(&self, from: &str, to: &str) -> Result<u32> {
        let range = format!("{from}...{to}");
        let output = cmd::run_ok(&self.root, &["rev-list", "--count", &range])?;
        let text = output.text()?;
        text.parse()
            .map_err(|_| Error::GitParse { args: output.args.clone(), record: text.to_owned() })
    }

    /// Whether a commit is really in this repository's object database.
    ///
    /// Not [`Git::rev_parse_opt`]: given a full object id, `git rev-parse --verify`
    /// answers from the text alone and says yes for an object the repository has never
    /// had. Asking for `<rev>^{commit}` makes it read the object, which is the question
    /// a fetch is decided by.
    ///
    /// # Errors
    /// [`Error::GitSpawn`] when `git` could not be started.
    pub fn has_commit(&self, rev: &str) -> Result<bool> {
        let peeled = format!("{rev}^{{commit}}");
        let args = ["rev-parse", "--verify", "--quiet", "--end-of-options", peeled.as_str()];
        Ok(cmd::run(&self.root, &args)?.ok())
    }

    /// The top of the working tree this repository is a checkout of.
    ///
    /// Nodal keys a project by its top level, so two commands run in two directories of
    /// one repository name one project rather than two.
    ///
    /// # Errors
    /// [`Error::Git`] when `git rev-parse` failed, [`Error::GitEncoding`] when the path
    /// is not UTF-8.
    pub fn toplevel(&self) -> Result<PathBuf> {
        let output =
            cmd::run_ok(&self.root, &["rev-parse", "--path-format=absolute", "--show-toplevel"])?;
        Ok(PathBuf::from(output.text()?))
    }

    /// Every worktree this repository has, the main one included.
    ///
    /// This is a read. `git worktree list` states what the repository already records
    /// and changes nothing, which is why `nodal doctor` may call it.
    ///
    /// # Errors
    /// [`Error::Git`] when `git worktree list` failed, [`Error::GitEncoding`] when its
    /// output is not UTF-8.
    pub fn worktrees(&self) -> Result<Vec<worktree::Registered>> {
        worktree::list(&self.root)
    }

    /// Which Git operations, if any, are in progress here.
    ///
    /// # Errors
    /// As [`Git::layout`].
    pub fn preflight(&self) -> Result<preflight::Report> {
        Ok(preflight::inspect(&self.git_dir()?))
    }

    /// Refuse to go on when a Git operation is in progress.
    ///
    /// # Errors
    /// [`Error::GitInProgress`] listing every state found; otherwise as [`Git::layout`].
    pub fn ensure_no_operation_in_progress(&self) -> Result<()> {
        let report = self.preflight()?;
        if report.is_clear() {
            return Ok(());
        }
        Err(Error::GitInProgress { repo: self.root.clone(), states: report.states })
    }

    /// Commit everything this repository holds to a ref, without touching its index.
    ///
    /// The safety net `nodal reclaim --force` takes before it removes a home
    /// ([`snapshot`]). `None` when the repository has no commit to build on.
    ///
    /// # Errors
    /// [`Error::Git`] when a plumbing command failed, [`Error::Io`] when the temporary
    /// index could not be removed.
    pub fn snapshot(&self, reference: &str, message: &str) -> Result<Option<snapshot::Snapshot>> {
        snapshot::take(&self.root, &self.git_dir()?, reference, message)
    }

    /// Scrub the Git state a copy-on-write clone inherited from its base.
    ///
    /// # Errors
    /// [`Error::GitLinkedWorktree`] on a linked or bare checkout, [`Error::GitInProgress`]
    /// when the clone inherited a half-finished operation, [`Error::GitUnknownBranch`] when
    /// the head branch does not exist, [`Error::Io`] when a removal failed.
    pub fn scrub(&self, options: &scrub::Options) -> Result<scrub::Report> {
        scrub::apply(&self.root, &self.layout()?, options)
    }

    /// Commit everything the working tree holds, untracked files included.
    ///
    /// # Errors
    /// As [`merge::commit_all`].
    pub fn commit_all(&self, message: &str) -> Result<Option<Oid>> {
        merge::commit_all(&self.root, message)
    }

    /// Fold what the branch has since its merge base with `onto` into one commit.
    ///
    /// # Errors
    /// As [`merge::squash`].
    pub fn squash(&self, onto: &Oid, message: &str) -> Result<Option<Oid>> {
        merge::squash(&self.root, onto, message)
    }

    /// Rebase the checked-out branch onto a commit, or carry on a rebase in progress.
    ///
    /// # Errors
    /// As [`merge::rebase`].
    pub fn rebase(&self, onto: &Oid) -> Result<merge::Outcome> {
        merge::rebase(&self.root, &self.git_dir()?, onto)
    }

    /// Carry on a rebase whose conflicts a person has resolved.
    ///
    /// # Errors
    /// As [`merge::resume`].
    pub fn resume_rebase(&self) -> Result<merge::Outcome> {
        merge::resume(&self.root, &self.git_dir()?)
    }

    /// Stop a rebase and put the branch back where it was.
    ///
    /// # Errors
    /// As [`merge::abort`].
    pub fn abort_rebase(&self) -> Result<()> {
        merge::abort(&self.root, &self.git_dir()?)
    }

    /// Whether this repository is in the middle of a rebase.
    ///
    /// # Errors
    /// As [`Git::layout`].
    pub fn is_rebasing(&self) -> Result<bool> {
        Ok(merge::rebasing(&self.git_dir()?))
    }

    /// The branch a rebase in progress will put back when it finishes.
    ///
    /// # Errors
    /// As [`Git::layout`].
    pub fn rebasing_branch(&self) -> Result<Option<String>> {
        Ok(merge::rebasing_branch(&self.git_dir()?))
    }

    /// The commit two revisions last had in common.
    ///
    /// # Errors
    /// As [`merge::merge_base`].
    pub fn merge_base(&self, left: &str, right: &str) -> Result<Oid> {
        merge::merge_base(&self.root, left, right)
    }

    /// Whether every commit of `earlier` is in `later`.
    ///
    /// # Errors
    /// As [`merge::is_ancestor`].
    pub fn is_ancestor(&self, earlier: &str, later: &str) -> Result<bool> {
        merge::is_ancestor(&self.root, earlier, later)
    }

    /// How many commits a range holds.
    ///
    /// # Errors
    /// As [`merge::count`].
    pub fn count(&self, range: &str) -> Result<u32> {
        merge::count(&self.root, range)
    }

    /// Move a local branch to a commit, and only when that is a fast-forward.
    ///
    /// # Errors
    /// As [`merge::fast_forward`].
    pub fn fast_forward(&self, branch: &str, to: &Oid) -> Result<bool> {
        merge::fast_forward(&self.root, branch, to)
    }

    /// Put a local branch back at the commit it pointed at.
    ///
    /// # Errors
    /// As [`merge::restore`].
    pub fn restore_branch(&self, branch: &str, to: &Oid) -> Result<()> {
        merge::restore(&self.root, branch, to)
    }

    /// Fetch one branch of a repository on this machine onto a ref of Nodal's own.
    ///
    /// # Errors
    /// As [`merge::fetch_branch`].
    pub fn fetch_branch(&self, from: &Path, branch: &str, into: &str) -> Result<Oid> {
        merge::fetch_branch(&self.root, from, branch, into)
    }

    /// What a symbolic ref points at, `None` when there is no such ref.
    ///
    /// # Errors
    /// [`Error::GitEncoding`] when the answer is not UTF-8.
    pub fn symbolic_ref(&self, name: &str) -> Result<Option<String>> {
        refs::symbolic(&self.root, name)
    }
}

/// Clone a remote into `destination`, which must not exist.
///
/// This is how the first base of a project is made. The credentials are the ones the
/// user's own `git` would use — a helper, an agent, or a token in the environment —
/// because [`cmd::run`] adds nothing but the two settings that stop Git prompting and
/// taking optional locks. Nothing is read out of the user's checkout: a base is a clone
/// of the remote, so no uncommitted or local-only state can reach one.
///
/// # Errors
/// [`Error::Io`] when the parent directory could not be made, [`Error::InvalidValue`]
/// when `destination` is not UTF-8, [`Error::Git`] when the clone failed.
pub fn clone(url: &str, destination: &Path) -> Result<Git> {
    let parent = destination.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    cmd::run_ok(parent, &["clone", "--", url, text_of(destination)?])?;
    Git::open(destination)
}

/// A path as an argument. Git takes bytes, but the one seam takes `&str`, so a path
/// that is not UTF-8 is refused here rather than mangled on the way through.
fn text_of(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| Error::InvalidValue {
        kind: "path",
        value: path.to_string_lossy().into_owned(),
    })
}
