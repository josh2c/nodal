//! The checks every operation makes before it creates anything.
//!
//! One rule, stated once, applied by every operation that places a directory: a home
//! never sits inside a tree Nodal already knows, and no such tree ever sits inside a
//! home. A home inside the project it was cloned from is the
//! failure that matters: the next clone copies the copy, the fingerprint of the project
//! changes because a unit is inside it, and a reclaim that removes a home removes part
//! of the user's own checkout.
//!
//! The registry is asked as well as the disk. A directory carrying `.nodal/id` is a
//! home whoever wrote it still owns, even if this machine's registry has never heard of
//! it, and a project root is a tree a person works in. Both are refused as
//! destinations and as parents.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::lifecycle::marker;
use crate::store::{environments, projects};
use crate::{Error, Result};

/// What a refused destination overlapped with, in the words the message uses.
pub const SOURCE: &str = "the tree it would be cloned from";
/// A repository a person works in directly.
pub const PROJECT: &str = "a project Nodal manages";
/// A home that belongs to some unit.
pub const HOME: &str = "another unit's home";

/// Refuse a home that overlaps the tree it comes from, a project, or another home.
///
/// `home` does not exist yet, which is the whole point of asking now; the check is made
/// on the resolved form of its nearest existing ancestor, so a symbolic link on the way
/// to it cannot hide an overlap.
///
/// # Errors
/// [`Error::InsideSource`] naming what the destination overlapped, and [`Error::Store`]
/// when the registry could not be read.
pub fn placement(conn: &Connection, home: &Path, source: &Path) -> Result<()> {
    let placed = resolve(home);
    refuse_overlap(&placed, home, source, SOURCE)?;
    for project in projects::list(conn)? {
        refuse_overlap(&placed, home, &project.root, PROJECT)?;
    }
    for environment in environments::list_all(conn)? {
        refuse_overlap(&placed, home, &environment.home, HOME)?;
    }
    refuse_marked_ancestor(&placed, home)
}

/// Refuse a destination that holds `other` or sits inside it.
fn refuse_overlap(placed: &Path, home: &Path, other: &Path, what: &'static str) -> Result<()> {
    let other = resolve(other);
    if placed.starts_with(&other) || other.starts_with(placed) {
        return Err(Error::InsideSource { home: home.to_path_buf(), tree: other, what });
    }
    Ok(())
}

/// Refuse a destination under a directory that is already some unit's home.
///
/// This is the check the registry cannot make: a home another machine's registry owns,
/// or one left by a build that is no longer installed, still carries its marker.
fn refuse_marked_ancestor(placed: &Path, home: &Path) -> Result<()> {
    for ancestor in placed.ancestors() {
        if marker::path(ancestor).is_file() {
            return Err(Error::InsideSource {
                home: home.to_path_buf(),
                tree: ancestor.to_path_buf(),
                what: HOME,
            });
        }
    }
    Ok(())
}

/// A path with every symbolic link on it resolved, as far as it exists.
///
/// `canonicalize` needs the whole path to be there, and the destination of a create is
/// exactly what is not. The longest existing prefix is resolved and the rest is put
/// back on, which is enough: a link cannot be part of a path that does not exist.
fn resolve(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut head = path;
    loop {
        if let Ok(real) = head.canonicalize() {
            return rest.iter().rev().fold(real, |path, part| path.join(part));
        }
        match (head.file_name(), head.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                head = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use tempfile::TempDir;

    use super::{HOME, SOURCE, refuse_marked_ancestor, refuse_overlap, resolve};
    use crate::lifecycle::marker;
    use crate::model::UnitId;

    #[test]
    fn a_home_inside_its_source_is_refused_and_so_is_one_that_holds_it() {
        let source = Path::new("/w/project");
        let inside = Path::new("/w/project/.nodal/e/abcd1234");
        assert!(refuse_overlap(inside, inside, source, SOURCE).is_err());
        let holding = Path::new("/w");
        assert!(refuse_overlap(holding, holding, source, SOURCE).is_err());
        let beside = Path::new("/w/homes/abcd1234");
        refuse_overlap(beside, beside, source, SOURCE).unwrap();
    }

    #[test]
    fn a_directory_under_a_marked_home_is_refused() {
        let dir = TempDir::new().unwrap();
        let unit: UnitId = "01J8Z6H0000000000000000001".parse().unwrap();
        marker::write(dir.path(), unit).unwrap();
        let under = dir.path().join("packages").join("web");
        let error = refuse_marked_ancestor(&under, &under).unwrap_err();
        assert!(error.to_string().contains(HOME), "{error}");
    }

    #[test]
    fn a_path_that_does_not_exist_yet_still_resolves_its_existing_part() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("e").join("abcd1234");
        assert!(resolve(&missing).starts_with(dir.path().canonicalize().unwrap()));
        assert!(resolve(&missing).ends_with("e/abcd1234"));
    }
}
