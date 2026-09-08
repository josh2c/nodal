//! The content a project holds that nothing may write, which is most of a base.
//!
//! A base is not a directory of ordinary files. Git writes every loose object and every
//! pack file with mode `0444`, a package store and a vendored dependency tree are
//! commonly written into a directory with mode `0555`, and macOS puts an extended
//! attribute of its own on effectively every file it touches. A copy of a base is a
//! copy of all of that.
//!
//! The fixture used to carry only half of the shape. Its files were read-only, because
//! git makes them so on every platform, and CI therefore believed it was testing the
//! condition a real machine has. It was not: nothing on a Linux runner puts an extended
//! attribute on a file, and the two together are what fails. An attribute is written to
//! a file through its own mode, and a mode of `0444` grants no write to anybody, its
//! owner included; a file that is only read-only, or only carries an attribute, is
//! copied without complaint.
//!
//! So the fixture states the crossing rather than hoping for it. [`plant`] puts a
//! read-only file that carries an extended attribute into every project the fixture
//! writes, and [`lock`] and [`mark`] are the two halves for a test that needs to build
//! the shape somewhere else — in a base, which is where a real one is met.

use std::path::Path;

/// The read-only file every fixture carries, relative to its root.
///
/// It sits under a vendored dependency directory because that is where a real project
/// has such a file: content a package manager wrote and nothing is meant to edit.
pub const LOCKED: &str = "vendor/store/immutable.txt";

/// What is in it. Nothing reads the bytes; the mode and the attribute are the point.
pub const LOCKED_CONTENTS: &str = "written once and never again\n";

/// The attribute the fixture writes, and the reason it exists.
///
/// The name is the fixture's own. macOS writes `com.apple.provenance` on its own files
/// and a Linux runner has no equivalent, so a fixture that waited for the platform to
/// supply one would carry the condition on one machine and not the other.
pub const ATTRIBUTE: &str = "user.nodal.fixture.provenance";

/// Its value.
pub const ATTRIBUTE_VALUE: &[u8] = b"a tool put this here";

/// Put the real condition into the project at `root`: a file that denies every write
/// and carries an extended attribute.
///
/// Planting is idempotent. The file is opened before it is rewritten, so writing the
/// fixture twice into one directory leaves the same project.
///
/// The directory holding it is left writable. A read-only directory is the other half
/// of the shape and it is not planted here, because a test that never removes its own
/// temporary directory cleanly is a test that leaves the disk dirtier every run;
/// [`lock`] is how a test that means to remove it asks for one.
///
/// A filesystem without extended attributes is not a failure: the file is still
/// read-only, and a caller that needs to know asks [`marked`] and says what it could
/// not prove.
pub fn plant(root: &Path) {
    let path = root.join(LOCKED);
    if let Some(parent) = path.parent() {
        drop(std::fs::create_dir_all(parent));
    }
    open(&path);
    drop(std::fs::write(&path, LOCKED_CONTENTS));
    // A filesystem that holds no attributes leaves the file read-only and nothing else,
    // which is what `marked` reports and what a test on such a machine says out loud.
    let _carries_one = mark(&path);
    lock(&path);
}

/// Put the fixture's extended attribute on one path, and report whether it went on.
///
/// The path is opened first and left as it was found, because the attribute cannot be
/// written through a mode that denies a write, and a caller that marks a file already
/// read-only means to mark it rather than to open it.
#[must_use]
pub fn mark(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return false };
    let mode = mode(&metadata);
    open(path);
    let marked = platform::set(path, ATTRIBUTE, ATTRIBUTE_VALUE);
    set_mode(path, mode);
    marked
}

/// Whether one path carries the fixture's extended attribute.
#[must_use]
pub fn marked(path: &Path) -> bool {
    platform::get(path, ATTRIBUTE).as_deref() == Some(ATTRIBUTE_VALUE)
}

/// Take every write off one path: mode `0444` for a file, `0555` for a directory.
///
/// This is what git does to an object and what a package manager does to a store. A
/// caller that locks a directory has to remove the tree itself afterwards, or the
/// temporary directory it is in cannot be removed at all.
pub fn lock(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return };
    let keep = if metadata.is_dir() { 0o555 } else { 0o444 };
    set_mode(path, mode(&metadata) & !0o222 & keep);
}

/// Put the write permission back, so the path can be changed or removed.
pub fn open(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return };
    set_mode(path, mode(&metadata) | 0o200 | if metadata.is_dir() { 0o100 } else { 0 });
}

/// Mark every loose git object and pack file under `root` the way macOS marks a file,
/// and report how many were marked.
///
/// This is the condition a real Mac fails on, in the place it fails: git has already
/// written every one of these files `0444`, and the platform has already put an
/// attribute on each. A Linux runner supplies the first half and never the second, so
/// a test that wants the real shape in a real base asks for it here.
#[must_use]
pub fn mark_git_objects(root: &Path) -> usize {
    let objects = root.join(".git/objects");
    let mut marked = 0;
    let mut pending = vec![objects];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else { continue };
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() && mark(&path) {
                marked += 1;
            }
        }
    }
    marked
}

/// The permission bits of one entry.
fn mode(metadata: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

/// Set the permission bits of one entry, and say nothing if the platform refuses.
fn set_mode(path: &Path, bits: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        drop(std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, bits);
    }
}

/// Reading and writing one extended attribute, on the platforms that have them.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;
    use std::path::Path;

    /// Write one attribute, and report whether the filesystem took it.
    pub(super) fn set(path: &Path, name: &str, value: &[u8]) -> bool {
        let (Ok(path), Ok(name)) = (c(path.as_os_str().as_bytes()), c(name.as_bytes())) else {
            return false;
        };
        // SAFETY: both strings are NUL-terminated and outlive the call, and the value
        // is a slice of the length passed with it.
        let answer = unsafe {
            #[cfg(target_os = "linux")]
            {
                libc::lsetxattr(path.as_ptr(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0)
            }
            #[cfg(target_os = "macos")]
            {
                libc::setxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                    libc::XATTR_NOFOLLOW,
                )
            }
        };
        answer == 0
    }

    /// Read one attribute, or `None` when it is not there.
    pub(super) fn get(path: &Path, name: &str) -> Option<Vec<u8>> {
        let (path, name) = (c(path.as_os_str().as_bytes()).ok()?, c(name.as_bytes()).ok()?);
        let mut buffer = vec![0_u8; 256];
        // SAFETY: both strings are NUL-terminated and outlive the call, and the buffer
        // is at least as long as the length passed with it.
        let read = unsafe {
            #[cfg(target_os = "linux")]
            {
                libc::lgetxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            }
            #[cfg(target_os = "macos")]
            {
                libc::getxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    0,
                    libc::XATTR_NOFOLLOW,
                )
            }
        };
        let read = usize::try_from(read).ok()?;
        buffer.truncate(read);
        Some(buffer)
    }

    /// One C string, or nothing when the bytes hold a NUL.
    fn c(bytes: &[u8]) -> Result<CString, std::ffi::NulError> {
        CString::new(bytes)
    }
}

/// A platform without extended attributes takes none and reports none.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use std::path::Path;

    pub(super) fn set(_path: &Path, _name: &str, _value: &[u8]) -> bool {
        false
    }

    pub(super) fn get(_path: &Path, _name: &str) -> Option<Vec<u8>> {
        None
    }
}
