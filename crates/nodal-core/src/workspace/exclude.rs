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
//!   copy of it wastes the blocks and the trust. This list is read from the root of the
//!   tree, so it reaches such a directory where it sits at the root and no further;
//!   [`super::relocate`] is what finds the rest, once the copy is made.
//!
//! The word a row is written with is the whole policy: `kept`, `dropped`, and
//! `dropped_everywhere` where no copy of the directory is usable at any path. Moving a
//! row is a one-word change, next to the reason it holds.
//!
//! Every row on the list carries where it came from ([`Origin`]), because the two
//! sources answer differently when the project tracks the path. A row of this table is
//! a guess about a name, and a commit is a fact about the content, so a default row
//! yields: the copy keeps the directory and says so. A row the project wrote is an
//! instruction, so it is refused aloud instead. [`super::tracked`] is where that
//! happens.

use std::path::{Component, Path, PathBuf};

/// One directory the prior art names, and what Nodal does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// The path, relative to the project root. A row matches the directory and
    /// everything under it.
    pub path: &'static str,
    /// Whether a unit home receives it. `false` puts the row in the default list.
    pub keep: bool,
    /// Whether a copy of it at another path must be removed rather than trusted.
    ///
    /// This list is anchored at the root of the tree, so it reaches the copy of such a
    /// directory that sits at the root and no other. A row marked here is also what
    /// [`super::relocate`] looks for anywhere under a home, which is how the copy under
    /// the second package of a repository, and the one Python writes beside every
    /// source file, are found.
    pub invalidate: bool,
    /// Why, in the words a report uses.
    pub reason: &'static str,
}

/// A row a unit home receives.
const fn kept(path: &'static str, reason: &'static str) -> Row {
    Row { path, keep: true, invalidate: false, reason }
}

/// A row a clone leaves out. The copy at the root of the tree is the one it reaches.
const fn dropped(path: &'static str, reason: &'static str) -> Row {
    Row { path, keep: false, invalidate: false, reason }
}

/// A row a clone leaves out and a home removes wherever under it the directory sits.
const fn dropped_everywhere(path: &'static str, reason: &'static str) -> Row {
    Row { path, keep: false, invalidate: true, reason }
}

/// Every directory this class of tool treats as generated state, and Nodal's answer for
/// each one. The rows that are not [`kept`] are [`Excludes::default`].
pub const ROWS: &[Row] = &[
    // Not this project's content.
    dropped(".claude/worktrees", "checkouts another tool made, which the clone would multiply"),
    dropped(".nodal", "the state of the copy it was found in"),
    dropped("test-results", "output of a run that did not happen here"),
    dropped("coverage", "output of a run that did not happen here"),
    // Content that names its own absolute path, so no copy of it is usable anywhere.
    dropped_everywhere(".next/cache", "a build cache that records its own path"),
    dropped_everywhere("__pycache__", "compiled modules that record their own path"),
    // Content Nodal clones on purpose. A copy-on-write clone of it costs no blocks.
    //
    // Some of it records absolute paths too: a Cargo `target` directory holds the path
    // it was built at, and a build at a new path starts cold. That is stale, not wrong.
    // The tool reads its own record, sees the change and rebuilds, and the copy cost no
    // blocks to carry, so the row is kept whole.
    kept("node_modules", "installed dependencies, kept warm"),
    kept(".pnpm-store", "the dependency store, kept warm"),
    kept(".venv", "installed dependencies, kept warm"),
    kept("target", "build output, kept warm"),
    kept("dist", "build output, kept warm"),
    kept("build", "build output, kept warm"),
    kept(".next", "build output, kept warm without its cache"),
    kept(".nuxt", "build output, kept warm"),
    kept(".svelte-kit", "build output, kept warm"),
    kept(".turbo", "a task cache, kept warm"),
    kept(".vite", "a task cache, kept warm"),
    kept(".parcel-cache", "a task cache, kept warm"),
    kept(".cache", "a task cache, kept warm"),
];

/// The rows a home must not keep at a path other than the one they were made at.
///
/// [`super::relocate`] is what acts on them, after a clone rather than during one.
#[must_use]
pub fn invalidated() -> Vec<&'static Row> {
    ROWS.iter().filter(|row| row.invalidate).collect()
}

/// The rows whose content a tool makes again: build output, installed dependencies and
/// task caches.
///
/// This is the table read a third time, for the one place where the answer inverts.
/// A live home keeps this content because a warm build is what the home is for; a
/// reclaimed home in the trash has no build to keep warm, and the same rows are then
/// thirteen gigabytes a person is storing for a fortnight to no purpose.
/// [`super::prune`] is what acts on it.
///
/// The rule is the two words the table already carries, and it is derived rather than
/// written a second time so that a row cannot be regenerable here and not there. A
/// [`kept`] row is kept precisely because the tool that wrote it can write it again,
/// and a [`dropped_everywhere`] row is a cache the tool rebuilds after a move. What is
/// left out is the other reason a row is dropped: content that is not this project's.
/// A nested checkout another tool made, a report of a run that happened elsewhere and
/// the state of the copy a home was found in are none of them things a build remakes,
/// so a prune must not be the operation that takes one away.
#[must_use]
pub fn regenerable() -> Vec<&'static Row> {
    ROWS.iter().filter(|row| row.keep || row.invalidate).collect()
}

/// Where a row on an exclusion list came from.
///
/// This decides what a tracked path does to the row. A default row is Nodal's guess
/// from a directory's name; a recipe row is what the project asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A row of [`ROWS`] that is not [`kept`].
    Default,
    /// A row the project wrote in `base.exclude`, or a caller stated itself.
    Recipe,
}

/// One row of an exclusion list: the path, and where the row came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excluded {
    /// The path, normalised, relative to the root of the tree.
    pub path: PathBuf,
    /// Which source put it on the list.
    pub origin: Origin,
}

/// The reason [`ROWS`] gives for dropping `path`, when the table holds it.
#[must_use]
pub fn reason_for(path: &Path) -> Option<&'static str> {
    ROWS.iter().find(|row| !row.keep && Path::new(row.path) == path).map(|row| row.reason)
}

/// The paths a clone leaves out.
///
/// A path matches the entry itself and everything under it. Paths are relative to the
/// tree being cloned; an absolute path or one that climbs out of the tree is dropped
/// when the list is built, because it cannot name anything inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Excludes {
    /// The rows, normalised, in the order they were given.
    rows: Vec<Excluded>,
}

impl Excludes {
    /// The list Nodal applies to every clone: the rows above that are not kept.
    #[must_use]
    pub fn default_list() -> Self {
        let mut list = Self::default();
        list.extend(
            ROWS.iter().filter(|row| !row.keep).map(|row| Path::new(row.path)),
            Origin::Default,
        );
        list
    }

    /// The default list with a recipe's `base.exclude` added.
    #[must_use]
    pub fn with_recipe(recipe: &[PathBuf]) -> Self {
        let mut list = Self::default_list();
        list.extend(recipe.iter().map(PathBuf::as_path), Origin::Recipe);
        list
    }

    /// A list of exactly these paths, for a caller that states its own policy.
    ///
    /// Such a caller owns the list it gives, so every row of it is [`Origin::Recipe`].
    #[must_use]
    pub fn from_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut list = Self::default();
        list.extend(paths, Origin::Recipe);
        list
    }

    /// Add paths from one source. A path that cannot name anything inside the tree is
    /// dropped.
    ///
    /// A path the list already holds is not added twice. It takes the new origin when
    /// that origin is [`Origin::Recipe`]: a project that writes a row Nodal also ships
    /// has asked for it, and asking is what the loud answer is for.
    fn extend<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>, origin: Origin) {
        for path in paths {
            let Some(path) = normalise(path) else { continue };
            if let Some(held) = self.rows.iter_mut().find(|row| row.path == path) {
                if origin == Origin::Recipe {
                    held.origin = Origin::Recipe;
                }
            } else {
                self.rows.push(Excluded { path, origin });
            }
        }
    }

    /// Whether a clone leaves `relative` out. `relative` is a path inside the tree,
    /// relative to its root.
    #[must_use]
    pub fn excludes(&self, relative: &Path) -> bool {
        self.rows.iter().any(|row| relative.starts_with(&row.path))
    }

    /// The rows, in the order they were given.
    #[must_use]
    pub fn rows(&self) -> &[Excluded] {
        &self.rows
    }

    /// Take `path` off the list, so a copy keeps it after all.
    ///
    /// [`super::tracked`] is the only caller: a default row the project tracks yields
    /// to the commit.
    pub fn keep(&mut self, path: &Path) {
        self.rows.retain(|row| row.path != path);
    }

    /// Whether the list is empty, so nothing is left out.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// `path` as a plain relative path, or `None` when it cannot name anything inside a
/// tree: an absolute path, an empty one, or one that starts by climbing out.
pub(super) fn normalise(path: &Path) -> Option<PathBuf> {
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
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Excludes, Origin, ROWS};

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
    fn a_row_a_home_must_not_keep_is_also_a_row_a_clone_leaves_out() {
        let list = Excludes::default_list();
        for row in super::invalidated() {
            assert!(
                !row.keep && list.excludes(Path::new(row.path)),
                "{} is invalidated after a clone but carried by one",
                row.path
            );
        }
    }

    #[test]
    fn what_a_tool_makes_again_is_every_row_but_the_content_that_is_not_ours() {
        let regenerable: Vec<&str> = super::regenerable().iter().map(|row| row.path).collect();
        for path in ["target", "node_modules", ".venv", ".turbo", ".next", "__pycache__"] {
            assert!(regenerable.contains(&path), "{path} is state a tool writes again");
        }
        for path in [".claude/worktrees", ".nodal", "test-results", "coverage"] {
            assert!(!regenerable.contains(&path), "{path} is not this project's to remake");
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
            list.rows().iter().filter(|row| row.path == Path::new("test-results")).count(),
            1,
            "a path the default list already holds is not added twice"
        );
    }

    #[test]
    fn a_row_carries_the_source_that_put_it_on_the_list() {
        let list = Excludes::with_recipe(&[PathBuf::from("var/run")]);
        let origin = |path: &str| {
            list.rows().iter().find(|row| row.path == Path::new(path)).map(|row| row.origin)
        };
        assert_eq!(origin("coverage"), Some(Origin::Default));
        assert_eq!(origin("var/run"), Some(Origin::Recipe));
    }

    #[test]
    fn a_recipe_row_the_table_also_holds_becomes_the_project_own() {
        let list = Excludes::with_recipe(&[PathBuf::from("coverage")]);
        let row = list
            .rows()
            .iter()
            .find(|row| row.path == Path::new("coverage"))
            .expect("the row is on the list");
        assert_eq!(row.origin, Origin::Recipe, "the project asked for a row the table also holds");
    }

    #[test]
    fn a_row_taken_off_the_list_stops_excluding_what_it_named() {
        let mut list = Excludes::default_list();
        assert!(list.excludes(Path::new("coverage/index.html")));
        list.keep(Path::new("coverage"));
        assert!(!list.excludes(Path::new("coverage/index.html")));
        assert!(list.excludes(Path::new("test-results/report.xml")), "the rest of the list holds");
    }

    #[test]
    fn a_path_that_cannot_name_anything_inside_the_tree_is_dropped() {
        let list = Excludes::from_paths([
            Path::new("/etc"),
            Path::new("../outside"),
            Path::new(""),
            Path::new("./inside"),
        ]);
        assert_eq!(
            list.rows().iter().map(|row| row.path.clone()).collect::<Vec<PathBuf>>(),
            [PathBuf::from("inside")]
        );
    }
}
