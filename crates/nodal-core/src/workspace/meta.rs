//! Permissions and timestamps, carried from an entry to its copy.
//!
//! A clone is only useful if the tools that read it cannot tell the difference. An
//! executable script must stay executable, and a build tool that decides what to redo
//! from modification times must see the times the source had, or the first command in
//! a new home rebuilds everything.
//!
//! Owner and group are not carried. A unit home belongs to the person who created it,
//! and changing an owner needs privilege that reclaim must never need either.

use std::fs::Metadata;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::error::{Error, Result};

/// Give `destination` the permissions `metadata` records. Links have no permissions of
/// their own on the platforms Nodal runs on, so a link is left alone.
///
/// # Errors
/// [`Error::Io`] when the mode could not be set.
pub fn permissions(destination: &Path, metadata: &Metadata) -> Result<()> {
    std::fs::set_permissions(destination, metadata.permissions()).map_err(Error::io(destination))
}

/// Give `destination` the access and modification times `metadata` records, without
/// following a link.
///
/// # Errors
/// [`Error::Io`] when the times could not be set.
pub fn times(destination: &Path, metadata: &Metadata) -> Result<()> {
    let times = [
        stamp(metadata.atime(), metadata.atime_nsec()),
        stamp(metadata.mtime(), metadata.mtime_nsec()),
    ];
    let path =
        std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(|_| Error::Io {
            path: destination.to_path_buf(),
            source: std::io::Error::from(std::io::ErrorKind::InvalidInput),
        })?;
    // SAFETY: the path is a NUL-terminated C string that outlives the call, and the
    // array holds the two values `utimensat` reads.
    let answer = unsafe {
        libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), libc::AT_SYMLINK_NOFOLLOW)
    };
    if answer == 0 {
        return Ok(());
    }
    Err(Error::Io { path: destination.to_path_buf(), source: std::io::Error::last_os_error() })
}

/// One timestamp as the system call takes it.
///
/// The two fields are as wide as the target makes them, so the conversions are needed
/// on one target and are the same type on another.
#[allow(clippy::useless_conversion, reason = "the width of both fields is per target")]
fn stamp(seconds: i64, nanoseconds: i64) -> libc::timespec {
    // SAFETY: `timespec` is a plain C structure of integers, for which every byte
    // pattern is a value; both fields are set before it is read.
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    time.tv_sec = libc::time_t::try_from(seconds).unwrap_or_default();
    time.tv_nsec = nanoseconds.try_into().unwrap_or_default();
    time
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    use super::{permissions, times};

    #[test]
    fn a_copy_keeps_the_mode_and_the_modification_time() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        std::fs::write(&source, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&destination, "#!/bin/sh\n").unwrap();
        let metadata = std::fs::symlink_metadata(&source).unwrap();
        permissions(&destination, &metadata).unwrap();
        times(&destination, &metadata).unwrap();
        let copied = std::fs::symlink_metadata(&destination).unwrap();
        assert_eq!(copied.permissions().mode() & 0o777, 0o755);
        assert_eq!(
            (copied.mtime(), copied.mtime_nsec()),
            (metadata.mtime(), metadata.mtime_nsec())
        );
    }
}
