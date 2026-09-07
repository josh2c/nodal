//! Entering a unit: which home a target names, and how a shell is asked to go there.
//!
//! Nodal never starts a shell to put a person in a unit. A command that names
//! a directory prints the path, which is all a script needs, and writes the same path
//! into the file named by `NODAL_CD_FILE` when one is open. The shell function
//! `nodal shell-init` installs is what opens that file: it makes the file, runs the
//! command, and changes its own directory to what it finds there. A shell without the
//! function is not broken by any of this; it gets the path on standard output.
//!
//! The channel is a file rather than a line of standard output so that a command's own
//! output, `--json` included, stays exactly what it was.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::model::{Project, Slug, Unit};
use crate::store::{environments, projects, units};
use crate::{Error, Result};

/// The variable naming the file a waiting shell reads a directory from.
pub const CD_FILE_VAR: &str = "NODAL_CD_FILE";

/// Ask the shell that is waiting, if one is, to enter `home`.
///
/// Returns whether a shell was listening, so that a command can say what happened. It
/// is never an error for nothing to be listening: `nodal` is a program first.
///
/// # Errors
/// [`Error::Io`] when the file the shell named cannot be written.
pub fn ask_to_enter(home: &Path) -> Result<bool> {
    let Some(path) = std::env::var_os(CD_FILE_VAR).filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    let path = PathBuf::from(path);
    std::fs::write(&path, home.as_os_str().as_encoded_bytes()).map_err(Error::io(&path))?;
    Ok(true)
}

/// The home a target names.
///
/// Three targets, tried in this order:
///
/// - nothing: the home the working directory is in, so `nodal cd` on its own goes to
///   the top of the unit a person is already inside;
/// - a directory: the home that directory is in;
/// - anything else: a unit's slug, looked up in the registry.
///
/// The registry is passed in already open, because opening it is what runs the
/// preamble that resolves an interrupted operation, and that belongs to the command
/// (`nodal-cli`) rather than to this function.
///
/// # Errors
/// [`Error::NotAHome`] when a directory is in no home, [`Error::UnitNotFound`] when no
/// unit has that slug, [`Error::UnitAmbiguous`] when two projects have one, and
/// [`Error::UnitNotMaterialized`] when the unit has no home on any host.
pub fn home(target: Option<&str>, cwd: &Path, conn: &Connection) -> Result<PathBuf> {
    let Some(target) = target else {
        return crate::env::files::find_home(cwd);
    };
    let path = Path::new(target);
    if path.is_dir() {
        return crate::env::files::find_home(path);
    }
    home_of_unit(conn, &Slug::parse(target)?, cwd)
}

/// The home of the unit `slug` names.
///
/// The project the working directory is in is searched first, because a slug is short
/// and two projects may each have one. Only when that finds nothing is every project
/// searched, and a slug that two of them answer is a message rather than a guess.
///
/// # Errors
/// As [`home`].
pub fn home_of_unit(conn: &Connection, slug: &Slug, cwd: &Path) -> Result<PathBuf> {
    let unit = match project_at(conn, cwd)? {
        Some(project) => units::find_by_slug(conn, project.id, slug)?,
        None => None,
    };
    let unit = match unit {
        Some(unit) => unit,
        None => search_every_project(conn, slug)?,
    };
    materialized_home(conn, &unit)
}

/// The project a directory belongs to, if the registry knows one.
///
/// A directory inside a unit's home belongs to the project that unit is of. The home is
/// a clone, so its own top level is the home and not the project, and walking up from it
/// reaches Nodal's state directory rather than the repository the work came from.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read.
pub fn project_at(conn: &Connection, path: &Path) -> Result<Option<Project>> {
    if let Some(project) = project_of_home(conn, path)? {
        return Ok(Some(project));
    }
    for directory in path.ancestors() {
        if let Some(project) = projects::find_by_root(conn, directory)? {
            return Ok(Some(project));
        }
    }
    Ok(None)
}

/// The project of the unit whose home holds `path`, when `path` is in one.
fn project_of_home(conn: &Connection, path: &Path) -> Result<Option<Project>> {
    let Ok(home) = crate::env::files::find_home(path) else {
        return Ok(None);
    };
    let Some(marked) = crate::lifecycle::marker::read(&home)? else {
        return Ok(None);
    };
    let Some(unit) = units::get(conn, marked)? else {
        return Ok(None);
    };
    projects::get(conn, unit.project_id)
}

/// The one unit with this slug, over every project.
fn search_every_project(conn: &Connection, slug: &Slug) -> Result<Unit> {
    let mut found: Vec<(Project, Unit)> = Vec::new();
    for project in projects::list(conn)? {
        if let Some(unit) = units::find_by_slug(conn, project.id, slug)? {
            found.push((project, unit));
        }
    }
    match found.len() {
        0 => Err(Error::UnitNotFound { slug: slug.to_string() }),
        1 => Ok(found.remove(0).1),
        _ => Err(Error::UnitAmbiguous {
            slug: slug.to_string(),
            projects: found.iter().map(|(project, _)| project.name.to_string()).collect(),
        }),
    }
}

/// Where a unit's newest materialisation is.
fn materialized_home(conn: &Connection, unit: &Unit) -> Result<PathBuf> {
    environments::latest_for_unit(conn, unit.id)?
        .map(|environment| environment.home)
        .ok_or_else(|| Error::UnitNotMaterialized { slug: unit.slug.to_string() })
}
