//! The one rule an exclusion list may not break: a copy never drops a tracked path.
//!
//! [`super::exclude`] is a list of directories a copy leaves out, and almost all of
//! them are generated state that no commit holds. A project may add its own rows to
//! that list, and inference proposes rows from what it finds on disk. Either route can
//! name a directory the project tracks.
//!
//! A copy that is missing a tracked path is dirty the moment it is made. `git status`
//! in it reports a deletion for every file under that path, so the person who made it
//! starts with a working tree they did not change and cannot explain. One heavy
//! directory of a real project produced 99 deletions this way.
//!
//! So this is the gate. [`enforce`] runs before a copy starts and reads the tree of the
//! commit the source is at. What it does with what it finds depends on which source put
//! the row on the list, because the two are not the same claim:
//!
//! * A **default** row ([`super::exclude::ROWS`]) is Nodal's guess from a directory's
//!   name. A commit is the project's own statement about the content under that name,
//!   and it beats a guess: the row yields, the copy keeps the directory, and the
//!   operation carries one note saying which row was kept and why. Without this, a
//!   project that commits a baseline report into `test-results` could make no unit at
//!   all, and no recipe key could give it one.
//! * A **recipe** row is what the project wrote in `base.exclude`. The person asked for
//!   a copy that drops content the same project tracks, which is a mistake only they can
//!   settle, so the copy is refused with [`Error::ExcludesTrackedPath`] naming every
//!   such path.
//!
//! The gate reads and writes nothing else: `git ls-tree` is one read of the object
//! database.
//!
//! The gate is here, at the copy, and not only at the source of the list. Inference
//! also checks the tree before it proposes a row ([`crate::recipe::infer::layout`]),
//! but a recipe a person writes by hand never went through inference, and a repository
//! can start to track a directory after the recipe was written. The copy is the last
//! place that can still say no.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::exclude::{Excluded, Excludes, Origin, reason_for};
use crate::git::Git;
use crate::{Error, Result};

/// The revision a copy is checked against: what the source tree is at.
const REVISION: &str = "HEAD";

/// Pathspec magic that makes Git read a path as characters and not as a pattern.
///
/// A recipe may name a directory whose name holds `*` or `?`. Without this, Git would
/// read such a row as a glob and answer about paths the list does not name.
const LITERAL: &str = ":(literal)";

/// A default row a copy kept because the source tree tracks what it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// The path the row names.
    pub path: PathBuf,
    /// Why the default list holds the row, in the words the table uses.
    pub reason: String,
}

impl std::fmt::Display for Kept {
    /// The note a report prints: the row, and why it yielded.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "{path} is excluded by default ({reason}), but the commit tracks it, so the copy keeps it",
            path = self.path.display(),
            reason = self.reason
        )
    }
}

/// Every row of `excludes` that `rev` tracks in `git`, in the order the list holds them.
///
/// An empty list is an empty answer, and so is a revision the repository does not have:
/// a repository with no commit yet tracks nothing.
///
/// # Errors
/// [`Error::InvalidValue`] when an excluded path is not UTF-8, [`Error::Git`] when
/// `git ls-tree` failed for a reason other than an unknown revision, [`Error::GitParse`]
/// on an unreadable record.
pub fn tracked(git: &Git, rev: &str, excludes: &Excludes) -> Result<Vec<Excluded>> {
    if excludes.is_empty() || git.rev_parse_opt(rev)?.is_none() {
        return Ok(Vec::new());
    }
    let mut pathspecs = Vec::with_capacity(excludes.rows().len());
    for row in excludes.rows() {
        let text = row.path.to_str().ok_or_else(|| Error::InvalidValue {
            kind: "excluded path",
            value: row.path.to_string_lossy().into_owned(),
        })?;
        pathspecs.push(format!("{LITERAL}{text}"));
    }
    let borrowed: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
    let entries = git.ls_tree(rev, false, &borrowed)?;
    Ok(excludes
        .rows()
        .iter()
        .filter(|row| entries.iter().any(|entry| entry.path.starts_with(&row.path)))
        .cloned()
        .collect())
}

/// Settle `excludes` against what `source` tracks, before a copy is made from it.
///
/// A recipe row the tree tracks refuses the copy. A default row the tree tracks is
/// taken off `excludes`, so the copy keeps the directory, and is answered here for the
/// operation to report.
///
/// A `source` that is not a Git repository tracks nothing, so the copy goes ahead with
/// the list as given. That is not a hole: every tree Nodal copies from is a repository,
/// and a directory that is not one has no tracked path for a list to drop.
///
/// # Errors
/// [`Error::ExcludesTrackedPath`] naming every path a recipe row would drop; otherwise
/// as [`tracked`].
pub fn enforce(source: &Path, excludes: &mut Excludes) -> Result<Vec<Kept>> {
    let Ok(git) = Git::open(source) else {
        return Ok(Vec::new());
    };
    let rows = tracked(&git, REVISION, excludes)?;
    let asked_for: Vec<PathBuf> = rows
        .iter()
        .filter(|row| row.origin == Origin::Recipe)
        .map(|row| row.path.clone())
        .collect();
    if !asked_for.is_empty() {
        return Err(Error::ExcludesTrackedPath {
            source_tree: source.to_owned(),
            paths: asked_for,
        });
    }
    let mut kept = Vec::with_capacity(rows.len());
    for row in rows {
        excludes.keep(&row.path);
        let reason = reason_for(&row.path).unwrap_or("a default row of the exclusion table");
        kept.push(Kept { path: row.path, reason: String::from(reason) });
    }
    Ok(kept)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{Excludes, Origin, enforce, tracked};
    use crate::Error;
    use crate::git::Git;

    /// A repository that tracks `paths`, each one a file with a line in it.
    fn repository(paths: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        run(root.path(), &["init", "--quiet", "."]);
        for path in paths {
            let file = root.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "content\n").unwrap();
        }
        run(root.path(), &["add", "--all"]);
        run(
            root.path(),
            &[
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "user.name=test",
                "commit",
                "--quiet",
                "--message=fixture",
            ],
        );
        root
    }

    fn run(dir: &Path, args: &[&str]) {
        let status = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn a_tracked_directory_in_the_list_is_named() {
        let root = repository(&["src/main.rs", "test-results/report.xml"]);
        let git = Git::open(root.path()).unwrap();
        let list = Excludes::default_list();
        let rows = tracked(&git, "HEAD", &list).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].path, PathBuf::from("test-results"));
        assert_eq!(rows[0].origin, Origin::Default);
    }

    #[test]
    fn a_directory_the_commit_does_not_hold_is_not_named() {
        let root = repository(&["src/main.rs"]);
        std::fs::create_dir_all(root.path().join("test-results")).unwrap();
        let git = Git::open(root.path()).unwrap();
        assert!(tracked(&git, "HEAD", &Excludes::default_list()).unwrap().is_empty());
        let mut list = Excludes::default_list();
        assert_eq!(enforce(root.path(), &mut list).unwrap(), []);
        assert!(list.excludes(Path::new("test-results")), "the row is still on the list");
    }

    #[test]
    fn every_tracked_default_row_yields_and_is_named_in_the_notes() {
        let root = repository(&["coverage/index.html", "test-results/report.xml"]);
        let mut list = Excludes::default_list();
        let kept = enforce(root.path(), &mut list).unwrap();

        let paths: Vec<&Path> = kept.iter().map(|one| one.path.as_path()).collect();
        assert_eq!(paths, [Path::new("test-results"), Path::new("coverage")]);
        assert!(!list.excludes(Path::new("coverage/index.html")), "a yielded row still excludes");
        assert!(!list.excludes(Path::new("test-results/report.xml")), "so does the other one");
        assert!(list.excludes(Path::new(".nodal")), "the rest of the table was dropped");

        let note = kept[0].to_string();
        assert!(note.contains("test-results"), "the note does not name the row: {note}");
        assert!(
            note.contains("output of a run that did not happen here"),
            "the note does not say why the row is on the list: {note}"
        );
    }

    #[test]
    fn a_recipe_row_is_refused_and_the_message_names_every_tracked_path() {
        let root = repository(&["coverage/index.html", "var/run/app.sock"]);
        let mut list = Excludes::with_recipe(&[PathBuf::from("var/run")]);
        let refused = enforce(root.path(), &mut list).unwrap_err();
        let Error::ExcludesTrackedPath { paths, .. } = &refused else {
            panic!("a tracked exclude was not refused: {refused}");
        };
        assert_eq!(paths, &[PathBuf::from("var/run")], "a default row was named in a refusal");
        let message = refused.to_string();
        assert!(message.contains("var/run"), "{message}");
    }

    #[test]
    fn a_default_row_the_recipe_repeats_is_the_project_own_and_is_refused() {
        let root = repository(&["coverage/index.html"]);
        let mut list = Excludes::with_recipe(&[PathBuf::from("coverage")]);
        let refused = enforce(root.path(), &mut list).unwrap_err();
        assert!(matches!(refused, Error::ExcludesTrackedPath { .. }), "{refused}");
    }

    #[test]
    fn a_directory_that_is_not_a_repository_tracks_nothing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("test-results")).unwrap();
        assert_eq!(enforce(root.path(), &mut Excludes::default_list()).unwrap(), []);
    }

    #[test]
    fn a_repository_with_no_commit_tracks_nothing() {
        let root = tempfile::tempdir().unwrap();
        run(root.path(), &["init", "--quiet", "."]);
        assert_eq!(enforce(root.path(), &mut Excludes::default_list()).unwrap(), []);
    }
}
