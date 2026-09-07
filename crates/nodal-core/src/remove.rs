//! Removing a path that something else may be removing at the same moment.
//!
//! Every undo in a lifecycle operation takes something away, and the framework's rule
//! is that an undo runs against a world it may never have changed: removing what is not
//! there is a success, not a failure (`docs/code-structure.md`).
//!
//! The rule is easy to state and easy to keep only half of. A recursive removal reads a
//! directory before it unlinks what is in it, and between those two moments another
//! process can take an entry away. Git does exactly this inside a repository it owns:
//! some builds run maintenance in the background and drop a lock file under
//! `.git/objects` that they then remove. A removal that tolerates an absent path but
//! not an absent entry *under* that path fails on one machine and not on another, for
//! a reason that has nothing to do with the operation being undone.
//!
//! So absence is success wherever it is found here: at the path, and under it.

use std::io::ErrorKind;
use std::path::Path;

use crate::{Error, Result};

/// How many times a recursive removal is attempted when the walk meets an entry that
/// has already gone.
///
/// Each attempt takes away everything it can reach, so an attempt that follows one
/// which met a vanishing entry has less left to do than the one before it. A path whose
/// contents are still being taken away after this many passes is a path something else
/// owns, and that is reported rather than waited on forever.
const ATTEMPTS: usize = 8;

/// Remove a directory and everything under it.
///
/// Idempotent, and tolerant of another process removing entries at the same time: what
/// this call has to be true afterwards is that the path is not there, and an entry
/// somebody else took away is one this call does not have to take away.
///
/// # Errors
/// [`Error::Io`] when the path could not be removed for a reason other than something
/// under it having already gone.
pub fn tree(path: &Path) -> Result<()> {
    for _ in 0..ATTEMPTS {
        match std::fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                if gone(path) {
                    return Ok(());
                }
            }
            Err(error) => return Err(Error::io(path)(error)),
        }
    }
    Err(Error::io(path)(std::io::Error::new(
        ErrorKind::NotFound,
        "entries under this path kept being removed by something else",
    )))
}

/// Remove one file. Removing one that is not there is a success.
///
/// # Errors
/// [`Error::Io`] when the file is there and could not be removed.
pub fn file(path: &Path) -> Result<()> {
    forgive_absence(std::fs::remove_file(path), path)
}

/// Remove one directory that has nothing in it. Removing one that is not there is a
/// success; one that something has since put a file into is reported, because leaving
/// it is right and saying nothing about it is not.
///
/// # Errors
/// [`Error::Io`] when the directory is there and could not be removed.
pub fn directory(path: &Path) -> Result<()> {
    forgive_absence(std::fs::remove_dir(path), path)
}

/// Whether a path is not there, without following a link to decide it.
#[must_use]
pub fn gone(path: &Path) -> bool {
    path.symlink_metadata().is_err()
}

/// Turn "it was not there" into success, and name the path on anything else.
fn forgive_absence(answer: std::io::Result<()>, path: &Path) -> Result<()> {
    match answer {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tempfile::TempDir;

    use super::{directory, file, gone, tree};

    /// How many files the racing test puts in the tree it removes.
    const RACING_FILES: usize = 2_000;

    #[test]
    fn removing_what_is_not_there_is_a_success() {
        let root = TempDir::new().unwrap();
        let absent = root.path().join("never-existed");
        tree(&absent).unwrap();
        file(&absent).unwrap();
        directory(&absent).unwrap();
        assert!(gone(&absent));
    }

    #[test]
    fn removing_a_tree_takes_everything_under_it() {
        let root = TempDir::new().unwrap();
        let full = root.path().join("full");
        std::fs::create_dir_all(full.join("a").join("b")).unwrap();
        std::fs::write(full.join("a").join("b").join("c.txt"), "c").unwrap();
        tree(&full).unwrap();
        tree(&full).unwrap();
        assert!(gone(&full));
    }

    /// The failure this module exists for: something else takes entries away while the
    /// removal is walking them. A plain recursive removal reports the entry it did not
    /// find and stops; this one has to finish.
    #[test]
    fn a_tree_whose_entries_are_being_taken_away_is_still_removed() {
        let root = TempDir::new().unwrap();
        let full = root.path().join("full");
        std::fs::create_dir_all(&full).unwrap();
        for index in 0..RACING_FILES {
            std::fs::write(full.join(format!("{index}.txt")), "x").unwrap();
        }
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let (stop, target) = (&stop, full.as_path());
            scope.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    for index in (0..RACING_FILES).rev() {
                        let _ = std::fs::remove_file(target.join(format!("{index}.txt")));
                    }
                }
            });
            tree(&full).unwrap();
            stop.store(true, Ordering::Relaxed);
        });
        assert!(gone(&full));
        assert!(!Path::new(&full).exists());
    }
}
