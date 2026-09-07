//! The macOS backend: `clonefile(2)`.
//!
//! `clonefile` copies a file by giving the copy the same blocks and marking them
//! copy-on-write. This backend makes one call per regular file. Everything else about
//! the clone — directories, symbolic links, hard links, permissions, extended
//! attributes and times — is the shared copier's, exactly as it is for every other
//! backend, so one clone is one clone whichever machine made it.
//!
//! `clonefile` also takes a directory and copies everything below it in one call. This
//! backend does not use that call. It makes a tree the shared copier would not have
//! made: two names for one file become two files, because one call on a directory
//! cannot know that two names below it are one file, and a directory the call wrote
//! into carries the time it was written rather than the time its source had.
//!
//! **No measurement here has been made.** The project has no macOS machine at the
//! moment, so nobody has run this backend against a real base on an APFS disk. The
//! tests and the acceptance script select it on the macOS runner the project builds
//! on, which shows that a clone made this way holds what it must; they say nothing
//! about what it costs on a base of the size a person works with. Read the speed of
//! this backend as intent until a run on a Mac is recorded.
//!
//! The file compiles on every platform. Away from macOS it reports that it does not
//! work, so nothing selects it and nothing depends on it.

use std::fs::Metadata;
use std::path::Path;

use super::tree::{Ops, Put, materialize};
use super::{Excludes, Materializer, Report};
use crate::error::Result;

/// Shares the blocks of every file with the base it came from, on APFS.
#[derive(Debug, Clone, Copy, Default)]
pub struct ApfsClonefile;

impl Materializer for ApfsClonefile {
    fn name(&self) -> &'static str {
        "clonefile"
    }

    fn supports(&self, path: &Path) -> bool {
        platform::supports(path)
    }

    fn clone_tree(&self, source: &Path, destination: &Path, exclude: &Excludes) -> Result<Report> {
        materialize(source, destination, exclude, Ops { file: put })
    }
}

/// Clone one file.
///
/// # Errors
/// [`crate::Error::Io`] naming the file that could not be cloned.
fn put(source: &Path, destination: &Path, metadata: &Metadata) -> Result<Put> {
    platform::clone(source, destination)?;
    Ok(Put { bytes: std::os::unix::fs::MetadataExt::size(metadata), shared: true })
}

/// Everything that is one call on macOS and nothing anywhere else.
#[cfg(target_os = "macos")]
mod platform {
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    use crate::error::{Error, Result};

    /// What APFS calls itself in `statfs`.
    const APFS: &str = "apfs";

    /// `CLONE_NOFOLLOW` from `<sys/clonefile.h>`: do not resolve a source that is a
    /// symbolic link. The `libc` crate does not publish this constant, so it is stated
    /// here with the header it comes from rather than borrowed from another API's
    /// constant that happens to have the same value.
    const CLONE_NOFOLLOW: u32 = 0x0001;

    /// Whether the filesystem holding `path` is APFS. The path need not exist yet: the
    /// answer is about the nearest directory that does.
    pub(super) fn supports(path: &Path) -> bool {
        nearest(path).and_then(name).is_some_and(|found| found == APFS)
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

    /// The name `statfs` gives the filesystem holding `path`.
    fn name(path: &Path) -> Option<String> {
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `statfs` is a plain C structure, for which the call fills every
        // field this function reads.
        let mut answer: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: the path is a NUL-terminated C string that outlives the call.
        if unsafe { libc::statfs(path.as_ptr(), &raw mut answer) } != 0 {
            return None;
        }
        let bytes: Vec<u8> = answer
            .f_fstypename
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| u8::try_from(*byte).unwrap_or(0))
            .collect();
        String::from_utf8(bytes).ok()
    }

    /// Clone one regular file.
    ///
    /// `CLONE_NOFOLLOW` is what makes the copier's rule about symbolic links hold at
    /// the one call that could break it. Without the flag `clonefile` resolves a
    /// source that is a link and copies what it points at, so a link the walk had
    /// already classified would become a file if it were replaced between the two.
    pub(super) fn clone(source: &Path, destination: &Path) -> Result<()> {
        let (from, to) = (c_path(source)?, c_path(destination)?);
        // SAFETY: both paths are NUL-terminated C strings that outlive the call.
        if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), CLONE_NOFOLLOW) } == 0 {
            return Ok(());
        }
        Err(Error::Io { path: destination.to_path_buf(), source: std::io::Error::last_os_error() })
    }

    /// A path as the C string `clonefile` takes.
    fn c_path(path: &Path) -> Result<std::ffi::CString> {
        std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::Io {
            path: path.to_path_buf(),
            source: std::io::Error::from(std::io::ErrorKind::InvalidInput),
        })
    }
}

/// The same two answers on a platform without `clonefile`.
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    use crate::error::{Error, Result};

    /// No filesystem here has `clonefile`, so this backend is never selected.
    pub(super) fn supports(_path: &Path) -> bool {
        false
    }

    /// Refuse, because [`supports`] said so.
    pub(super) fn clone(source: &Path, _destination: &Path) -> Result<()> {
        Err(Error::MaterializeUnsupported { backend: "clonefile", path: source.to_path_buf() })
    }
}
