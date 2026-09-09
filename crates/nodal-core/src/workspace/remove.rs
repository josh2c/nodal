//! Removing a tree that was made to be read.
//!
//! A home is a copy of a base, and a base holds content its tools wrote read-only. Git
//! writes every loose object and every pack file with a mode that denies all writes,
//! and a vendored dependency tree or a package store is commonly written into a
//! directory with the same rule. A copy of such a tree carries the modes it came from,
//! which is what makes the copy behave like the source; it also makes the copy
//! something the plain removal cannot take away.
//!
//! The permission that governs removing a name belongs to the directory holding it,
//! not to the file. So a read-only file is removed without ceremony, and a read-only
//! directory has to be opened first. [`tree`] opens them, deepest last, and then
//! removes.
//!
//! This is one function and not four because the trees are one kind of thing. A create
//! that fails undoes its half-made home with it, `gc` takes away a reclaimed home with
//! it, and the base store evicts a base with it. A tree any of them could not remove is
//! a directory a person is left to find and delete by hand.

use std::path::Path;

use crate::error::{Error, Result};

/// Remove a directory and everything under it, including content that denies writes.
///
/// A directory that is not there is already gone, which is not a failure: an undo runs
/// against a world it may never have changed.
///
/// The first attempt is the plain removal, so a tree that has nothing read-only in it
/// costs exactly what it did before. Only when that is refused does the tree get
/// opened, and the removal is then tried once more.
///
/// # Errors
/// [`Error::Io`] naming the path that could not be removed, when it could not be
/// removed after its directories were opened.
pub fn tree(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => unlock(path),
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// Give every directory in the tree the permissions its owner needs to read it and to
/// remove what is in it.
///
/// Only directories are touched. A file's own mode does not govern whether its name can
/// be unlinked, so changing it would be a write that buys nothing.
///
/// A symbolic link is never followed: the walk classifies with the link's own metadata,
/// so a link pointing outside the tree is removed as a name and what it points at is
/// left alone.
///
/// Nothing here reports a failure. This runs only after a removal was already refused,
/// and its whole purpose is to give the removal that follows a better chance; a
/// directory that could not be opened is reported by that removal, naming the path,
/// which is the answer a person needs.
#[cfg(unix)]
fn unlock(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    /// Read, write and enter, for the owner.
    const OWNER: u32 = 0o700;

    let Ok(metadata) = std::fs::symlink_metadata(path) else { return };
    if !metadata.is_dir() {
        return;
    }
    let mode = metadata.permissions().mode();
    if mode & OWNER != OWNER {
        let opened = std::fs::Permissions::from_mode(mode | OWNER);
        drop(std::fs::set_permissions(path, opened));
    }
    let Ok(entries) = std::fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        unlock(&entry.path());
    }
}

/// Windows has no mode to open, so the removal that was refused is the answer.
#[cfg(not(unix))]
fn unlock(_path: &Path) {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::os::unix::fs::PermissionsExt as _;

    use super::tree;

    /// Set the mode of a path.
    fn mode(path: &std::path::Path, bits: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).unwrap();
    }

    #[test]
    fn a_tree_that_denies_every_write_is_still_removed() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let store = home.join("vendor/store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join("library"), "bytes").unwrap();
        mode(&store.join("library"), 0o444);
        mode(&store, 0o555);
        mode(&home.join("vendor"), 0o555);

        assert!(std::fs::remove_dir_all(&home).is_err(), "the plain removal is refused");
        tree(&home).unwrap();
        assert!(!home.exists());
    }

    #[test]
    fn a_directory_that_cannot_even_be_entered_is_removed() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        let shut = home.join("shut");
        std::fs::create_dir_all(&shut).unwrap();
        std::fs::write(shut.join("file"), "bytes").unwrap();
        mode(&shut, 0o000);

        tree(&home).unwrap();
        assert!(!home.exists());
    }

    #[test]
    fn a_link_out_of_the_tree_is_removed_and_what_it_points_at_is_not() {
        let directory = tempfile::tempdir().unwrap();
        let outside = directory.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("kept"), "bytes").unwrap();
        mode(&outside, 0o555);

        let home = directory.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::os::unix::fs::symlink(&outside, home.join("link")).unwrap();
        mode(&home, 0o555);

        tree(&home).unwrap();
        assert!(!home.exists());
        assert!(outside.join("kept").exists(), "the tree the link pointed at is untouched");

        // The temporary directory is removed by the plain removal when this test ends,
        // so the one directory this test locked is opened again first.
        mode(&outside, 0o755);
    }

    #[test]
    fn a_tree_that_is_not_there_is_already_gone() {
        let directory = tempfile::tempdir().unwrap();
        tree(&directory.path().join("never-made")).unwrap();
    }
}
