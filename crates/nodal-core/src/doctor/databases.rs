//! Directories named like a Nodal database that no registry row knows.
//!
//! Nodal names every database it makes, and the name starts with [`PREFIX`]. A
//! directory under Nodal's own state directory with such a name, and no row in the
//! registry naming it, is a database an interrupted run left behind or one a registry
//! that was moved no longer knows about.
//!
//! The search is under the state directory and nowhere else. A directory outside
//! Nodal's own state is not Nodal's to name, whatever it is called, so doctor does not
//! claim one.
//!
//! Doctor reports these. Dropping a database is a later command's decision, and it is
//! not one this module can be asked to make.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::doctor::{Scope, Section, size};
use crate::model::DbName;
use crate::output::view::doctor::{Finding, Kind};
use crate::store::{environments, templates};

/// The first characters of every database name Nodal makes.
pub const PREFIX: &str = "nodal_";

/// How deep under the state directory the search goes. A database sits under the
/// project segment that owns it, which is one level; the limit leaves room for one more.
const DEPTH: usize = 3;

/// Every database directory under the state directory that the registry does not name.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read.
pub fn find(conn: &Connection, scope: &Scope) -> crate::Result<Vec<(Section, Finding)>> {
    let known = named(conn, scope)?;
    let mut rows = Vec::new();
    for path in directories(&scope.state_dir) {
        let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { continue };
        if known.contains(name) {
            continue;
        }
        let measured = size::measure(&path);
        let under = path.strip_prefix(&scope.state_dir).unwrap_or(&path);
        rows.push((
            scope.section(&path),
            Finding::new(Kind::OrphanDatabase, under.display().to_string())
                .sized(measured.bytes, measured.complete)
                .says(format!("{name}, which no registry row names")),
        ));
    }
    Ok(rows)
}

/// Every database name the registry holds, from both tables that hold one.
fn named(conn: &Connection, scope: &Scope) -> crate::Result<BTreeSet<String>> {
    let mut known: BTreeSet<String> = environments::list_all(conn)?
        .into_iter()
        .filter_map(|environment| environment.db_name)
        .map(|name| name.to_string())
        .collect();
    for project in &scope.projects {
        for template in templates::list_for_project(conn, project.project.id)? {
            known.insert(template.db_name.to_string());
        }
    }
    Ok(known)
}

/// Every directory under `root`, to [`DEPTH`], whose name is a Nodal database name.
fn directories(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![(root.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { continue };
            if is_database_name(name) {
                found.push(path);
            } else if depth + 1 < DEPTH {
                queue.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found
}

/// Whether a directory name is a name Nodal gives a database.
///
/// The prefix and the shape both have to hold. The shape is [`DbName`]'s own, so a
/// directory doctor calls a database is a directory the rest of Nodal could name one.
#[must_use]
pub fn is_database_name(name: &str) -> bool {
    name.starts_with(PREFIX) && DbName::parse(name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::is_database_name;

    #[test]
    fn only_a_name_nodal_gives_a_database_counts() {
        assert!(is_database_name("nodal_storefront_01j8z6h0"));
        assert!(is_database_name("nodal_storefront_tpl_bb02"));
        assert!(!is_database_name("nodal"), "the prefix alone is not a name");
        assert!(!is_database_name("postgres"), "another tool's database is not Nodal's");
        assert!(!is_database_name("nodal_Storefront"), "an identifier holds no capital");
        assert!(!is_database_name("nodal_store-front"), "an identifier holds no dash");
    }
}
