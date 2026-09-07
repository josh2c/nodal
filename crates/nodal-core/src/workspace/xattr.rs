//! Extended attributes, copied from one path to another.
//!
//! A tree can carry meaning outside its bytes. Quarantine flags on macOS, capability
//! and security labels on Linux, and per-file notes some build tools write are all
//! extended attributes, and a copy that drops them is a copy that behaves differently
//! from the tree it came from.
//!
//! Links are not handled here. Linux refuses a `user.*` attribute on a symbolic link,
//! so a copy of one would fail on the only attributes a project writes; callers copy
//! attributes for files and directories.

use std::path::Path;

use crate::error::Result;

/// Copy every extended attribute of `source` onto `destination`, and report how many.
///
/// A filesystem that holds no extended attributes is not a failure: the source has
/// none to copy, and the answer is `0`.
///
/// # Errors
/// [`Error::Io`] naming the path whose attributes could not be read or written.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn copy(source: &Path, destination: &Path) -> Result<usize> {
    let mut copied = 0;
    for name in names(source)? {
        let Some(value) = read(source, &name)? else { continue };
        write(destination, &name, &value)?;
        copied += 1;
    }
    Ok(copied)
}

/// Copy every extended attribute of `source` onto `destination`, and report how many.
///
/// This platform has no extended attributes, so the answer is always `0`.
///
/// # Errors
/// None on this platform.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn copy(_source: &Path, _destination: &Path) -> Result<usize> {
    Ok(0)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use std::ffi::{CString, OsStr, OsString};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::Path;

    use crate::error::{Error, Result};

    /// A path as the C string the system calls take.
    ///
    /// # Errors
    /// [`Error::Io`] when the path holds a NUL byte, which no filesystem accepts.
    fn c_path(path: &Path) -> Result<CString> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::Io {
            path: path.to_path_buf(),
            source: std::io::Error::from(std::io::ErrorKind::InvalidInput),
        })
    }

    /// An attribute name as the C string the system calls take.
    ///
    /// # Errors
    /// [`Error::Io`] when the name holds a NUL byte.
    fn c_name(path: &Path, name: &OsStr) -> Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| Error::Io {
            path: path.to_path_buf(),
            source: std::io::Error::from(std::io::ErrorKind::InvalidInput),
        })
    }

    /// Whether the last error says there is nothing to read: a filesystem that holds
    /// no extended attributes, or an attribute that went away between two calls.
    fn unsupported(error: &std::io::Error) -> bool {
        #[cfg(target_os = "linux")]
        let empty = [libc::ENOTSUP, libc::ENODATA];
        #[cfg(target_os = "macos")]
        let empty = [libc::ENOTSUP, libc::ENOATTR];
        error.raw_os_error().is_some_and(|code| empty.contains(&code))
    }

    /// The names of every extended attribute of `path`, which are NUL separated.
    ///
    /// # Errors
    /// [`Error::Io`] when the names could not be listed.
    pub(super) fn names(path: &Path) -> Result<Vec<OsString>> {
        let c_path = c_path(path)?;
        let size = call(path, || unsafe { list(c_path.as_ptr(), std::ptr::null_mut(), 0) })?;
        let Some(size) = size else { return Ok(Vec::new()) };
        let mut buffer = vec![0_u8; size];
        let read = call(path, || unsafe {
            list(c_path.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len())
        })?;
        let Some(read) = read else { return Ok(Vec::new()) };
        buffer.truncate(read);
        Ok(buffer
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(|name| OsString::from_vec(name.to_vec()))
            .collect())
    }

    /// The value of one attribute, or `None` when it went away between the two calls.
    ///
    /// # Errors
    /// [`Error::Io`] when the value could not be read.
    pub(super) fn read(path: &Path, name: &OsStr) -> Result<Option<Vec<u8>>> {
        let (c_path, c_name) = (c_path(path)?, c_name(path, name)?);
        let size = call(path, || unsafe {
            get(c_path.as_ptr(), c_name.as_ptr(), std::ptr::null_mut(), 0)
        })?;
        let Some(size) = size else { return Ok(None) };
        let mut buffer = vec![0_u8; size];
        let read = call(path, || unsafe {
            get(c_path.as_ptr(), c_name.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len())
        })?;
        let Some(read) = read else { return Ok(None) };
        buffer.truncate(read);
        Ok(Some(buffer))
    }

    /// Put one attribute on `path`.
    ///
    /// # Errors
    /// [`Error::Io`] when the attribute could not be written.
    pub(super) fn write(path: &Path, name: &OsStr, value: &[u8]) -> Result<()> {
        let (c_path, c_name) = (c_path(path)?, c_name(path, name)?);
        let written =
            unsafe { set(c_path.as_ptr(), c_name.as_ptr(), value.as_ptr().cast(), value.len()) };
        if written == 0 {
            return Ok(());
        }
        Err(Error::Io { path: path.to_path_buf(), source: std::io::Error::last_os_error() })
    }

    /// Run one size-or-value call, and read a filesystem without extended attributes
    /// as an empty answer rather than a failure.
    fn call(path: &Path, run: impl FnOnce() -> isize) -> Result<Option<usize>> {
        let answer = run();
        if answer >= 0 {
            return Ok(Some(usize::try_from(answer).unwrap_or(0)));
        }
        let error = std::io::Error::last_os_error();
        if unsupported(&error) {
            return Ok(None);
        }
        Err(Error::Io { path: path.to_path_buf(), source: error })
    }

    /// List the attribute names of a path, without following a link.
    #[cfg(target_os = "linux")]
    unsafe fn list(path: *const libc::c_char, buffer: *mut libc::c_char, size: usize) -> isize {
        unsafe { libc::llistxattr(path, buffer, size) }
    }

    /// Read one attribute value, without following a link.
    #[cfg(target_os = "linux")]
    unsafe fn get(
        path: *const libc::c_char,
        name: *const libc::c_char,
        buffer: *mut libc::c_void,
        size: usize,
    ) -> isize {
        unsafe { libc::lgetxattr(path, name, buffer, size) }
    }

    /// Write one attribute value, without following a link.
    #[cfg(target_os = "linux")]
    unsafe fn set(
        path: *const libc::c_char,
        name: *const libc::c_char,
        value: *const libc::c_void,
        size: usize,
    ) -> libc::c_int {
        unsafe { libc::lsetxattr(path, name, value, size, 0) }
    }

    /// List the attribute names of a path, without following a link.
    #[cfg(target_os = "macos")]
    unsafe fn list(path: *const libc::c_char, buffer: *mut libc::c_char, size: usize) -> isize {
        unsafe { libc::listxattr(path, buffer, size, libc::XATTR_NOFOLLOW) }
    }

    /// Read one attribute value, without following a link.
    #[cfg(target_os = "macos")]
    unsafe fn get(
        path: *const libc::c_char,
        name: *const libc::c_char,
        buffer: *mut libc::c_void,
        size: usize,
    ) -> isize {
        unsafe { libc::getxattr(path, name, buffer, size, 0, libc::XATTR_NOFOLLOW) }
    }

    /// Write one attribute value, without following a link.
    #[cfg(target_os = "macos")]
    unsafe fn set(
        path: *const libc::c_char,
        name: *const libc::c_char,
        value: *const libc::c_void,
        size: usize,
    ) -> libc::c_int {
        unsafe { libc::setxattr(path, name, value, size, 0, libc::XATTR_NOFOLLOW) }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix::{names, read, write};

#[cfg(test)]
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::ffi::OsStr;

    use super::{copy, unix};

    /// Whether this filesystem takes a `user` attribute at all, so a test on a
    /// filesystem without them reports nothing rather than failing.
    fn holds_attributes(path: &std::path::Path) -> bool {
        unix::write(path, OsStr::new(NAME), b"probe").is_ok()
    }

    /// The attribute the tests write. `user.` is the namespace an unprivileged process
    /// may write on Linux; macOS has no namespace rule.
    const NAME: &str = "user.nodal.test";

    #[test]
    fn an_attribute_survives_a_copy() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        std::fs::write(&source, "content").unwrap();
        std::fs::write(&destination, "content").unwrap();
        if !holds_attributes(&source) {
            return;
        }
        unix::write(&source, OsStr::new(NAME), b"kept").unwrap();
        assert_eq!(copy(&source, &destination).unwrap(), 1);
        assert_eq!(
            unix::read(&destination, OsStr::new(NAME)).unwrap().as_deref(),
            Some(&b"kept"[..])
        );
    }

    #[test]
    fn a_file_without_attributes_copies_none() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        std::fs::write(&source, "content").unwrap();
        std::fs::write(&destination, "content").unwrap();
        assert_eq!(copy(&source, &destination).unwrap(), 0);
    }
}
