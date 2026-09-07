//! Where a fingerprint's input entries come from.
//!
//! A fingerprint is computed over `(path, mode, object id)` triples, never over file
//! contents, because Git has already hashed the contents. One `git ls-tree -r` at a
//! commit is the whole input, which is what makes a fingerprint cheap enough to take
//! on every command.
//!
//! [`TreeSource`] exists because there is a second source coming: staleness compares
//! the materialized fingerprint against the *working* tree, so an uncommitted lockfile
//! change counts (`docs/contracts.md`, fingerprint inputs). That source must produce
//! Git blob object ids for the same files, so that a clean checkout of a commit and
//! the commit itself fingerprint identically; it arrives with the staleness task.

use crate::Result;
use crate::git::{Git, tree::Entry};

/// A listing of the paths a fingerprint could be computed over.
pub trait TreeSource {
    /// Every entry, as `git ls-tree -r` would report it. Order is not promised;
    /// [`super::compute`] sorts.
    ///
    /// # Errors
    /// Whatever reading the source failed with.
    fn entries(&self) -> Result<Vec<Entry>>;
}

/// The tree of a commit, read with one `git ls-tree`.
#[derive(Debug, Clone, Copy)]
pub struct GitTreeAtCommit<'a> {
    /// The repository to read.
    repo: &'a Git,
    /// The revision to read it at; anything `git rev-parse` accepts.
    rev: &'a str,
}

impl<'a> GitTreeAtCommit<'a> {
    /// Read `repo`'s tree at `rev`.
    #[must_use]
    pub fn new(repo: &'a Git, rev: &'a str) -> Self {
        Self { repo, rev }
    }
}

impl TreeSource for GitTreeAtCommit<'_> {
    /// The whole tree in one call. The tree is listed rather than filtered by pathspec
    /// because the input table selects by file name at any depth, and one unfiltered
    /// listing is one process; a pathspec per selector would be dozens.
    fn entries(&self) -> Result<Vec<Entry>> {
        self.repo.ls_tree(self.rev, true, &[])
    }
}

/// A listing given directly, for tests and for callers that already hold one.
impl TreeSource for Vec<Entry> {
    fn entries(&self) -> Result<Vec<Entry>> {
        Ok(self.clone())
    }
}
