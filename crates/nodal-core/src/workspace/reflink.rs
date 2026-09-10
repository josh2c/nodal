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
//! Whether a filesystem shares blocks is not a property of its name. XFS shares them
//! only when `mkfs.xfs -m reflink=1` made it, which no mount option and no `statfs`
//! field reports. So this backend answers by trying one clone in the directory it is
//! asked about ([`super::sharing`] is what asks, once), and the table below only names
//! filesystems for a person to read.
//!
//! Where one file still cannot be cloned once the walk has started, the bytes are
//! copied instead and the report counts it. A home that is partly shared is still a
//! home; a report that hid it would not be true.

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

    fn available(&self) -> bool {
        platform::AVAILABLE
    }

    fn shares_blocks(&self) -> bool {
        true
    }

    fn filesystem(&self, directory: &Path) -> Option<String> {
        platform::filesystem(directory)
    }

    fn clone_probe(&self, source: &Path, destination: &Path) -> std::io::Result<()> {
        platform::clone_probe(source, destination)
    }

    fn clone_tree_on(
        &self,
        source: &Path,
        destination: &Path,
        exclude: &Excludes,
        workers: usize,
    ) -> Result<Report> {
        materialize(source, destination, exclude, Ops { file: put }, workers)
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

    /// This platform has `FICLONE`, so a probe here is worth the file it writes.
    pub(super) const AVAILABLE: bool = true;

    /// One filesystem and how `statfs` names it.
    struct Filesystem {
        /// The value `statfs` reports in `f_type`, held in a type wide enough for
        /// every target, where the field is signed on one and unsigned on another.
        magic: i128,
        /// What it is, for a reader of this table and for a person reading a report.
        name: &'static str,
    }

    /// The filesystems this table can name. It is a table of names and nothing else.
    /// Whether a filesystem shares blocks is not a property of its magic number: XFS
    /// shares them only when `mkfs.xfs -m reflink=1` made it, `OpenZFS` since 2.2 does,
    /// and overlayfs does when the upper layer does. A clone is tried instead.
    ///
    /// A filesystem that is not here reads as its magic number, because a person
    /// reporting one is what puts the next row in the table. `ext2` and `ext3` report
    /// the magic `ext4` reports, so a machine on either of them reads `ext4` here.
    const KNOWN: &[Filesystem] = &[
        Filesystem { magic: 0x9123_683E, name: "btrfs" },
        Filesystem { magic: 0x5846_5342, name: "xfs" },
        Filesystem { magic: 0xca45_1a4e, name: "bcachefs" },
        Filesystem { magic: 0x0000_EF53, name: "ext4" },
        Filesystem { magic: 0x0102_1994, name: "tmpfs" },
        Filesystem { magic: 0x794c_7630, name: "overlayfs" },
        Filesystem { magic: 0x2FC1_2FC1, name: "zfs" },
        Filesystem { magic: 0xF2F5_2010, name: "f2fs" },
        Filesystem { magic: 0x7366_746E, name: "ntfs3" },
        Filesystem { magic: 0x0102_1997, name: "9p" },
        Filesystem { magic: 0x6573_5546, name: "fuse" },
    ];

    /// What the filesystem holding `directory` is called, and `None` where `statfs`
    /// could not answer.
    pub(super) fn filesystem(directory: &Path) -> Option<String> {
        let magic = magic(directory)?;
        Some(
            KNOWN
                .iter()
                .find(|filesystem| filesystem.magic == magic)
                .map_or_else(|| format!("filesystem 0x{magic:x}"), |found| found.name.to_owned()),
        )
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
        std::fs::remove_file(destination).map_err(Error::io(destination))?;
        let bytes = std::fs::copy(source, destination).map_err(Error::io(destination))?;
        Ok(Put { bytes, shared: false })
    }

    /// The one call a probe makes: `FICLONE` and nothing else. It copies no bytes,
    /// because a probe asks whether blocks can be shared and a copy is not an answer.
    ///
    /// # Errors
    /// The kernel's own error, unchanged. The caller reads `EOPNOTSUPP`, `EXDEV` and
    /// `EINVAL` as "this filesystem does not share blocks", and every other errno as
    /// "the question could not be put".
    pub(super) fn clone_probe(source: &Path, destination: &Path) -> std::io::Result<()> {
        let from = File::open(source)?;
        let to = OpenOptions::new().write(true).create_new(true).mode(0o600).open(destination)?;
        // SAFETY: both descriptors are open for the length of the call, and `FICLONE`
        // reads the second argument as a descriptor and writes nothing back.
        if unsafe { libc::ioctl(to.as_raw_fd(), FICLONE, from.as_raw_fd()) } == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
    }
}

/// The same two answers on a platform without `FICLONE`.
#[cfg(not(target_os = "linux"))]
mod platform {
    use std::fs::Metadata;
    use std::path::Path;

    use super::super::tree::Put;
    use crate::error::{Error, Result};

    /// No filesystem here has `FICLONE`, so this backend is never selected and a
    /// probe never writes a file to find that out.
    pub(super) const AVAILABLE: bool = false;

    /// This backend names no filesystem away from Linux.
    pub(super) fn filesystem(_directory: &Path) -> Option<String> {
        None
    }

    /// Never called, because [`AVAILABLE`] is false.
    ///
    /// # Errors
    /// Always, because this platform has no call that shares blocks.
    pub(super) fn clone_probe(_source: &Path, _destination: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    /// Refuse, because [`AVAILABLE`] said so.
    pub(super) fn clone_file(
        source: &Path,
        _destination: &Path,
        _metadata: &Metadata,
    ) -> Result<Put> {
        Err(Error::MaterializeUnsupported { backend: "reflink", path: source.to_path_buf() })
    }
}
