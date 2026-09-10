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
//!
//! [`copy`] is the other direction over the same shape: a second copy of a tree, made
//! from its content rather than from its metadata, for a test that needs two of
//! something the fixture only writes once.

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

/// Copy everything under `from` into `to`, content for content.
///
/// A test that needs a second copy of the fixture project uses this rather than
/// [`std::fs::copy`]. That call carries the source's mode, and the fixture plants the
/// shape a real base has: a file at mode `0444` (`nodal_fixture::read_only`), and Git
/// writes every loose object read-only besides. A tree copied that way is one a test
/// cannot go on writing in.
///
/// What is copied is what a test is asking for: the shape of the tree and the bytes in
/// it. Directories are made writable, files are written fresh, and a symbolic link is
/// made again as a link rather than followed. No mode and no attribute of the source
/// crosses over, which is the whole reason this can be relied on.
///
/// A path that goes between the listing and the read is left out rather than raised
/// ([`gone_or`]). A tree holding a `.git` is a tree something else may be writing.
///
/// # Panics
///
/// Naming the first path that could not be read or written, which is a machine a
/// property cannot be asserted on rather than a difference.
pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    copy_into(from.as_ref(), to.as_ref());
}

/// Copy one directory into another, and everything under it.
fn copy_into(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|error| panic!("{}: {error}", to.display()));
    let read =
        std::fs::read_dir(from).unwrap_or_else(|error| panic!("{}: {error}", from.display()));
    for entry in read {
        let entry = entry.unwrap_or_else(|error| panic!("{}: {error}", from.display()));
        let path = entry.path();
        let target = to.join(entry.file_name());
        let Some(kind) = gone_or(&path, entry.file_type()) else { continue };
        if kind.is_symlink() {
            let Some(points_at) = gone_or(&path, std::fs::read_link(&path)) else { continue };
            link(&points_at, &target);
        } else if kind.is_dir() {
            copy_into(&path, &target);
        } else {
            let Some(content) = gone_or(&path, std::fs::read(&path)) else { continue };
            std::fs::write(&target, content)
                .unwrap_or_else(|error| panic!("{}: {error}", target.display()));
        }
    }
}

/// What was read, or nothing when the path went between the listing and the read.
///
/// A directory is listed before it is read, and a repository is a directory something
/// else may be writing: Git's own background pass writes a lock file in `.git/objects`
/// and removes it again, and so does a `nodal` command running in the same tree. A file
/// that is no longer there is not part of the tree a test asked to copy. Every other
/// failure is a machine a property cannot be asserted on, and is raised.
///
/// # Panics
///
/// Naming the path, for any failure but the path having gone.
fn gone_or<T>(path: &Path, read: std::io::Result<T>) -> Option<T> {
    match read {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("{}: {error}", path.display()),
    }
}

/// Make a symbolic link at `at` pointing where the one it was copied from points.
#[cfg(unix)]
fn link(points_at: &Path, at: &Path) {
    std::os::unix::fs::symlink(points_at, at)
        .unwrap_or_else(|error| panic!("{}: {error}", at.display()));
}

/// A host where a test tree holds no links.
#[cfg(not(unix))]
fn link(points_at: &Path, at: &Path) {
    let _ = (points_at, at);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a fixture that cannot be built fails the test")]

    use std::path::Path;

    use nodal_fixture::read_only;
    use tempfile::TempDir;

    use super::{Snapshot, copy};

    /// The crossing a real base has, and the one the standard copy refuses on macOS: a
    /// file at mode `0444` carrying an extended attribute. A copier a test relies on has
    /// to take it on every runner, or the suite is asserting a property on one platform
    /// and a copy failure on the other.
    #[test]
    fn a_tree_holding_a_read_only_file_with_an_attribute_is_copied_whole() {
        let root = TempDir::new().unwrap();
        let from = root.path().join("project");
        let from = nodal_fixture::write(&from);
        assert!(from.join(read_only::LOCKED).is_file(), "the fixture planted no locked file");

        let to = root.path().join("copy");
        copy(&from, &to);

        let source = Snapshot::of(&from);
        assert!(source.len() > 1, "the fixture wrote nothing to copy");
        assert_eq!(
            std::fs::read_to_string(to.join(read_only::LOCKED)).unwrap(),
            read_only::LOCKED_CONTENTS,
            "the file the platform refuses to copy did not arrive"
        );
    }

    /// A repository is a directory something else may be writing. What has gone by the
    /// time the copier reaches it is left out; anything else is the machine failing and
    /// is raised.
    #[test]
    fn a_path_that_went_between_the_listing_and_the_read_is_left_out() {
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        assert!(super::gone_or(Path::new("/nowhere"), Err::<(), _>(missing)).is_none());
        assert_eq!(super::gone_or(Path::new("/nowhere"), Ok(7)), Some(7));
    }

    /// The copy is content and shape, never the source's mode. A tree copied out of a
    /// base has to be one a test can go on writing in.
    #[test]
    fn what_is_copied_is_writable_however_locked_the_original_was() {
        let root = TempDir::new().unwrap();
        let from = root.path().join("project");
        let from = nodal_fixture::write(&from);

        let to = root.path().join("copy");
        copy(&from, &to);

        let copied = to.join(read_only::LOCKED);
        std::fs::write(&copied, "written again\n")
            .expect("the copy carried the mode that denies every write");
    }
}
