//! Keeping a home's idea of the project as new as the person's own checkout.
//!
//! A home is a clone of a base, and a base is a clone of the remote taken whenever it
//! was built. Its `origin/*` refs are frozen at that moment and nothing moves them, so
//! a list's BEHIND column was arithmetic over three-week-old data: right, and about the
//! wrong commits. One maintainer's machine printed `-0 (origin/main)` for four
//! worktrees of a checkout last touched three weeks earlier.
//!
//! What moves them is a fetch out of the checkout beside them, by path. **No network
//! call of Nodal's own**: the transport is the filesystem, the source is a directory,
//! and a project whose `origin` is unreachable refreshes exactly as well as one whose
//! `origin` answers. What the checkout last fetched is what the homes are measured
//! against, which is also the honest promise — Nodal reports what the person's own
//! repository knows, and never goes and finds out more than they have.
//!
//! The cost is one `git` process per home, and a command surveys every home of the
//! project, so the cost is the thing this module is mostly about. A [`Stamp`] is a
//! reading of the checkout's refs taken without starting a process at all, and a home
//! that already refreshed at the same stamp is skipped. In the ordinary case — several
//! `nodal` commands between two fetches — every home after the first is free.

use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::git::{Git, layout, refs};
use crate::{Error, Result};

/// The stamp file, relative to a home's **git directory** and not to the home.
///
/// Not `.nodal/`, where every other file Nodal writes into a home lives, and the reason
/// is that this one is written by a read. `nodal ls` reports on a home; it must not put
/// anything into one. A path under `.nodal/` is in the working tree, it is kept out of
/// `git status` by a line in `info/exclude`, and a home whose exclude line is missing
/// would be reported as having an untracked file that a list command had just made.
///
/// The git directory is not in the working tree at all, so there is no rule to depend
/// on: `git status` cannot see this wherever it is written, and it is removed with the
/// repository it belongs to.
const FILE: &str = "nodal/refs-stamp";

/// A reading of a checkout's refs that changes whenever any of them does.
///
/// Two numbers: the newest modification time anywhere under the checkout's `refs`, its
/// `packed-refs` and its `HEAD`, and how many entries were counted. Both are read by
/// `stat`, so taking one starts no process and reads no ref file.
///
/// The time answers writes and the count answers nothing on its own; it is there because
/// a clock that goes backwards would otherwise let an older reading look like a newer
/// one. Either number differing is a refresh, so the failure direction is a fetch that
/// was not needed rather than a stale answer.
///
/// A checkout that cannot be read at all — moved, deleted, never a repository — gives
/// [`Stamp::unreadable`], which no home ever matches and which [`ensure`] treats as
/// nothing to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// The newest modification time seen, in nanoseconds since the Unix epoch.
    newest: u128,
    /// How many files and directories were counted.
    entries: u64,
    /// Whether the checkout answered at all.
    readable: bool,
}

impl Stamp {
    /// The stamp of a checkout whose refs could not be read.
    #[must_use]
    pub const fn unreadable() -> Self {
        Self { newest: 0, entries: 0, readable: false }
    }

    /// Whether this stamp is worth acting on.
    #[must_use]
    pub const fn is_readable(&self) -> bool {
        self.readable
    }

    /// The one line a home records, which is compared as text.
    fn line(&self) -> String {
        format!("{} {}\n", self.newest, self.entries)
    }

    /// Whether `home` last refreshed at this stamp.
    ///
    /// A home with no stamp file, one whose file cannot be read, and one whose git
    /// directory cannot be found have all not. Each of those refreshes again, which is
    /// the safe direction: a fetch nobody needed costs one process, and a skip nobody
    /// earned is a stale answer.
    #[must_use]
    pub fn matches(&self, home: &Path) -> bool {
        let Some(path) = layout::dir(home).map(|directory| directory.join(FILE)) else {
            return false;
        };
        self.readable && std::fs::read_to_string(path).is_ok_and(|held| held == self.line())
    }

    /// Record this stamp in `home`'s git directory.
    ///
    /// A home whose git directory cannot be found records nothing and is not an error:
    /// it refreshes again next time, which is what a missing stamp always means.
    ///
    /// # Errors
    /// [`Error::Io`] when the directory or the file could not be written.
    pub fn write(&self, home: &Path) -> Result<()> {
        let Some(git_dir) = layout::dir(home).filter(|_| self.readable) else {
            return Ok(());
        };
        let path = git_dir.join(FILE);
        let Some(parent) = path.parent() else { return Ok(()) };
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        std::fs::write(&path, self.line()).map_err(Error::io(&path))
    }
}

/// Read a checkout's refs without starting a process.
///
/// [`Stamp::unreadable`] when the path is not a repository this can find the refs of.
/// That is not an error: a checkout a person moved or deleted is an ordinary thing, and
/// the homes it made go on being surveyed against what they already have.
#[must_use]
pub fn stamp(checkout: &Path) -> Stamp {
    let Some(git_dir) = layout::dir(checkout) else { return Stamp::unreadable() };
    let mut reading = Stamp { newest: 0, entries: 0, readable: true };
    for path in [git_dir.join("HEAD"), git_dir.join("packed-refs")] {
        see(&mut reading, &path);
    }
    walk(&mut reading, &git_dir.join("refs"));
    reading
}

/// Fold one path's modification time into a reading.
fn see(reading: &mut Stamp, path: &Path) {
    let Ok(data) = std::fs::symlink_metadata(path) else { return };
    reading.entries += 1;
    let Ok(modified) = data.modified() else { return };
    let Ok(since) = modified.duration_since(UNIX_EPOCH) else { return };
    reading.newest = reading.newest.max(since.as_nanos());
}

/// Fold a directory and everything under it into a reading.
///
/// Directories count as well as files. A loose ref is replaced by a rename, which
/// changes the modification time of the directory holding it and not of any file that
/// survives, so a walk that looked only at files would miss every update.
fn walk(reading: &mut Stamp, directory: &Path) {
    see(reading, directory);
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            walk(reading, &path);
        } else {
            see(reading, &path);
        }
    }
}

/// Bring one home's remote-tracking refs up to what the checkout last fetched.
///
/// Does nothing when the checkout could not be read, and nothing when this home already
/// refreshed at this stamp. Otherwise one `git fetch` by path, and the stamp is written
/// only after it succeeds, so a fetch that failed is tried again on the next command
/// rather than recorded as done.
///
/// The checkout's own branches come too, and not only its remote-tracking refs. A
/// project that has never been pushed has no `origin/*` at all, and it is an ordinary
/// project; the branch its units merge into is a branch in the person's own checkout, so
/// that is the copy a survey has to be able to read.
///
/// # Errors
/// [`Error::Git`] when the fetch was refused, [`Error::Io`] when the stamp could not be
/// written.
pub fn ensure(home: &Path, checkout: &Path, stamp: &Stamp) -> Result<()> {
    if !stamp.is_readable() || stamp.matches(home) {
        return Ok(());
    }
    Git::at(home).refresh_from(checkout, &[refs::MIRROR_ORIGIN, refs::MIRROR_HEADS])?;
    stamp.write(home)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{FILE, Stamp, stamp};

    /// A path that is not a repository is not an error and is not a stamp anything
    /// matches. A checkout a person deleted must not stop their homes being surveyed.
    #[test]
    fn a_checkout_that_is_not_there_reads_as_unreadable() {
        let directory = tempfile::tempdir().unwrap();
        let reading = stamp(&directory.path().join("gone"));
        assert!(!reading.is_readable());
        assert!(!reading.matches(directory.path()));
    }

    /// A home with no stamp has not refreshed, and one that has matches only that
    /// reading. A stamp that cannot be placed at all matches nothing, so the home
    /// refreshes rather than trusting a file it could not find.
    #[test]
    fn a_home_without_a_stamp_matches_nothing() {
        let home = tempfile::tempdir().unwrap();
        let reading = Stamp { newest: 7, entries: 3, readable: true };
        assert!(!reading.matches(home.path()), "nowhere to keep a stamp is not a match");

        std::fs::create_dir_all(home.path().join(".git")).unwrap();
        assert!(!reading.matches(home.path()));
        reading.write(home.path()).unwrap();
        assert!(reading.matches(home.path()));
        assert!(!Stamp { newest: 8, ..reading }.matches(home.path()));
    }

    /// The stamp goes in the git directory, never in the working tree. A read command
    /// writes this, and a read command must leave nothing a `git status` would report.
    #[test]
    fn the_stamp_is_written_outside_the_working_tree() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".git")).unwrap();
        Stamp { newest: 7, entries: 3, readable: true }.write(home.path()).unwrap();

        assert!(home.path().join(".git").join(FILE).is_file(), "it is in the git directory");
        let listed: Vec<_> = std::fs::read_dir(home.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(listed, [std::ffi::OsString::from(".git")], "and nowhere else: {listed:?}");
    }

    /// An unreadable stamp is never written, so it can never be matched later.
    #[test]
    fn an_unreadable_stamp_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".git")).unwrap();
        Stamp::unreadable().write(home.path()).unwrap();
        assert!(!home.path().join(".git").join("nodal").exists());
    }
}
