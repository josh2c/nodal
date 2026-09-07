//! The probable signal: a process that stands in a home.
//!
//! A person opens a terminal, changes to a unit's home and starts a dev server. If the
//! shell integration is not installed and direnv has not approved the home, that process
//! carries no `NODAL_ID` at all — and it is still the unit's process, burning the unit's
//! port. Reading `/proc/<pid>/cwd` is how it is found.
//!
//! It is probable and not certain, and the difference is real: a directory says where a
//! process was started, not what it is working on. An editor opened in a home, a `tail`
//! following a log, a shell somebody left there are all attributed by this rule and none
//! of them is the unit's dev server. The row says probable so that a person reading it
//! knows the difference before acting on it.
//!
//! A process that carries `NODAL_ID` is left to [`process_env`](super::process_env): a
//! thing that has said which unit it belongs to is not guessed about.

use crate::env;
use crate::runtime::attribute::{
    Attributed, Attributor, Confidence, Reading, Scope, Source, process_row,
};
use crate::runtime::processes::Running;

/// The signal, over a process table that has already been read.
#[derive(Debug, Clone, Copy)]
pub struct FromDirectory<'a> {
    /// The processes to attribute.
    pub running: &'a [Running],
}

impl<'a> FromDirectory<'a> {
    /// The signal over this table.
    #[must_use]
    pub const fn new(running: &'a [Running]) -> Self {
        Self { running }
    }
}

impl Attributor for FromDirectory<'_> {
    fn source(&self) -> Source {
        Source::Cwd
    }

    fn read(&self, scope: &Scope) -> Reading {
        Reading::saw(rows(self.running, scope))
    }
}

/// One row for every process that stands in a home and does not name a unit itself.
#[must_use]
pub fn rows(running: &[Running], scope: &Scope) -> Vec<Attributed> {
    running.iter().filter_map(|process| row(process, scope)).collect()
}

/// The row one process makes, when it stands in a home.
fn row(process: &Running, scope: &Scope) -> Option<Attributed> {
    if process.var(env::vars::ID).is_some() {
        return None;
    }
    let home = scope.containing(process.cwd.as_ref()?)?;
    let what = process.command.clone().unwrap_or_default();
    Some(process_row(home, process.pid, what, Confidence::Probable, Source::Cwd))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeMap;

    use super::rows;
    use crate::runtime::attribute::fixture::{UNIT, scope};
    use crate::runtime::attribute::{Confidence, Source};
    use crate::runtime::processes::Running;

    /// A process that carries nothing, standing somewhere.
    fn standing(pid: u32, cwd: &str) -> Running {
        Running::new(pid, BTreeMap::new()).in_directory(cwd)
    }

    #[test]
    fn a_process_inside_a_home_is_probable() {
        let table = [standing(21, "/homes/worker-import/apps/web").running("node dev")];
        let rows = rows(&table, &scope());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].slug.as_str(), "worker-import");
        assert_eq!(rows[0].confidence, Confidence::Probable);
        assert_eq!(rows[0].signal, Source::Cwd);
    }

    #[test]
    fn a_process_outside_every_home_is_not_a_row() {
        let table = [standing(22, "/tmp"), Running::new(23, BTreeMap::new())];
        assert!(rows(&table, &scope()).is_empty());
    }

    #[test]
    fn a_process_that_names_its_unit_is_not_guessed_about() {
        let mut vars = BTreeMap::new();
        vars.insert(String::from("NODAL_ID"), String::from(UNIT));
        let table = [Running::new(24, vars).in_directory("/homes/worker-import")];
        assert!(rows(&table, &scope()).is_empty(), "one process made two rows");
    }
}
