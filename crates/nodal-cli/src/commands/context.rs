//! Where a command writes the units' memories again.
//!
//! Every command that touches a unit compiles `WORKUNIT.md` for the whole project, and
//! this is the one place that decides what a failure to do so means. It means a line on
//! standard error and nothing else.
//!
//! A command's own work is done by the time this runs. A create that made a home, a
//! merge that moved a branch and a run that recorded a command all succeeded, and
//! reporting that they failed because a memory could not be written would be a lie
//! about the thing the person asked for. So the answer on standard output stays what
//! the command answers with, and what could not be compiled is said beside it.
//!
//! The project is resolved before the operation runs where the operation can remove the
//! directory the person is standing in ([`super::merge`], [`super::reclaim`]). A
//! project read afterwards from a home that is now in the trash is no project at all,
//! and the siblings would keep a ledger that still names the unit that has gone.

use std::path::Path;

use nodal_core::context;
use nodal_core::model::Project;
use nodal_core::runtime::entry;
use nodal_core::store::Store;

/// Write the memory of every unit of `project`.
pub fn refresh(store: &Store, project: &Project) {
    match context::refresh(store.conn(), project) {
        Ok(report) => context::report_notes(&report),
        Err(error) => eprintln!("nodal: context: {error}"),
    }
}

/// The same, for the project a path is in. A path in no project writes nothing.
pub fn refresh_at(store: &Store, path: &Path) {
    match context::refresh_at(store.conn(), path) {
        Ok(report) => context::report_notes(&report),
        Err(error) => eprintln!("nodal: context: {error}"),
    }
}

/// The project of the unit a command names, read before the operation runs.
///
/// The unit decides the project, because the working directory may not: a reclaim run
/// from the project root names a unit whose home is somewhere else entirely, and a
/// merge run from inside a home is run from a directory that will not exist a moment
/// later. The directory is the fallback, for a command that named no unit.
#[must_use]
pub fn project_of(store: &Store, target: Option<&str>, cwd: &Path) -> Option<Project> {
    entry::unit_named(store.conn(), target, cwd)
        .ok()
        .and_then(|unit| entry::project_of_unit(store.conn(), &unit).ok())
        .or_else(|| entry::project_at(store.conn(), cwd).ok().flatten())
}
