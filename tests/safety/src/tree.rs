//! What a directory holds, byte for byte, so that "nothing changed" is a fact and not a
//! reading of a status line.
//!
//! Two properties need this. A base has to be the same tree after ten units are cloned
//! from it as it was before the first, and a read command has to leave every tree it
//! reported on exactly as it found it. `git status` cannot state either one: it says
//! nothing about a file no commit tracks, and nothing at all about a directory that is
//! not a repository.
//!
//! So a snapshot holds the content itself. The trees here are the fixture project and a
//! clone of it, which is under a hundred small files, and holding the bytes is what
//! makes the comparison mean what it says.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One thing found in a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    /// A file: its permission bits and its content.
    File(u32, Vec<u8>),
    /// A symbolic link and where it points, which is read rather than followed.
    Link(PathBuf),
    /// A directory, which is held so that an emptied one is still a difference.
    Directory,
}

impl Entry {
    /// What this entry is, for a message about a difference.
    const fn kind(&self) -> &'static str {
        match self {
            Self::File(..) => "file",
            Self::Link(_) => "link",
            Self::Directory => "directory",
        }
    }
}

/// Everything under a directory, keyed by the path relative to its root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The root the paths are relative to, for a message.
    root: PathBuf,
    /// What was found, in path order.
    entries: BTreeMap<PathBuf, Entry>,
}

impl Snapshot {
    /// Everything under `root`.
    ///
    /// # Panics
    ///
    /// If a directory or a file under it could not be read, which is not a difference
    /// but a machine a property cannot be asserted on.
    #[must_use]
    pub fn of(root: impl AsRef<Path>) -> Self {
        Self::of_except(root, |_| false)
    }

    /// The same, without the paths `skip` answers `true` for.
    ///
    /// A skipped directory is not descended into. The caller says what it left out and
    /// why, in the test, so that a snapshot never quietly covers less than it reads as.
    ///
    /// # Panics
    ///
    /// As [`Snapshot::of`].
    #[must_use]
    pub fn of_except(root: impl AsRef<Path>, skip: impl Fn(&Path) -> bool) -> Self {
        let root = root.as_ref().to_path_buf();
        let mut entries = BTreeMap::new();
        walk(&root, &root, &skip, &mut entries);
        Self { root, entries }
    }

    /// How many things it holds, so that a test can say the snapshot was not empty.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// One line for every path the two disagree about, in path order.
    ///
    /// The content itself is never in a line. A difference names the path and says what
    /// kind of difference it is, because a test that printed a lockfile would drown the
    /// one line that mattered.
    #[must_use]
    pub fn differences(&self, later: &Self) -> Vec<String> {
        let mut lines = Vec::new();
        for (path, before) in &self.entries {
            match later.entries.get(path) {
                None => lines.push(format!("removed: {}", path.display())),
                Some(after) if after == before => {}
                Some(after) => lines.push(difference(path, before, after)),
            }
        }
        for path in later.entries.keys() {
            if !self.entries.contains_key(path) {
                lines.push(format!("added: {}", path.display()));
            }
        }
        lines.sort();
        lines
    }

    /// Insist that nothing under the root changed, and say what did when something did.
    ///
    /// # Panics
    ///
    /// Naming every path the two snapshots disagree about.
    pub fn assert_unchanged(&self, later: &Self, claim: &str) {
        let differences = self.differences(later);
        assert!(
            differences.is_empty(),
            "{claim}: {} changed under {}:\n  {}",
            differences.len(),
            self.root.display(),
            differences.join("\n  ")
        );
    }
}

/// One line about one path the two snapshots disagree about.
fn difference(path: &Path, before: &Entry, after: &Entry) -> String {
    let where_ = path.display();
    match (before, after) {
        (Entry::File(mode, content), Entry::File(later_mode, later_content)) => {
            if content == later_content {
                format!("mode {mode:o} became {later_mode:o}: {where_}")
            } else {
                format!(
                    "content changed from {} to {} bytes: {where_}",
                    content.len(),
                    later_content.len()
                )
            }
        }
        (Entry::Link(target), Entry::Link(later)) => {
            format!(
                "link now points at {} rather than {}: {where_}",
                later.display(),
                target.display()
            )
        }
        (before, after) => {
            format!("{} became a {}: {where_}", before.kind(), after.kind())
        }
    }
}

/// Read one directory into `entries`, and everything under it.
fn walk(
    root: &Path,
    directory: &Path,
    skip: &impl Fn(&Path) -> bool,
    entries: &mut BTreeMap<PathBuf, Entry>,
) {
    let read = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
    for entry in read {
        let entry = entry.unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if skip(&relative) {
            continue;
        }
        let kind = entry.file_type().unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        if kind.is_symlink() {
            let target = std::fs::read_link(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            entries.insert(relative, Entry::Link(target));
        } else if kind.is_dir() {
            entries.insert(relative, Entry::Directory);
            walk(root, &path, skip, entries);
        } else {
            let content =
                std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            entries.insert(relative, Entry::File(mode_of(&path), content));
        }
    }
}

/// The permission bits of a path, so that a file made runnable is a difference.
#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .permissions()
        .mode()
}

/// A host with no permission bits to read, where content alone is the answer.
#[cfg(not(unix))]
const fn mode_of(_path: &Path) -> u32 {
    0
}
