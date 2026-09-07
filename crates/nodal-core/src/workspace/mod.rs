//! Materialization: how a unit home is made from a base.
//!
//! A home is a copy of a clean base, and the copy has to be cheap enough that a person
//! makes one without thinking about it. Every filesystem Nodal supports can share the
//! blocks of a file between two names, so a copy costs metadata and nothing else; the
//! backends here are the ways to ask for that, one per filesystem, behind one trait.
//!
//! [`select_backend`] answers once, for a path, and an operation never asks again.
//! Where nothing can share blocks, [`copy::CopyFallback`] copies the bytes and says so,
//! because a home that costs its own disk still works.

pub mod apfs;
pub mod copy;
pub mod exclude;
pub mod home;
pub mod meta;
pub mod reflink;
pub mod tree;
pub mod walk;
pub mod xattr;

use std::path::Path;

use serde::Serialize;

use crate::error::Result;
pub use exclude::Excludes;

/// What a clone left behind. Every count is of entries in the source tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Regular files put across one at a time.
    pub files: usize,
    /// Directories the copier made itself.
    pub directories: usize,
    /// Symbolic links recreated.
    pub symlinks: usize,
    /// Files that are a second name for a file already copied.
    pub hardlinks: usize,
    /// Extended attributes carried over.
    pub attributes: usize,
    /// Entries the exclusion list left out.
    pub excluded: usize,
    /// Entries the source had when it was listed and no longer had when they were
    /// copied. A live tree is allowed to change under a clone; what it holds is
    /// reported rather than made into a failure.
    pub vanished: usize,
    /// Files whose blocks could not be shared, so their bytes were copied.
    pub copied: usize,
    /// Bytes the clone holds, as the source counts them. On a backend that shares
    /// blocks, almost none of them are new bytes on the disk.
    pub bytes: u64,
}

/// How a tree is copied onto one filesystem.
///
/// The trait is the seam between the operations, which are the same everywhere, and
/// the one call per filesystem that makes a copy cheap.
pub trait Materializer {
    /// The name of this backend, as a report and a log line name it.
    fn name(&self) -> &'static str;

    /// Whether this backend works for a tree at `path`. The answer is about the
    /// filesystem `path` is on, so it holds for the nearest directory that exists.
    fn supports(&self, path: &Path) -> bool;

    /// Copy the tree at `source` into `destination`, leaving out what `exclude` names.
    ///
    /// # Errors
    /// [`crate::Error::MaterializeDestination`] when the destination cannot be used,
    /// [`crate::Error::MaterializeUnsupported`] when this backend does not work there,
    /// [`crate::Error::Io`] when an entry could not be read or written.
    fn clone_tree(&self, source: &Path, destination: &Path, exclude: &Excludes) -> Result<Report>;
}

/// The backends, in the order they are tried.
fn backends() -> [Box<dyn Materializer>; 3] {
    [Box::new(apfs::ApfsClonefile), Box::new(reflink::ReflinkCopy), Box::new(copy::CopyFallback)]
}

/// The backend that makes the cheapest copy at `path`.
///
/// The answer is taken once, before an operation starts, and the operation then never
/// branches on which one it got. The last backend works everywhere, so there is always
/// an answer; it warns, because a home that costs its own disk is worth knowing about.
#[must_use]
pub fn select_backend(path: &Path) -> Box<dyn Materializer> {
    let fallback = || -> Box<dyn Materializer> { Box::new(copy::CopyFallback) };
    let chosen =
        backends().into_iter().find(|backend| backend.supports(path)).unwrap_or_else(fallback);
    if chosen.name() == copy::CopyFallback.name() {
        tracing::warn!(
            path = %path.display(),
            "this filesystem cannot share blocks between files, so each unit home costs its own disk"
        );
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::{Materializer, backends, copy::CopyFallback, select_backend};

    #[test]
    fn the_last_backend_works_everywhere() {
        let directory = std::env::temp_dir();
        let last =
            backends().into_iter().next_back().is_some_and(|backend| backend.supports(&directory));
        assert!(last, "the fallback must support any path");
        assert!(CopyFallback.supports(&directory));
    }

    #[test]
    fn a_path_always_gets_a_backend() {
        let chosen = select_backend(&std::env::temp_dir());
        assert!(!chosen.name().is_empty());
    }
}
