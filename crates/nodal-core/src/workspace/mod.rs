//! Materialization: how a unit home is made from a base.
//!
//! A home is a copy of a clean base, and the copy has to be cheap enough that a person
//! makes one without thinking about it. Every filesystem Nodal supports can share the
//! blocks of a file between two names, so a copy costs metadata and nothing else; the
//! backends here are the ways to ask for that, one per filesystem, behind one trait.
//!
//! [`backends`] is the only list of them. It answers three questions with one order:
//! which backend a clone probe asks, what the filesystem holding a path is called, and
//! which backend an operation gets. A second list would let the report and the copy
//! disagree, which is the one thing a person cannot check.
//!
//! [`select_backend`] takes the recorded answer for the state root
//! ([`sharing::Sharing`]) and never asks a filesystem anything. The question is asked
//! once, when the state root is made, and every command after that reads the record.
//! Where the record says blocks are not shared, [`copy::CopyFallback`] copies the bytes
//! and says so, because a home that costs its own disk still works.
//!
//! A copy is not always usable where it lands. [`relocate`] is what a home does about
//! the content that recorded the path it was made at, and it runs once the copy is
//! there.
//!
//! A copy stops being worth its disk. [`prune`] is what a reclaimed home loses on its
//! way to the trash: the build output and the installed dependencies it was keeping
//! warm, which nothing will ever read again and which a tool writes again in minutes.
//! It removes only what an ignore rule covers and only what the exclusion table calls
//! regenerable, so the trash keeps every file that holds work.
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
pub mod prune;
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
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::error::Result;
pub use exclude::Excludes;
use sharing::{Shares, Sharing};

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
/// the one call per filesystem that makes a copy cheap. A backend answers what it is,
/// what this platform lets it do, and what the filesystem in front of it is called, so
/// that [`backends`] can be the only list of backends this crate holds.
pub trait Materializer {
    /// The name of this backend, as a report and a log line name it.
    fn name(&self) -> &'static str;

    /// Whether this platform has the one call this backend makes. This is a property
    /// of the build, not of a path: it is the same answer for every directory.
    fn available(&self) -> bool;

    /// Whether a copy this backend makes shares blocks with its source.
    fn shares_blocks(&self) -> bool;

    /// What the filesystem holding `directory` is called, and `None` where this
    /// backend cannot name it. `directory` must exist; [`nearest`] is how a caller
    /// gets one.
    ///
    /// The name is for a person to read. It decides nothing: a name says which
    /// filesystem this is, never whether that filesystem was formatted or mounted so
    /// that it shares blocks.
    fn filesystem(&self, directory: &Path) -> Option<String>;

    /// Try one clone, which is how a probe asks whether blocks can be shared.
    ///
    /// `source` exists and `destination` does not. The call copies no bytes: a probe
    /// asks whether blocks can be shared, and a copy is not an answer.
    ///
    /// # Errors
    /// The operating system's own error, unchanged, because the caller sorts a
    /// filesystem that will not share blocks from a directory it could not write in.
    fn clone_probe(&self, source: &Path, destination: &Path) -> std::io::Result<()>;

    /// Copy the tree at `source` into `destination`, leaving out what `exclude` names,
    /// on `workers` workers.
    ///
    /// The result does not depend on the count: the same tree, byte for byte, with the
    /// same report, at every number of workers. [`tree::materialize`] says how, and
    /// `tests/safety/tests/clone_identity.rs` asserts it.
    ///
    /// # Errors
    /// [`crate::Error::MaterializeDestination`] when the destination cannot be used,
    /// [`crate::Error::MaterializeUnsupported`] when this backend does not work there,
    /// [`crate::Error::Io`] when an entry could not be read or written.
    fn clone_tree_on(
        &self,
        source: &Path,
        destination: &Path,
        exclude: &Excludes,
        workers: usize,
    ) -> Result<Report>;

    /// The same copy, on the worker count [`tree::workers`] gives this machine.
    ///
    /// Every caller but a measurement and the identity test uses this one.
    ///
    /// # Errors
    /// As [`Materializer::clone_tree_on`].
    fn clone_tree(&self, source: &Path, destination: &Path, exclude: &Excludes) -> Result<Report> {
        self.clone_tree_on(source, destination, exclude, tree::workers())
    }
}

/// The backends, in the order they are tried. This is the only list of them.
fn backends() -> [Box<dyn Materializer>; 3] {
    [Box::new(apfs::ApfsClonefile), Box::new(reflink::ReflinkCopy), Box::new(copy::CopyFallback)]
}

/// The backend that shares blocks on this platform, and `None` where this build has
/// none.
///
/// One backend answers both halves of the sharing question: it names the filesystem a
/// report prints, and it makes the clone a probe tries. The same backend is then what
/// [`select_backend`] hands out where the record says blocks are shared, so the report
/// and the copy cannot disagree.
pub(crate) fn sharing_backend() -> Option<Box<dyn Materializer>> {
    backends().into_iter().find(|backend| backend.available() && backend.shares_blocks())
}

/// The backend that makes the cheapest copy, from the answer recorded for the state
/// root.
///
/// Nothing here touches a filesystem. The question was asked once, when the state root
/// was made, and [`sharing`] is where the answer is kept; an operation reads it and
/// never asks again. The fallback works everywhere, so there is always an answer.
#[must_use]
pub fn select_backend(record: &Sharing) -> Box<dyn Materializer> {
    if record.shares == Shares::Yes
        && let Some(backend) = sharing_backend()
    {
        return backend;
    }
    tracing::warn!(
        root = %record.root.display(),
        answer = record.shares.word(),
        "nodal does not share blocks at the state root, so each unit home costs its own disk"
    );
    Box::new(copy::CopyFallback)
}

/// The nearest ancestor of `path`, itself included, that is a directory.
///
/// Every question here is about a filesystem, and a path that does not exist yet is on
/// the filesystem of the directory it will be made in. This is how a backend answers
/// for a home before the home is there.
///
/// A regular file, and a symbolic link that points at nothing, are both treated as
/// absent: neither can be written in, so neither can answer a question that is asked
/// by writing a file. The parent is asked instead.
pub(crate) fn nearest(path: &Path) -> Option<&Path> {
    let mut candidate = Some(path);
    while let Some(path) = candidate {
        if path.is_dir() {
            return Some(path);
        }
        candidate = path.parent().filter(|parent| !parent.as_os_str().is_empty());
    }
    None
}

/// How much a probe writes. Small enough to cost nothing, large enough that btrfs
/// stores it as an extent rather than inside the inode, which is the shape a real file
/// has and the shape a clone is asked about.
const PROBE_BYTES: usize = 4096;

/// The name every file a probe writes begins with, so that a probe killed between the
/// write and the removal leaves something the next probe recognises and clears.
const PROBE_PREFIX: &str = ".nodal-sharing-";

/// Whether `directory` can give one file the blocks of another, by trying it once.
///
/// A magic number cannot answer this. XFS shares blocks only when it was formatted
/// that way (`mkfs.xfs -m reflink=1`), which is not a mount option and is not in
/// `statfs`; `OpenZFS` shares them from 2.2; overlayfs shares them when the upper layer
/// does. So the question is asked the only way that is true on every host: write one
/// small file and try one clone with the backend's own call.
///
/// `clone` is handed a source that exists and a destination that does not, and answers
/// with the operating system's own error where it would not clone.
///
/// The answer has three values, because a directory that could not be written in has
/// not said that it copies. It has said nothing, and a full disk, a read-only mount or
/// a directory owned by somebody else all reach here. Saying "cannot share blocks"
/// there would demote every home on a filesystem that shares them perfectly well.
pub(crate) fn probe_sharing(
    directory: &Path,
    clone: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Shares {
    sweep(directory);
    let (source, destination) = probe_paths(directory);
    let answer = ask(&source, &destination, clone);
    drop(std::fs::remove_file(&destination));
    drop(std::fs::remove_file(&source));
    tracing::debug!(
        directory = %directory.display(),
        answer = answer.word(),
        "asked whether blocks can be shared here"
    );
    answer
}

/// Write the file and try the clone, which is the probe itself.
fn ask(
    source: &Path,
    destination: &Path,
    clone: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Shares {
    if let Err(error) = write_probe(source) {
        return Shares::could_not_ask(&error);
    }
    match clone(source, destination) {
        Ok(()) => Shares::Yes,
        Err(error) if unanswered(&error) => Shares::could_not_ask(&error),
        Err(_) => Shares::No,
    }
}

/// Whether an error from a clone says nothing about whether this filesystem shares
/// blocks.
///
/// A refusal to share is `EOPNOTSUPP`, `EXDEV` or `EINVAL`, and that is an answer. A
/// disk with no room on it, a mount with no write on it and a directory somebody else
/// owns are not answers, and neither is `EAGAIN`: `OpenZFS` returns it for a source
/// whose blocks are not on the disk yet, which is a filesystem that shares blocks
/// saying "not this instant". `ENOENT` is not one either: it means somebody took one of
/// the two files away while the probe was running.
fn unanswered(error: &std::io::Error) -> bool {
    use std::io::ErrorKind::{
        Interrupted, NotFound, PermissionDenied, QuotaExceeded, ReadOnlyFilesystem, StorageFull,
        WouldBlock,
    };
    matches!(
        error.kind(),
        PermissionDenied
            | ReadOnlyFilesystem
            | StorageFull
            | QuotaExceeded
            | WouldBlock
            | Interrupted
            | NotFound
    )
}

/// How old one of a probe's files has to be before another probe clears it. A probe
/// takes microseconds, so anything this old was left by a process that is gone; the age
/// is what keeps a sweep from taking the file another probe is using this instant.
const PROBE_STALE: Duration = Duration::from_secs(60);

/// Remove what a probe that was killed left in `directory`.
///
/// A probe writes two files and removes both, and a `SIGKILL` between the two leaves
/// one behind. Nothing else sweeps the state root, so a probe clears the last probe's
/// leavings before it starts. Both names carry [`PROBE_PREFIX`], so this can never
/// reach a file a person owns, and only a file [`PROBE_STALE`] old is taken, so it can
/// never reach a probe that is still running.
fn sweep(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(PROBE_PREFIX) && stale(&entry) {
            drop(std::fs::remove_file(entry.path()));
        }
    }
}

/// Whether a file was left behind rather than being written this instant. A file whose
/// age cannot be read is left alone: a sweep is tidying, and tidying may not guess.
fn stale(entry: &std::fs::DirEntry) -> bool {
    entry
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|written| SystemTime::now().duration_since(written).ok())
        .is_some_and(|age| age >= PROBE_STALE)
}

/// Two names in `directory` that nothing else holds: the process, a counter within it,
/// and the clock, so that two Nodal runs probing one state root at once cannot collide.
fn probe_paths(directory: &Path) -> (PathBuf, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::UNIX_EPOCH.elapsed().map_or(0, |since| since.as_nanos());
    let unique = format!("{}-{}-{nanos}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    (
        directory.join(format!("{PROBE_PREFIX}from-{unique}")),
        directory.join(format!("{PROBE_PREFIX}to-{unique}")),
    )
}

/// Write the file a probe clones, and put its blocks on the disk.
///
/// `create_new` so that a probe can never write over something a person owns, whatever
/// name it happened to pick. The sync is what makes the answer true on `OpenZFS`,
/// which refuses to clone a file whose blocks are still only in memory: without it a
/// filesystem that shares blocks answers `EAGAIN` for a file written a microsecond ago.
///
/// # Errors
/// The operating system's own error, which the caller reports as "could not ask".
fn write_probe(source: &Path) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(source)?;
    file.write_all(&[0; PROBE_BYTES])?;
    file.sync_all()
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::io::{Error, ErrorKind};
    use std::path::Path;

    use super::sharing::{Shares, Sharing};
    use super::{
        Materializer, backends, copy::CopyFallback, nearest, probe_sharing, select_backend,
        sharing_backend,
    };
    use crate::model::Timestamp;

    /// A record with the answer a test wants, about a state root no test touches.
    fn record(shares: Shares) -> Sharing {
        Sharing {
            root: Path::new("/home/j/.nodal").to_path_buf(),
            filesystem: Some(String::from("btrfs")),
            shares,
            probed_at: Timestamp::now(),
            device: Some(66),
        }
    }

    #[test]
    fn the_last_backend_works_everywhere() {
        let last = backends().into_iter().next_back().expect("a last backend");
        assert!(last.available(), "the fallback must work on every platform");
        assert!(!last.shares_blocks(), "the fallback copies bytes");
        assert_eq!(last.name(), CopyFallback.name());
    }

    /// One list of backends answers both halves of the question. A second list is what
    /// let the report and the copy disagree.
    #[test]
    fn one_backend_shares_blocks_on_this_platform() {
        let sharing = sharing_backend();
        assert_eq!(sharing.is_some(), cfg!(any(target_os = "linux", target_os = "macos")));
        if let Some(backend) = sharing {
            assert!(backend.available() && backend.shares_blocks());
        }
    }

    #[test]
    fn a_record_that_shares_blocks_gets_the_backend_that_shares_them() {
        let chosen = select_backend(&record(Shares::Yes));
        let expected = sharing_backend().map_or_else(|| CopyFallback.name(), |ours| ours.name());
        assert_eq!(chosen.name(), expected);
    }

    /// Neither "no" nor "could not ask" gets a backend that shares blocks. A copy that
    /// works is what a machine nobody could ask about needs.
    #[test]
    fn every_other_answer_copies_the_bytes() {
        for answer in
            [Shares::No, Shares::could_not_ask(&Error::from(ErrorKind::ReadOnlyFilesystem))]
        {
            assert_eq!(select_backend(&record(answer)).name(), CopyFallback.name());
        }
    }

    /// The seam the backends are tested through. A host has one answer to give and
    /// this project is developed on hosts that give both, so the call each backend
    /// makes is stood in for and every answer is read on any machine.
    fn probe(directory: &Path, answer: std::io::Result<()>) -> Shares {
        probe_sharing(directory, |source, destination| {
            assert_eq!(source.parent(), Some(directory), "the probe writes where it was asked");
            assert!(source.is_file(), "the clone is offered a file that exists");
            assert!(!destination.exists(), "and a destination that does not");
            answer?;
            std::fs::write(destination, b"cloned")
        })
    }

    #[test]
    fn a_directory_whose_clone_succeeds_shares_blocks() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(probe(directory.path(), Ok(())), Shares::Yes);
    }

    #[test]
    fn a_directory_whose_clone_is_refused_does_not_share_blocks() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        assert_eq!(probe(directory.path(), Err(Error::from(ErrorKind::Unsupported))), Shares::No);
    }

    /// A clone that failed for a reason that is not about sharing has said nothing.
    /// `EAGAIN` is the one that matters: `OpenZFS` answers it for a file written a
    /// moment ago, and reading it as "no" demotes a filesystem that shares blocks.
    #[test]
    fn a_clone_that_could_not_run_is_not_an_answer() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        for kind in [ErrorKind::WouldBlock, ErrorKind::StorageFull, ErrorKind::PermissionDenied] {
            let answer = probe(directory.path(), Err(Error::from(kind)));
            assert!(matches!(answer, Shares::CouldNotAsk { .. }), "{kind:?}: {answer:?}");
        }
    }

    /// Every answer leaves the directory as it found it. A probe that littered would
    /// put a file in the state root that nothing else ever sweeps.
    #[test]
    fn a_probe_leaves_nothing_behind() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        for answer in [Ok(()), Err(Error::from(ErrorKind::Unsupported))] {
            probe(directory.path(), answer);
            let left: Vec<_> = std::fs::read_dir(directory.path())
                .expect("a readable directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .collect();
            assert!(left.is_empty(), "{left:?}");
        }
    }

    /// What a probe a `SIGKILL` stopped left behind is cleared by the next probe, and
    /// what a probe running right now wrote is not.
    #[test]
    fn a_probe_clears_what_a_killed_probe_left_and_nothing_newer() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let orphan = directory.path().join(format!("{}orphan", super::PROBE_PREFIX));
        let running = directory.path().join(format!("{}running", super::PROBE_PREFIX));
        for path in [&orphan, &running] {
            std::fs::write(path, b"a probe's file").expect("a file");
        }
        age(&orphan, super::PROBE_STALE * 2);

        assert_eq!(probe(directory.path(), Ok(())), Shares::Yes);
        assert!(!orphan.exists(), "a killed probe's file is still there");
        assert!(running.is_file(), "a probe running now had its file taken");
    }

    /// Put a file's modification time `age` into the past, so that a sweep sees the
    /// file a killed probe would have left rather than one written this instant.
    fn age(path: &Path, age: std::time::Duration) {
        let then = std::time::SystemTime::now() - age;
        let file = std::fs::File::options().write(true).open(path).expect("a writable file");
        file.set_modified(then).expect("a file whose time can be set");
    }

    /// A directory nothing can be written to has not said that it copies. It has said
    /// nothing, and the errno is carried so that a person can see which nothing.
    #[test]
    fn a_directory_that_cannot_be_written_to_could_not_be_asked() {
        let missing = std::env::temp_dir().join("nodal-no-such-directory-for-a-probe");
        let answer = probe_sharing(&missing, |_, _| Ok(()));
        let Shares::CouldNotAsk { errno, why } = answer else {
            panic!("a directory that is not there answered {answer:?}");
        };
        assert_eq!(errno, Some(libc::ENOENT));
        assert!(!why.is_empty());
    }

    #[test]
    fn the_nearest_existing_ancestor_is_where_a_question_is_asked() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let unmade = directory.path().join("home").join("deeper");
        assert_eq!(nearest(&unmade), Some(directory.path()));
        assert_eq!(nearest(directory.path()), Some(directory.path()));
    }

    /// A regular file cannot be written in, so it is not the directory that answers.
    /// The old rule stopped at one and then asked it, which failed every time.
    #[test]
    fn a_regular_file_is_not_where_a_question_is_asked() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let file = directory.path().join("registry.db");
        std::fs::write(&file, b"not a directory").expect("a file");
        assert_eq!(nearest(&file), Some(directory.path()));
        assert_eq!(nearest(&file.join("under")), Some(directory.path()));
    }

    /// A symbolic link that points at nothing is absent as well, for the same reason:
    /// nothing can be written through it.
    #[test]
    #[cfg(unix)]
    fn a_dangling_link_is_not_where_a_question_is_asked() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let link = directory.path().join("state");
        std::os::unix::fs::symlink(directory.path().join("gone"), &link).expect("a link");
        assert_eq!(nearest(&link), Some(directory.path()));
    }

    #[test]
    fn a_relative_path_with_no_ancestor_has_nowhere_to_ask() {
        assert_eq!(nearest(Path::new("no/such/relative/path")), None);
    }
}
