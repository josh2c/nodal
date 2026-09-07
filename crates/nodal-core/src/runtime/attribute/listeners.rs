//! Ports: a granted port with something listening on it.
//!
//! The registry says which ports each home was granted; the kernel says which ports are
//! bound ([`crate::services::listeners`]). Where the two meet is a row: the unit whose
//! port it is, and the recipe's name for it.
//!
//! It is probable, and for a reason worth stating. A grant is a reservation, not an
//! enforcement: nothing stops another program on the machine from binding 41231 before
//! the unit's dev server gets to it, and the kernel tables this reads do not say which
//! process holds the socket. So the row means "the port this unit was granted is in
//! use", which is what a person needs to know, and it says probable because "by this
//! unit" is the inference, not the reading.
//!
//! A host without the kernel tables reports a note, as every signal does.

use std::collections::BTreeSet;

use crate::runtime::attribute::{Attributed, Attributor, Confidence, Kind, Reading, Scope, Source};
use crate::services::listeners;

/// The signal, over the machine's own listening ports.
#[derive(Debug, Clone, Copy, Default)]
pub struct FromPorts;

impl Attributor for FromPorts {
    fn source(&self) -> Source {
        Source::Listener
    }

    fn read(&self, scope: &Scope) -> Reading {
        match listeners::listening_ports() {
            Ok(bound) => Reading::saw(rows(&bound, scope)),
            Err(error) => Reading::nothing(Source::Listener, error.to_string()),
        }
    }
}

/// One row for every granted port that has a listener.
#[must_use]
pub fn rows(bound: &BTreeSet<u16>, scope: &Scope) -> Vec<Attributed> {
    let mut rows = Vec::new();
    for home in &scope.homes {
        for (name, port) in &home.ports.0 {
            if !bound.contains(port) {
                continue;
            }
            rows.push(Attributed {
                unit: home.unit,
                slug: home.slug.clone(),
                environment: home.environment,
                kind: Kind::Listener,
                what: name.to_string(),
                pid: None,
                port: Some(*port),
                confidence: Confidence::Probable,
                signal: Source::Listener,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeSet;

    use super::rows;
    use crate::runtime::attribute::fixture::scope;
    use crate::runtime::attribute::{Confidence, Kind};

    #[test]
    fn a_bound_grant_is_attributed_to_the_home_that_holds_it() {
        let bound = BTreeSet::from([41_231, 5_432]);
        let rows = rows(&bound, &scope());
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].slug.as_str(), "payroll-export");
        assert_eq!(rows[0].kind, Kind::Listener);
        assert_eq!(rows[0].what, "app");
        assert_eq!(rows[0].port, Some(41_231));
        assert_eq!(rows[0].confidence, Confidence::Probable);
    }

    #[test]
    fn a_grant_nothing_listens_on_is_not_a_row() {
        assert!(rows(&BTreeSet::new(), &scope()).is_empty());
    }
}
