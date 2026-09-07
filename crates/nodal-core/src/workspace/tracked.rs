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
//! So this is the gate. [`refuse`] runs before a copy starts, reads the tree of the
//! commit the source is at, and returns [`Error::ExcludesTrackedPath`] naming every
//! path the list would drop. It reads and writes nothing else: `git ls-tree` is one
//! read of the object database.
//!
//! The gate is here, at the copy, and not only at the source of the list. Inference
//! also checks the tree before it proposes a row ([`crate::recipe::infer::layout`]),
//! but a recipe a person writes by hand never went through inference, and a repository
//! can start to track a directory after the recipe was written. The copy is the last
//! place that can still say no.

use std::path::{Path, PathBuf};

use super::exclude::Excludes;
use crate::git::Git;
use crate::{Error, Result};

/// The revision a copy is checked against: what the source tree is at.
const REVISION: &str = "HEAD";

/// Pathspec magic that makes Git read a path as characters and not as a pattern.
///
/// A recipe may name a directory whose name holds `*` or `?`. Without this, Git would
/// read such a row as a glob and answer about paths the list does not name.
const LITERAL: &str = ":(literal)";

/// Every path in `excludes` that `rev` tracks in `git`, in the order the list holds
/// them.
///
/// An empty list is an empty answer, and so is a revision the repository does not have:
/// a repository with no commit yet tracks nothing.
///
/// # Errors
/// [`Error::InvalidValue`] when an excluded path is not UTF-8, [`Error::Git`] when
/// `git ls-tree` failed for a reason other than an unknown revision, [`Error::GitParse`]
/// on an unreadable record.
pub fn tracked(git: &Git, rev: &str, excludes: &Excludes) -> Result<Vec<PathBuf>> {
    if excludes.is_empty() || git.rev_parse_opt(rev)?.is_none() {
        return Ok(Vec::new());
    }
    let mut pathspecs = Vec::with_capacity(excludes.paths().len());
    for path in excludes.paths() {
        let text = path.to_str().ok_or_else(|| Error::InvalidValue {
            kind: "excluded path",
            value: path.to_string_lossy().into_owned(),
        })?;
        pathspecs.push(format!("{LITERAL}{text}"));
    }
    let borrowed: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
    let entries = git.ls_tree(rev, false, &borrowed)?;
    Ok(excludes
        .paths()
        .iter()
        .filter(|path| entries.iter().any(|entry| entry.path.starts_with(path)))
        .cloned()
        .collect())
}

/// Refuse to copy `source` when `excludes` would leave a tracked path out of the copy.
///
/// A `source` that is not a Git repository tracks nothing, so the copy goes ahead. That
/// is not a hole: every tree Nodal copies from is a repository, and a directory that is
/// not one has no tracked path for a list to drop.
///
/// # Errors
/// [`Error::ExcludesTrackedPath`] naming every path the list would drop; otherwise as
/// [`tracked`].
pub fn refuse(source: &Path, excludes: &Excludes) -> Result<()> {
    let Ok(git) = Git::open(source) else {
        return Ok(());
    };
    let paths = tracked(&git, REVISION, excludes)?;
    if paths.is_empty() {
        return Ok(());
    }
    Err(Error::ExcludesTrackedPath { source_tree: source.to_owned(), paths })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{Excludes, refuse, tracked};
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
        assert_eq!(tracked(&git, "HEAD", &list).unwrap(), [PathBuf::from("test-results")]);
    }

    #[test]
    fn a_directory_the_commit_does_not_hold_is_not_named() {
        let root = repository(&["src/main.rs"]);
        std::fs::create_dir_all(root.path().join("test-results")).unwrap();
        let git = Git::open(root.path()).unwrap();
        assert!(tracked(&git, "HEAD", &Excludes::default_list()).unwrap().is_empty());
        assert!(refuse(root.path(), &Excludes::default_list()).is_ok());
    }

    #[test]
    fn a_copy_is_refused_and_the_message_names_every_tracked_path() {
        let root = repository(&["coverage/index.html", "test-results/report.xml"]);
        let refused = refuse(root.path(), &Excludes::default_list()).unwrap_err();
        let Error::ExcludesTrackedPath { paths, .. } = &refused else {
            panic!("a tracked exclude was not refused: {refused}");
        };
        assert_eq!(paths, &[PathBuf::from("test-results"), PathBuf::from("coverage")]);
        let message = refused.to_string();
        assert!(message.contains("test-results"), "{message}");
        assert!(message.contains("coverage"), "{message}");
    }

    #[test]
    fn a_recipe_row_is_checked_with_the_rest_of_the_list() {
        let root = repository(&["var/run/app.sock"]);
        let list = Excludes::with_recipe(&[PathBuf::from("var/run")]);
        let refused = refuse(root.path(), &list).unwrap_err();
        assert!(matches!(refused, Error::ExcludesTrackedPath { .. }), "{refused}");
    }

    #[test]
    fn a_directory_that_is_not_a_repository_tracks_nothing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("test-results")).unwrap();
        assert!(refuse(root.path(), &Excludes::default_list()).is_ok());
    }

    #[test]
    fn a_repository_with_no_commit_tracks_nothing() {
        let root = tempfile::tempdir().unwrap();
        run(root.path(), &["init", "--quiet", "."]);
        assert!(refuse(root.path(), &Excludes::default_list()).is_ok());
    }
}
