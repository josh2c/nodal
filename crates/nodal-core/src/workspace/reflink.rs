//! The Linux backend: one `FICLONE` per file.
//!
//! `FICLONE` is the call behind `cp --reflink`. It gives the new file the same blocks
//! as the old one and marks them copy-on-write, so the copy costs metadata and the
//! disk holds one set of bytes until one of the two is written to. A base of any size
//! becomes a home in the time it takes to walk it.
//!
//! A unit home is a reflinked **directory**, not a filesystem snapshot. A btrfs
//! subvolume snapshot is as cheap, but deleting a subvolume needs a mount option this
//! project cannot assume, and reclaiming a home must never need privilege. Directory
//! reflink has no such restriction and measured the same cost.
//!
//! Where a file cannot be cloned — a filesystem that says it cannot, or a source the
//! kernel refuses — the bytes are copied instead and the report counts it. A home that
//! is partly shared is still a home; a report that hid it would not be true.

use std::fs::Metadata;
use std::path::Path;

use super::tree::{Ops, Put, materialize};
use super::{Excludes, Materializer, Report};
use crate::error::Result;

/// Shares the blocks of every file with the base it came from. Works on filesystems
/// with copy-on-write files: btrfs, XFS formatted with reflink, bcachefs.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReflinkCopy;

impl Materializer for ReflinkCopy {
    fn name(&self) -> &'static str {
        "reflink"
    }

    fn supports(&self, path: &Path) -> bool {
        platform::supports(path)
    }

    fn clone_tree(&self, source: &Path, destination: &Path, exclude: &Excludes) -> Result<Report> {
        materialize(source, destination, exclude, Ops { file: put })
    }
}

/// Clone one file, or copy its bytes where the kernel will not clone it.
///
/// # Errors
/// [`crate::Error::Io`] naming the file that could not be read or written.
fn put(source: &Path, destination: &Path, metadata: &Metadata) -> Result<Put> {
    platform::clone_file(source, destination, metadata)
}

/// Everything that is one call on Linux and nothing anywhere else.
#[cfg(target_os = "linux")]
mod platform {
    use std::fs::{File, Metadata, OpenOptions};
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    use super::super::tree::Put;
    use crate::error::{Error, Result};

    /// `FICLONE`, which is `_IOW(0x94, 9, int)`. The encoding is the same on every
    /// architecture Nodal releases for.
    const FICLONE: libc::c_ulong = 0x4004_9409;

    /// One filesystem that can share blocks between files, and how `statfs` names it.
    struct Filesystem {
        /// The value `statfs` reports in `f_type`, held in a type wide enough for
        /// every target, where the field is signed on one and unsigned on another.
        magic: i128,
        /// What it is, for a reader of this table.
        name: &'static str,
    }

    /// The filesystems that can share blocks. A filesystem that is not here gets the
    /// copying backend; a filesystem that is here but refuses one file copies that
    /// file, so a row that is too generous costs bytes, never correctness.
    const SHARING: &[Filesystem] = &[
        Filesystem { magic: 0x9123_683E, name: "btrfs" },
        Filesystem { magic: 0x5846_5342, name: "xfs" },
        Filesystem { magic: 0xca45_1a4e, name: "bcachefs" },
    ];

    /// Whether the filesystem holding `path` can share blocks between files. The path
    /// need not exist yet: the answer is about the nearest directory that does.
    pub(super) fn supports(path: &Path) -> bool {
        let Some(existing) = nearest(path) else { return false };
        let Some(magic) = magic(existing) else { return false };
        let found = SHARING.iter().find(|filesystem| filesystem.magic == magic);
        if let Some(filesystem) = found {
            tracing::debug!(path = %existing.display(), filesystem = filesystem.name, "blocks can be shared here");
        }
        found.is_some()
    }

    /// The nearest ancestor of `path`, itself included, that exists.
    fn nearest(path: &Path) -> Option<&Path> {
        let mut candidate = Some(path);
        while let Some(path) = candidate {
            if path.symlink_metadata().is_ok() {
                return Some(path);
            }
            candidate = path.parent();
        }
        None
    }

    /// What `statfs` reports for the filesystem holding `path`.
    fn magic(path: &Path) -> Option<i128> {
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `statfs` is a plain C structure of integers, for which every byte
        // pattern is a value, and the call fills it before it is read.
        let mut answer: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: the path is a NUL-terminated C string that outlives the call.
        if unsafe { libc::statfs(path.as_ptr(), &raw mut answer) } != 0 {
            return None;
        }
        Some(i128::from(answer.f_type))
    }

    /// Give `destination` the blocks of `source`, or its bytes where the kernel
    /// refuses to share them. Reports how many bytes the file holds.
    pub(super) fn clone_file(
        source: &Path,
        destination: &Path,
        metadata: &Metadata,
    ) -> Result<Put> {
        let from = File::open(source).map_err(Error::io(source))?;
        let to = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(metadata.mode() & 0o777)
            .open(destination)
            .map_err(Error::io(destination))?;
        // SAFETY: both descriptors are open for the length of the call, and `FICLONE`
        // reads the second argument as a descriptor and writes nothing back.
        let answer = unsafe { libc::ioctl(to.as_raw_fd(), FICLONE, from.as_raw_fd()) };
        if answer == 0 {
            return Ok(Put { bytes: metadata.size(), shared: true });
        }
        drop((from, to));
        crate::remove::file(destination)?;
        let bytes = std::fs::copy(source, destination).map_err(Error::io(destination))?;
        Ok(Put { bytes, shared: false })
    }
}

/// The same two answers on a platform without `FICLONE`.
#[cfg(not(target_os = "linux"))]
mod platform {
    use std::fs::Metadata;
    use std::path::Path;

    use super::super::tree::Put;
    use crate::error::{Error, Result};

    /// No filesystem here has `FICLONE`, so this backend is never selected.
    pub(super) fn supports(_path: &Path) -> bool {
        false
    }

    /// Refuse, because [`supports`] said so.
    pub(super) fn clone_file(
        source: &Path,
        _destination: &Path,
        _metadata: &Metadata,
    ) -> Result<Put> {
        Err(Error::MaterializeUnsupported { backend: "reflink", path: source.to_path_buf() })
    }
}
