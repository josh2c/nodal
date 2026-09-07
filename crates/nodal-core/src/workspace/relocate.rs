//! What a home does about content that records the path it was made at.
//!
//! A home is a copy of a tree that was made somewhere else, and almost all of that tree
//! does not care where it is. An installed `node_modules` works at any path. A build
//! cache often does not: the tool that wrote it put absolute paths inside it, so the
//! copy at the new path describes a directory that is not there. Cargo does this in
//! `target` and Next.js does it in `.next/cache`, and a build in the copy therefore
//! starts cold whatever the copy cost to make.
//!
//! There are two answers to that, and this module is the seam between them.
//!
//! *Invalidate.* Remove the cache, and say so. Nothing then trusts it, the next build
//! writes a correct one, and the home is smaller. This is [`InvalidateCache`], the
//! default, and the only relocator V0 ships.
//!
//! *Rewrite.* Replace the recorded path inside the cache files with the new one and
//! keep the warm build. A path can only be replaced in the bytes of a file it is
//! already in when the two paths are the same length, which is why every home of a
//! project is given a path of one length (`docs/contracts.md`, home path policy). That
//! rule exists to keep this repair available; it is not evidence that the repair is
//! worth making. An experiment recovered only part of a warm build and cost time and
//! disk of its own, so a rewrite is opt-in, experimental, and not implemented here.
//! Nothing in this module claims that a warm build survives a move.
//!
//! ## Why a sweep, when the exclusion list already drops these paths
//!
//! [`super::exclude`] is anchored at the root of the tree, so its `.next/cache` row
//! names the cache of the package at the root and no other. A repository that holds
//! more than one package keeps one cache under each of them, and Python writes
//! `__pycache__` beside every source file it compiles. A relocator matches a name
//! anywhere under the home, so what a clone carried is removed before a tool can read
//! it. The two are one policy read twice: the rows [`super::exclude::invalidated`]
//! returns are the rows this module looks for.
//!
//! The sweep reads every directory under the home once. The clone that ran a moment
//! earlier created each of those directories and put every file in it, so the sweep is
//! a small part of what materialising a home already costs.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::exclude;
use crate::error::{Error, Result};

/// What [`InvalidateCache`] is called in a report and in an event.
pub const INVALIDATE: &str = "invalidate";

/// Why a path a project named is removed, in the words a report uses.
const RECIPE_REASON: &str = "named by the project as a cache that records its own path";

/// One cache name a relocator looks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The path it is known by: one segment, such as `__pycache__`, or several, such as
    /// `.next/cache`. It matches a directory under the home whose path ends with it.
    pub path: PathBuf,
    /// Why a home cannot keep it, in the words a report uses.
    pub reason: String,
}

/// One cache a relocation removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removal {
    /// Where it was, relative to the home.
    pub path: PathBuf,
    /// Why it could not be kept.
    pub reason: String,
}

/// What a relocation did to one home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Which relocator acted, as [`CacheRelocator::name`] gives it.
    pub relocator: &'static str,
    /// The path the content was made at.
    pub from: PathBuf,
    /// The path it is at now.
    pub to: PathBuf,
    /// What was removed, in the order it was found.
    pub removed: Vec<Removal>,
    /// How many directories were read to find them. The cost of the sweep, in the one
    /// unit that describes it.
    pub examined: usize,
}

impl Report {
    /// Whether the relocation left the home as it found it.
    #[must_use]
    pub const fn changed_nothing(&self) -> bool {
        self.removed.is_empty()
    }

    /// The relocation in words, for an event body and a log line.
    #[must_use]
    pub fn describe(&self) -> String {
        let (from, to) = (self.from.display(), self.to.display());
        if self.changed_nothing() {
            return format!(
                "No cache in this home records the path it was made at. The content was made at {from}. The home is at {to}."
            );
        }
        let count = self.removed.len();
        let noun = if count == 1 { "cache" } else { "caches" };
        let list = self
            .removed
            .iter()
            .map(|removal| removal.path.display().to_string())
            .collect::<Vec<String>>()
            .join(", ");
        format!(
            "Removed {count} {noun} from the new home: {list}. Each one records the path it was made at. The content was made at {from}. The home is at {to}. A cache of this kind is not correct at a new path, so the home does not keep it."
        )
    }
}

/// What a home does about content that records the path it was made at.
///
/// The trait is the seam between the operations, which are the same whichever answer is
/// in use, and the answer itself. V0 has one implementation and no way to select
/// another; the trait is what lets a rewrite be added later without an operation
/// learning that it exists.
pub trait CacheRelocator {
    /// The name of this relocator, as a report and a log line name it.
    fn name(&self) -> &'static str;

    /// Make the tree at `home` safe to use, given that its content was made at
    /// `from_path` and is now at `to_path`.
    ///
    /// `home` is the tree to act on, and for a create it is `to_path`. The three are
    /// separate arguments because an answer that repairs rather than removes needs both
    /// ends of the move, and needs their lengths, which the tree alone does not give.
    ///
    /// # Errors
    /// [`crate::Error::Io`] when a directory under `home` could not be read or removed.
    fn relocate(&self, home: &Path, from_path: &Path, to_path: &Path) -> Result<Report>;
}

/// Remove every cache that records the path it was made at, and report what went.
///
/// The names come from [`super::exclude::invalidated`] and from the paths a project
/// adds in `base.invalidate`. A cache the table does not name is left alone: a home
/// keeps its warm content, and this is the short list of what it cannot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvalidateCache {
    /// The names, in the order they are tried.
    names: Vec<Named>,
}

impl InvalidateCache {
    /// The names the default table holds.
    #[must_use]
    pub fn default_list() -> Self {
        let named = exclude::invalidated()
            .into_iter()
            .map(|row| Named { path: PathBuf::from(row.path), reason: row.reason.to_owned() });
        Self { names: named.collect() }
    }

    /// The default list with the paths a recipe names added.
    ///
    /// A path that cannot name anything inside a tree is dropped, as it is when the
    /// same kind of list decides what a clone leaves out.
    #[must_use]
    pub fn with_recipe(recipe: &[PathBuf]) -> Self {
        let mut list = Self::default_list();
        for path in recipe.iter().filter_map(|path| exclude::normalise(path)) {
            if !list.names.iter().any(|named| named.path == path) {
                list.names.push(Named { path, reason: RECIPE_REASON.to_owned() });
            }
        }
        list
    }

    /// The names it looks for, in the order they are tried.
    #[must_use]
    pub fn names(&self) -> &[Named] {
        &self.names
    }

    /// Why `relative` cannot be kept, or `None` when no name matches it.
    fn matched(&self, relative: &Path) -> Option<&str> {
        self.names
            .iter()
            .find(|named| relative.ends_with(&named.path))
            .map(|named| named.reason.as_str())
    }
}

impl CacheRelocator for InvalidateCache {
    fn name(&self) -> &'static str {
        INVALIDATE
    }

    /// One walk of the directories under `home`, parents first. A directory a name
    /// matches is removed whole and is not descended into, so a cache inside a cache is
    /// counted once.
    ///
    /// Repeatable: a second sweep of a home whose caches have gone finds nothing and
    /// removes nothing.
    fn relocate(&self, home: &Path, from_path: &Path, to_path: &Path) -> Result<Report> {
        let mut report = Report {
            relocator: self.name(),
            from: from_path.to_path_buf(),
            to: to_path.to_path_buf(),
            removed: Vec::new(),
            examined: 0,
        };
        let mut queue = vec![PathBuf::new()];
        while let Some(directory) = queue.pop() {
            for name in directories_in(&home.join(&directory))? {
                let relative = directory.join(name);
                report.examined += 1;
                if let Some(reason) = self.matched(&relative) {
                    let path = home.join(&relative);
                    std::fs::remove_dir_all(&path).map_err(Error::io(&path))?;
                    report.removed.push(Removal { path: relative, reason: reason.to_owned() });
                } else {
                    queue.push(relative);
                }
            }
        }
        Ok(report)
    }
}

/// The names of the directories in `directory`, sorted, so two sweeps of one tree find
/// the same caches in the same order.
///
/// A symbolic link to a directory is not one: following it would take the sweep out of
/// the home, and removing what it points at would take away somebody else's cache.
fn directories_in(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(Error::io(directory))? {
        let entry = entry.map_err(Error::io(directory))?;
        let kind = entry.file_type().map_err(Error::io(entry.path()))?;
        if kind.is_dir() {
            names.push(PathBuf::from(entry.file_name()));
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::{Path, PathBuf};

    use super::{CacheRelocator, INVALIDATE, InvalidateCache};

    /// A home holding one cache at the root, one under a second package, one the table
    /// does not name, and a source file beside each.
    fn home() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for directory in [".next/cache/webpack", "apps/web/.next/cache", "apps/web/.turbo", "src"] {
            std::fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        std::fs::write(root.path().join(".next/cache/webpack/0.pack"), "/old/path").unwrap();
        std::fs::write(root.path().join("apps/web/.turbo/log"), "kept").unwrap();
        std::fs::write(root.path().join("src/main.ts"), "export {};").unwrap();
        root
    }

    fn relocate(root: &Path) -> super::Report {
        InvalidateCache::default_list().relocate(root, Path::new("/base/00000001"), root).unwrap()
    }

    #[test]
    fn a_cache_that_records_its_path_goes_wherever_it_sits() {
        let root = home();
        let report = relocate(root.path());
        let mut removed: Vec<PathBuf> =
            report.removed.iter().map(|removal| removal.path.clone()).collect();
        removed.sort();
        assert_eq!(removed, [PathBuf::from(".next/cache"), "apps/web/.next/cache".into()]);
        assert!(!root.path().join(".next/cache").exists());
        assert!(!root.path().join("apps/web/.next/cache").exists());
        assert_eq!(report.relocator, INVALIDATE);
    }

    #[test]
    fn a_cache_the_table_does_not_name_is_left_alone() {
        let root = home();
        relocate(root.path());
        assert!(root.path().join("apps/web/.turbo/log").is_file());
        assert!(root.path().join(".next").is_dir(), "the build output around the cache stays");
        assert!(root.path().join("src/main.ts").is_file());
    }

    #[test]
    fn a_second_sweep_finds_nothing_and_removes_nothing() {
        let root = home();
        assert_eq!(relocate(root.path()).removed.len(), 2);
        let again = relocate(root.path());
        assert!(again.changed_nothing());
        assert!(again.describe().contains("No cache"));
    }

    #[test]
    fn a_project_adds_names_of_its_own_and_repeats_none() {
        let list = InvalidateCache::with_recipe(&[
            PathBuf::from("var/pack"),
            PathBuf::from(".next/cache"),
            PathBuf::from("/etc"),
        ]);
        let paths: Vec<&Path> = list.names().iter().map(|named| named.path.as_path()).collect();
        assert!(paths.contains(&Path::new("var/pack")));
        assert_eq!(paths.iter().filter(|path| **path == Path::new(".next/cache")).count(), 1);
        assert!(!paths.contains(&Path::new("/etc")), "a path outside the tree names nothing");
    }

    #[test]
    fn a_report_says_what_went_and_where_the_content_was_made() {
        let root = home();
        let body = relocate(root.path()).describe();
        assert!(body.contains("Removed 2 caches"), "{body}");
        assert!(body.contains(".next/cache"), "{body}");
        assert!(body.contains("/base/00000001"), "{body}");
    }

    #[test]
    fn a_link_to_a_cache_outside_the_home_is_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join(".next/cache")).unwrap();
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
        let report = relocate(root.path());
        assert!(report.changed_nothing());
        assert!(outside.path().join(".next/cache").is_dir());
    }
}
