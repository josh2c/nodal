//! A project holding more open units than a person can hold in their head.
//!
//! Every other row of a report is something a tool left behind. This one is about a
//! project that is working exactly as it should and has simply accumulated: twelve open
//! units, each with a home on the disk, most of them from work that finished weeks ago.
//! The units are Nodal's own, so nothing here is unmanaged; the row is a count and the
//! disk that count costs.
//!
//! The threshold is [`LIMIT`]. Under it there is no row at all, because a project with
//! three open units is not a thing to report.

use rusqlite::Connection;

use crate::doctor::{Scope, Section, size};
use crate::model::UnitStatus;
use crate::output::view::doctor::{Finding, Kind};
use crate::store::{environments, projects, units};

/// How many open units a project holds before doctor says so.
pub const LIMIT: usize = 10;

/// Every project over [`LIMIT`], with the disk its homes hold.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read.
pub fn find(conn: &Connection, scope: &Scope) -> crate::Result<Vec<(Section, Finding)>> {
    let mut rows = Vec::new();
    for project in projects::list(conn)? {
        let open = units::list_by_status(conn, project.id, UnitStatus::Open)?;
        if open.len() <= LIMIT {
            continue;
        }
        let mut bytes = 0;
        let mut complete = true;
        for unit in &open {
            for environment in environments::list_for_unit(conn, unit.id)? {
                let measured = size::measure(&environment.home);
                bytes += measured.bytes;
                complete &= measured.complete;
            }
        }
        rows.push((
            scope.section(&project.root),
            Finding::new(Kind::UnitCount, project.name.to_string())
                .sized(bytes, complete)
                .says(format!("{} open units, over the threshold of {LIMIT}", open.len())),
        ));
    }
    Ok(rows)
}
