//! The filtered walk every backend copies from.
//!
//! One walk, one exclusion filter, one order. Each backend decides only how it puts a
//! file at the other end, so the two backends cannot disagree about what a clone holds.
//!
//! The walk does not follow symbolic links: a link is an entry of its own, and a link
//! to a directory never becomes a directory in the copy. Entries arrive parents first,
//! and names within a directory are sorted, so two walks of the same tree produce the
//! same sequence and a failure is reported at the same place twice.

use std::fs::Metadata;
use std::path::{Path, PathBuf};

use super::exclude::Excludes;
use crate::error::{Error, Result};

/// What an entry is. A walk classifies once, from metadata it already read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A directory, which the copy creates before anything inside it.
    Directory,
    /// A regular file.
    File,
    /// A symbolic link, which the copy recreates rather than follows.
    Symlink,
    /// Anything else: a socket, a device, a fifo. No copy of it is meaningful.
    Other,
}

impl Kind {
    /// The kind `metadata` describes. `metadata` must come from a call that does not
    /// follow links, or a link is reported as what it points at.
    #[must_use]
    fn of(metadata: &Metadata) -> Self {
        let kind = metadata.file_type();
        if kind.is_dir() {
            Self::Directory
        } else if kind.is_file() {
            Self::File
        } else if kind.is_symlink() {
            Self::Symlink
        } else {
            Self::Other
        }
    }
}

/// One entry of a tree.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Its path relative to the root of the walk.
    pub relative: PathBuf,
    /// What it is.
    pub kind: Kind,
    /// Its metadata, read without following links.
    pub metadata: Metadata,
}

impl Entry {
    /// Where this entry is, under `root`.
    #[must_use]
    pub fn under(&self, root: &Path) -> PathBuf {
        root.join(&self.relative)
    }
}

/// What a walk left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skipped {
    /// Entries the exclusion list matched.
    pub excluded: usize,
    /// Entries that are neither a directory, a file nor a symbolic link.
    pub other: usize,
    /// Entries the directory listed that were gone by the time they were read.
    pub vanished: usize,
}

/// Every entry under `root` the exclusion list keeps, parents first.
///
/// The root itself is not an entry: a caller creates the destination and then reads
/// this sequence into it.
///
/// A tree is read while it is alive. An entry a directory listed and that is gone by
/// the time its metadata is read is an entry the tree no longer has, so it is counted
/// and left out rather than reported: a project whose own tools write and remove a
/// temporary file is still a project a unit can be made from.
///
/// # Errors
/// [`Error::Io`] naming the directory that could not be read or the entry whose
/// metadata could not be read for any reason other than its being gone.
pub fn walk(root: &Path, exclude: &Excludes) -> Result<(Vec<Entry>, Skipped)> {
    let mut entries = Vec::new();
    let mut skipped = Skipped::default();
    let mut queue = vec![PathBuf::new()];
    while let Some(directory) = queue.pop() {
        for entry in read_sorted(&root.join(&directory))? {
            let relative = directory.join(entry.file_name());
            if exclude.excludes(&relative) {
                skipped.excluded += 1;
                continue;
            }
            let path = root.join(&relative);
            let Some(metadata) = read_metadata(&path)? else {
                skipped.vanished += 1;
                continue;
            };
            let kind = Kind::of(&metadata);
            if kind == Kind::Other {
                skipped.other += 1;
                continue;
            }
            if kind == Kind::Directory {
                queue.push(relative.clone());
            }
            entries.push(Entry { relative, kind, metadata });
        }
    }
    Ok((entries, skipped))
}

/// The metadata of one entry, `None` when the entry has gone since it was listed.
fn read_metadata(path: &Path) -> Result<Option<Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// The entries of `directory`, sorted by name, so a walk has one order.
fn read_sorted(directory: &Path) -> Result<Vec<std::fs::DirEntry>> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(Error::io(directory))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::io(directory))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::{Kind, walk};
    use crate::workspace::exclude::Excludes;

    /// A tree with a directory, a file in it, and one excluded directory.
    fn tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::create_dir_all(root.path().join("test-results/run")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(root.path().join("test-results/run/report.xml"), "<r/>").unwrap();
        std::fs::write(root.path().join("README.md"), "# tree").unwrap();
        root
    }

    #[test]
    fn a_walk_reports_every_entry_the_list_keeps() {
        let root = tree();
        let (entries, skipped) = walk(root.path(), &Excludes::default_list()).unwrap();
        let mut paths: Vec<PathBuf> = entries.iter().map(|entry| entry.relative.clone()).collect();
        paths.sort();
        assert_eq!(paths, [PathBuf::from("README.md"), "src".into(), "src/main.rs".into()]);
        assert_eq!(skipped.excluded, 1, "the excluded directory is counted once");
    }

    #[test]
    fn a_parent_arrives_before_what_is_under_it() {
        let root = tree();
        let (entries, _) = walk(root.path(), &Excludes::default()).unwrap();
        let position = |path: &str| {
            entries.iter().position(|entry| entry.relative == Path::new(path)).unwrap()
        };
        assert!(position("src") < position("src/main.rs"));
        assert!(position("test-results") < position("test-results/run"));
        assert!(position("test-results/run") < position("test-results/run/report.xml"));
    }

    #[test]
    fn a_link_is_an_entry_and_is_not_followed() {
        let root = tree();
        std::os::unix::fs::symlink("src", root.path().join("link")).unwrap();
        let (entries, _) = walk(root.path(), &Excludes::default_list()).unwrap();
        let link = entries.iter().find(|entry| entry.relative == Path::new("link")).unwrap();
        assert_eq!(link.kind, Kind::Symlink);
        assert!(!entries.iter().any(|entry| entry.relative == Path::new("link/main.rs")));
    }
}
