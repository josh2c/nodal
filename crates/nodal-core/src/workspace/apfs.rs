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
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    use crate::error::{Error, Result};

    /// This platform has `clonefile`, so a probe here is worth the file it writes.
    pub(super) const AVAILABLE: bool = true;

    /// `CLONE_NOFOLLOW` from `<sys/clonefile.h>`: do not resolve a source that is a
    /// symbolic link. The `libc` crate does not publish this constant, so it is stated
    /// here with the header it comes from rather than borrowed from another API's
    /// constant that happens to have the same value.
    const CLONE_NOFOLLOW: u32 = 0x0001;

    /// What the filesystem holding `directory` is called, as `statfs` names it. APFS
    /// says `apfs`. The name is reported and never selected on; a clone is tried.
    pub(super) fn filesystem(directory: &Path) -> Option<String> {
        name(directory)
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
    ///
    /// This is the boundary between the two error types. Everything below it answers
    /// with the operating system's own error, and this function is where that error
    /// becomes [`Error::Io`] naming the path it is about.
    ///
    /// # Errors
    /// [`Error::Io`] naming the file that could not be cloned.
    pub(super) fn clone(source: &Path, destination: &Path) -> Result<()> {
        let from = c_path(source).map_err(Error::io(source))?;
        let to = c_path(destination).map_err(Error::io(destination))?;
        call(&from, &to).map_err(Error::io(destination))
    }

    /// The one call a probe makes: `clonefile` and nothing else.
    ///
    /// # Errors
    /// The operating system's own error, unchanged, so that the caller can tell a
    /// volume that will not clone from one it could not write in.
    pub(super) fn clone_probe(source: &Path, destination: &Path) -> std::io::Result<()> {
        call(&c_path(source)?, &c_path(destination)?)
    }

    /// The call itself, which both callers make and neither repeats.
    ///
    /// # Errors
    /// The operating system's own error. A caller that owes its own kind of error maps
    /// this one; a caller that does not hands it on.
    fn call(from: &CStr, to: &CStr) -> std::io::Result<()> {
        // SAFETY: both paths are NUL-terminated C strings that outlive the call.
        if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), CLONE_NOFOLLOW) } == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
    }

    /// A path as the C string `clonefile` takes.
    ///
    /// # Errors
    /// `InvalidInput`, for the one path a C string cannot hold: one with a NUL in it.
    fn c_path(path: &Path) -> std::io::Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))
    }
}

/// The same two answers on a platform without `clonefile`.
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::path::Path;

    use crate::error::{Error, Result};

    /// No filesystem here has `clonefile`, so this backend is never selected and a
    /// probe never writes a file to find that out.
    pub(super) const AVAILABLE: bool = false;

    /// This backend names no filesystem away from macOS.
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
    pub(super) fn clone(source: &Path, _destination: &Path) -> Result<()> {
        Err(Error::MaterializeUnsupported { backend: "clonefile", path: source.to_path_buf() })
    }
}
