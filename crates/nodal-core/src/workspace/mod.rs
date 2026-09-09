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
//!
//! A copy is not always usable where it lands. [`relocate`] is what a home does about
//! the content that recorded the path it was made at, and it runs once the copy is
//! there.
//!
//! A copy also has to be taken away again. [`remove`] is how, and it is one function
//! for every tree Nodal owns: a base holds content written read-only, so a home cloned
//! from one holds it too, and a removal that cannot open a read-only directory leaves a
//! directory behind that nothing will ever clear.
//!
//! A copy also has to be complete. [`tracked`] is the gate before one starts: it
//! refuses an exclusion list that would leave out a path the source commit tracks,
//! because a copy missing such a path is dirty the moment it is made.

pub mod apfs;
pub mod copy;
pub mod exclude;
pub mod home;
pub mod meta;
pub mod reflink;
pub mod relocate;
pub mod remove;
pub mod sharing;
pub mod tracked;
pub mod tree;
pub mod walk;
pub mod xattr;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// The nearest ancestor of `path`, itself included, that exists.
///
/// Every question here is about a filesystem, and a path that does not exist yet is on
/// the filesystem of the directory it will be made in. This is how a backend answers
/// for a home before the home is there.
pub(crate) fn nearest(path: &Path) -> Option<&Path> {
    let mut candidate = Some(path);
    while let Some(path) = candidate {
        if path.symlink_metadata().is_ok() {
            return Some(path);
        }
        candidate = path.parent();
    }
    None
}

/// How much a probe writes. Small enough to cost nothing, large enough that btrfs
/// stores it as an extent rather than inside the inode, which is the shape a real file
/// has and the shape a clone is asked about.
const PROBE_BYTES: usize = 4096;

/// Whether `directory` can give one file the blocks of another, by trying it once.
///
/// A magic number cannot answer this. XFS shares blocks only when it was formatted
/// that way (`mkfs.xfs -m reflink=1`), which is not a mount option and is not in
/// `statfs`; `OpenZFS` shares them from 2.2; overlayfs shares them when the upper layer
/// does. So the question is asked the only way that is true on every host: write one
/// small file and try one clone with the backend's own call.
///
/// `clone` is handed a source that exists and a destination that does not, and answers
/// whether the destination came away holding the source's blocks.
///
/// The probe leaves the directory as it found it. Both paths are removed either way,
/// and the directory gets the access and modification times it had back, because
/// `nodal doctor` reads a machine and writes nothing to it, and a probe that moved the
/// state root's clock forward on every report would be a write. A directory that
/// cannot be written to answers no, which is the answer the copy it stands for gives.
pub(crate) fn probe_sharing(directory: &Path, clone: impl FnOnce(&Path, &Path) -> bool) -> bool {
    let (source, destination) = probe_paths(directory);
    let times = times_of(directory);
    let shares = write_probe(&source) && clone(&source, &destination);
    drop(std::fs::remove_file(&destination));
    drop(std::fs::remove_file(&source));
    if let Some(times) = times {
        restore_times(directory, &times);
    }
    tracing::debug!(directory = %directory.display(), shares, "asked whether blocks can be shared here");
    shares
}

/// Two names in `directory` that nothing else holds: the process, a counter within it,
/// and the clock, so that two Nodal runs probing one state root at once cannot collide.
fn probe_paths(directory: &Path) -> (PathBuf, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::UNIX_EPOCH.elapsed().map_or(0, |since| since.as_nanos());
    let unique = format!("{}-{}-{nanos}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    (
        directory.join(format!(".nodal-sharing-{unique}")),
        directory.join(format!(".nodal-clone-{unique}")),
    )
}

/// Write the file a probe clones. `create_new` so that a probe can never write over
/// something a person owns, whatever name it happened to pick.
fn write_probe(source: &Path) -> bool {
    let Ok(mut file) = std::fs::OpenOptions::new().write(true).create_new(true).open(source) else {
        return false;
    };
    file.write_all(&[0; PROBE_BYTES]).is_ok()
}

/// The access and modification times `directory` has now, in the shape `utimensat`
/// takes them back in, and `None` where the directory could not be read.
fn times_of(directory: &Path) -> Option<[libc::timespec; 2]> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = std::fs::metadata(directory).ok()?;
    Some([
        timespec(metadata.atime(), metadata.atime_nsec()),
        timespec(metadata.mtime(), metadata.mtime_nsec()),
    ])
}

/// One `timespec` from the two numbers `stat` reports for one time.
#[allow(
    clippy::cast_possible_truncation,
    clippy::unnecessary_cast,
    reason = "both numbers come from this platform's own stat, so they fit its timespec"
)]
fn timespec(seconds: i64, nanoseconds: i64) -> libc::timespec {
    libc::timespec { tv_sec: seconds as libc::time_t, tv_nsec: nanoseconds as libc::c_long }
}

/// Give `directory` its times back. A failure is ignored: the probe has already
/// answered, and a clock that moved is not worth failing a report over.
fn restore_times(directory: &Path, times: &[libc::timespec; 2]) {
    use std::os::unix::ffi::OsStrExt as _;

    let Ok(path) = std::ffi::CString::new(directory.as_os_str().as_bytes()) else { return };
    // SAFETY: the path is a NUL-terminated C string and the two times are an array of
    // exactly the two elements `utimensat` reads, all of which outlive the call.
    unsafe {
        libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;

    use super::{
        Materializer, backends, copy::CopyFallback, nearest, probe_sharing, select_backend,
    };

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

    /// The seam the two backends are tested through. A host has one answer to give and
    /// this project is developed on hosts that give both, so the call each backend
    /// makes is stood in for and both answers are read on any machine.
    fn probe(directory: &Path, answer: bool) -> bool {
        probe_sharing(directory, |source, destination| {
            assert_eq!(source.parent(), Some(directory), "the probe writes where it was asked");
            assert!(source.is_file(), "the clone is offered a file that exists");
            assert!(!destination.exists(), "and a destination that does not");
            answer && std::fs::write(destination, b"cloned").is_ok()
        })
    }

    #[test]
    fn a_directory_whose_clone_succeeds_shares_blocks() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        assert!(probe(directory.path(), true));
    }

    #[test]
    fn a_directory_whose_clone_fails_does_not_share_blocks() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        assert!(!probe(directory.path(), false));
    }

    /// Both answers leave the directory as they found it. A probe that littered would
    /// put a file in the state root on every `nodal doctor`.
    #[test]
    fn a_probe_leaves_nothing_behind() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        for answer in [true, false] {
            probe(directory.path(), answer);
            let left: Vec<_> = std::fs::read_dir(directory.path())
                .expect("a readable directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .collect();
            assert!(left.is_empty(), "{left:?}");
        }
    }

    /// The directory keeps the time it had. `nodal doctor` probes on every run and
    /// writes nothing to the machine it reads, and a modification time is a write.
    #[test]
    fn a_probe_puts_the_directorys_time_back() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let was = std::fs::metadata(directory.path()).expect("a readable directory");
        probe(directory.path(), true);
        let now = std::fs::metadata(directory.path()).expect("a readable directory");
        assert_eq!(now.modified().ok(), was.modified().ok());
    }

    /// A directory nothing can be written to answers no rather than pretending.
    #[test]
    fn a_directory_that_cannot_be_written_to_does_not_share_blocks() {
        let missing = std::env::temp_dir().join("nodal-no-such-directory-for-a-probe");
        assert!(!probe_sharing(&missing, |_, _| true));
    }

    #[test]
    fn the_nearest_existing_ancestor_is_where_a_question_is_asked() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let unmade = directory.path().join("home").join("deeper");
        assert_eq!(nearest(&unmade), Some(directory.path()));
        assert_eq!(nearest(directory.path()), Some(directory.path()));
    }
}
