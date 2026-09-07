//! What a unit home does not receive: a table, not code.
//!
//! Two things decide it. The first is this table, which names every directory the
//! prior art in this class leaves out, and states for each one whether Nodal leaves it
//! out too. The second is the recipe's `base.exclude`, which a project adds on top.
//!
//! Most rows are kept, not dropped. Tools in this class exclude generated state by
//! design and never try to keep it warm; Nodal clones that state on purpose, because a
//! copy-on-write clone of an installed dependency tree costs no blocks and saves an
//! install. So a row is dropped only for one of two reasons:
//!
//! * The content is not this project's. Nested checkouts another tool made belong to
//!   the machine, not to the tree. A project that keeps run logs of its own names that
//!   directory in `base.exclude`, which is added to this list.
//! * The content names its own absolute path, so a copy at a new path is wrong rather
//!   than only stale. A build cache of this kind must be rebuilt after a move, and a
//!   copy of it wastes the blocks and the trust.
//!
//! A row's `keep` value is the whole policy. Moving one is a one-word change, next to
//! the reason it holds.

use std::path::{Component, Path, PathBuf};

/// One directory the prior art names, and what Nodal does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// The path, relative to the project root. A row matches the directory and
    /// everything under it.
    pub path: &'static str,
    /// Whether a unit home receives it. `false` puts the row in the default list.
    pub keep: bool,
    /// Why, in the words a report uses.
    pub reason: &'static str,
}

/// Every directory this class of tool treats as generated state, and Nodal's answer for
/// each one. The rows with `keep: false` are [`Excludes::default`].
pub const ROWS: &[Row] = &[
    // Not this project's content.
    Row {
        path: ".claude/worktrees",
        keep: false,
        reason: "checkouts another tool made, which the clone would multiply",
    },
    Row { path: ".nodal", keep: false, reason: "the state of the copy it was found in" },
    Row { path: "test-results", keep: false, reason: "output of a run that did not happen here" },
    Row { path: "coverage", keep: false, reason: "output of a run that did not happen here" },
    // Content that names its own absolute path.
    Row { path: ".next/cache", keep: false, reason: "a build cache that records its own path" },
    Row { path: "__pycache__", keep: false, reason: "compiled modules that record their own path" },
    // Content Nodal clones on purpose. A copy-on-write clone of it costs no blocks.
    Row { path: "node_modules", keep: true, reason: "installed dependencies, kept warm" },
    Row { path: ".pnpm-store", keep: true, reason: "the dependency store, kept warm" },
    Row { path: ".venv", keep: true, reason: "installed dependencies, kept warm" },
    Row { path: "target", keep: true, reason: "build output, kept warm" },
    Row { path: "dist", keep: true, reason: "build output, kept warm" },
    Row { path: "build", keep: true, reason: "build output, kept warm" },
    Row { path: ".next", keep: true, reason: "build output, kept warm without its cache" },
    Row { path: ".nuxt", keep: true, reason: "build output, kept warm" },
    Row { path: ".svelte-kit", keep: true, reason: "build output, kept warm" },
    Row { path: ".turbo", keep: true, reason: "a task cache, kept warm" },
    Row { path: ".vite", keep: true, reason: "a task cache, kept warm" },
    Row { path: ".parcel-cache", keep: true, reason: "a task cache, kept warm" },
    Row { path: ".cache", keep: true, reason: "a task cache, kept warm" },
];

/// The paths a clone leaves out.
///
/// A path matches the entry itself and everything under it. Paths are relative to the
/// tree being cloned; an absolute path or one that climbs out of the tree is dropped
/// when the list is built, because it cannot name anything inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Excludes {
    /// The paths, normalised, in the order they were given.
    paths: Vec<PathBuf>,
}

impl Excludes {
    /// The list Nodal applies to every clone: the rows above that are not kept.
    #[must_use]
    pub fn default_list() -> Self {
        Self::from_paths(ROWS.iter().filter(|row| !row.keep).map(|row| Path::new(row.path)))
    }

    /// The default list with a recipe's `base.exclude` added.
    #[must_use]
    pub fn with_recipe(recipe: &[PathBuf]) -> Self {
        let mut list = Self::default_list();
        list.extend(recipe.iter().map(PathBuf::as_path));
        list
    }

    /// A list of exactly these paths, for a caller that states its own policy.
    #[must_use]
    pub fn from_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut list = Self::default();
        list.extend(paths);
        list
    }

    /// Add paths. A path that cannot name anything inside the tree is dropped.
    fn extend<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>) {
        for path in paths {
            if let Some(path) = normalise(path)
                && !self.paths.contains(&path)
            {
                self.paths.push(path);
            }
        }
    }

    /// Whether a clone leaves `relative` out. `relative` is a path inside the tree,
    /// relative to its root.
    #[must_use]
    pub fn excludes(&self, relative: &Path) -> bool {
        self.paths.iter().any(|excluded| relative.starts_with(excluded))
    }

    /// The paths, in the order they were given.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Whether the list is empty, so nothing is left out.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// `path` as a plain relative path, or `None` when it cannot name anything inside a
/// tree: an absolute path, an empty one, or one that starts by climbing out.
fn normalise(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Excludes, ROWS};

    #[test]
    fn the_default_list_is_the_rows_that_are_not_kept() {
        let list = Excludes::default_list();
        for row in ROWS {
            assert_eq!(
                !list.excludes(Path::new(row.path)),
                row.keep,
                "{} is in the wrong list",
                row.path
            );
        }
    }

    #[test]
    fn no_path_appears_in_the_table_twice() {
        let mut seen: Vec<&str> = ROWS.iter().map(|row| row.path).collect();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "a path appears twice in the table");
    }

    #[test]
    fn an_excluded_directory_takes_everything_under_it() {
        let list = Excludes::from_paths([Path::new("node_modules")]);
        assert!(list.excludes(Path::new("node_modules")));
        assert!(list.excludes(Path::new("node_modules/react/index.js")));
        assert!(!list.excludes(Path::new("node_modules_of_mine")));
        assert!(!list.excludes(Path::new("src/node_modules.js")));
    }

    #[test]
    fn a_recipe_adds_to_the_default_list() {
        let list =
            Excludes::with_recipe(&[PathBuf::from("var/run"), PathBuf::from("test-results")]);
        assert!(list.excludes(Path::new("var/run/app.sock")));
        assert!(list.excludes(Path::new(".next/cache/webpack")));
        assert_eq!(
            list.paths().iter().filter(|path| *path == Path::new("test-results")).count(),
            1,
            "a path the default list already holds is not added twice"
        );
    }

    #[test]
    fn a_path_that_cannot_name_anything_inside_the_tree_is_dropped() {
        let list = Excludes::from_paths([
            Path::new("/etc"),
            Path::new("../outside"),
            Path::new(""),
            Path::new("./inside"),
        ]);
        assert_eq!(list.paths(), [PathBuf::from("inside")]);
    }
}
