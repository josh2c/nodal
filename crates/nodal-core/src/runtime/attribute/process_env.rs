//! The certain signal: a process that carries the unit's own variable.
//!
//! `NODAL_ID` is written by Nodal into a home's environment, and a shell that entered
//! that home passes it to everything it starts. A process carrying it is therefore in
//! that unit, and nothing else on a machine puts that variable there. This is the one
//! signal that needs no inference, and the only one that produces [`Confidence::Certain`]
//! for a process.
//!
//! `NODAL_ROOT` decides which materialisation, when a unit has more than one on this
//! host. A process whose `NODAL_ROOT` names a home the registry does not know is still
//! attributed to its unit: the variable names the unit outright, and a home that has
//! been reclaimed under a running process does not make the process anonymous.

use std::path::Path;

use crate::env;
use crate::model::UnitId;
use crate::runtime::attribute::{
    Attributed, Attributor, Confidence, Home, Reading, Scope, Source, process_row,
};
use crate::runtime::processes::Running;

/// The signal, over a process table that has already been read.
#[derive(Debug, Clone, Copy)]
pub struct FromEnvironment<'a> {
    /// The processes to attribute.
    pub running: &'a [Running],
}

impl<'a> FromEnvironment<'a> {
    /// The signal over this table.
    #[must_use]
    pub const fn new(running: &'a [Running]) -> Self {
        Self { running }
    }
}

impl Attributor for FromEnvironment<'_> {
    fn source(&self) -> Source {
        Source::Environment
    }

    fn read(&self, scope: &Scope) -> Reading {
        Reading::saw(rows(self.running, scope))
    }
}

/// One row for every process that names a unit this host has a home for.
#[must_use]
pub fn rows(running: &[Running], scope: &Scope) -> Vec<Attributed> {
    running.iter().filter_map(|process| row(process, scope)).collect()
}

/// The row one process makes, when it names a unit and that unit is here.
fn row(process: &Running, scope: &Scope) -> Option<Attributed> {
    let unit = UnitId::parse(process.var(env::vars::ID)?).ok()?;
    let home = home(process, unit, scope)?;
    let what = process.command.clone().unwrap_or_default();
    Some(process_row(home, process.pid, what, Confidence::Certain, Source::Environment))
}

/// The materialisation a process is in: the one it names, when that is one of the
/// unit's, and otherwise the unit's home on this host.
fn home<'a>(process: &Running, unit: UnitId, scope: &'a Scope) -> Option<&'a Home> {
    process
        .var(env::vars::ROOT)
        .and_then(|root| scope.at(Path::new(root)))
        .filter(|home| home.unit == unit)
        .or_else(|| scope.of(unit))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeMap;

    use super::rows;
    use crate::runtime::attribute::fixture::{OTHER, UNIT, scope};
    use crate::runtime::attribute::{Confidence, Kind};
    use crate::runtime::processes::Running;

    /// One process, with the variables named.
    fn process(pid: u32, pairs: &[(&str, &str)]) -> Running {
        let vars: BTreeMap<String, String> =
            pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
        Running::new(pid, vars)
    }

    #[test]
    fn a_process_that_carries_the_unit_is_certain() {
        let table = [process(11, &[("NODAL_ID", UNIT), ("NODAL_ROOT", "/homes/worker-import")])
            .running("node dev")];
        let rows = rows(&table, &scope());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].slug.as_str(), "worker-import");
        assert_eq!(rows[0].confidence, Confidence::Certain);
        assert_eq!(rows[0].kind, Kind::Process);
        assert_eq!(rows[0].what, "node dev");
        assert_eq!(rows[0].pid, Some(11));
    }

    #[test]
    fn a_process_of_a_unit_this_host_has_no_home_for_is_not_a_row() {
        let table = [process(12, &[("NODAL_ID", "01ARZ3NDEKTSV4RRFFQ69G5FZZ")])];
        assert!(rows(&table, &scope()).is_empty());
    }

    #[test]
    fn a_process_that_names_no_unit_or_names_nonsense_is_not_a_row() {
        let table =
            [process(13, &[("USER", "josh")]), process(14, &[("NODAL_ID", "not-an-identifier")])];
        assert!(rows(&table, &scope()).is_empty());
    }

    #[test]
    fn the_home_a_process_names_decides_which_materialisation_it_is_in() {
        // The root belongs to the other unit, so it is not this process's home; the
        // unit it names is what answers.
        let table = [process(15, &[("NODAL_ID", OTHER), ("NODAL_ROOT", "/homes/worker-import")])];
        let rows = rows(&table, &scope());
        assert_eq!(rows[0].slug.as_str(), "payroll-export");
    }

    #[test]
    fn a_process_with_no_command_still_makes_a_row() {
        let table = [process(16, &[("NODAL_ID", UNIT)])];
        let rows = rows(&table, &scope());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].what.is_empty());
    }
}
