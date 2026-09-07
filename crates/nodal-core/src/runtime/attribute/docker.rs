//! Containers: certain by label, probable by mount.
//!
//! A unit's services are containers, and a container is the thing a person is least able
//! to attribute by hand: `docker ps` shows twenty of them and none of the names says
//! which branch it belongs to. Two rules answer it.
//!
//! A container Nodal started carries [`UNIT_LABEL`] and [`ENV_LABEL`]. That is a
//! statement of identity Nodal wrote, so it is certain, exactly as `NODAL_ID` is for a
//! process.
//!
//! A container Nodal did not start — one a person ran, one a compose file brought up —
//! carries neither, and is attributed by the home it mounts. That is probable: a mount
//! is evidence and not a statement, and a container that mounts a home to read one file
//! out of it is attributed the same way as the unit's own database.
//!
//! The daemon being unreachable is the ordinary case on a fresh machine, and it is a
//! note rather than a failure ([`crate::services::docker`]). The rest of `nodal ps` is
//! unaffected by it.

use crate::model::UnitId;
use crate::runtime::attribute::{
    Attributed, Attributor, Confidence, Home, Kind, Reading, Scope, Source,
};
use crate::services::docker::{Container, Docker, ENV_LABEL, Survey, UNIT_LABEL, survey};

/// The signal, over the machine's Docker.
#[derive(Clone, Copy)]
pub struct FromContainers<'a> {
    /// The tool containers are read through.
    pub docker: &'a dyn Docker,
}

impl<'a> FromContainers<'a> {
    /// The signal over this Docker.
    #[must_use]
    pub const fn new(docker: &'a dyn Docker) -> Self {
        Self { docker }
    }
}

impl Attributor for FromContainers<'_> {
    fn source(&self) -> Source {
        Source::Docker
    }

    fn read(&self, scope: &Scope) -> Reading {
        match survey(self.docker) {
            Ok(Survey::Ran(containers)) => Reading::saw(rows(&containers, scope)),
            Ok(Survey::Unavailable { why }) => Reading::nothing(Source::Docker, why),
            // A daemon that answers and then writes something unreadable is a fault, but
            // it is still not a reason to fail the whole answer: it is one more thing
            // this signal could not do.
            Err(error) => Reading::nothing(Source::Docker, error.to_string()),
        }
    }
}

/// One row for every container that belongs to a home in `scope`.
#[must_use]
pub fn rows(containers: &[Container], scope: &Scope) -> Vec<Attributed> {
    containers.iter().filter_map(|container| row(container, scope)).collect()
}

/// The row one container makes: by its label, or failing that by what it mounts.
fn row(container: &Container, scope: &Scope) -> Option<Attributed> {
    let (home, confidence) = labelled(container, scope)
        .map_or_else(|| mounted(container, scope), |home| Some((home, Confidence::Certain)))?;
    Some(Attributed {
        unit: home.unit,
        slug: home.slug.clone(),
        environment: home.environment,
        kind: Kind::Container,
        what: container.name.clone(),
        pid: None,
        port: None,
        confidence,
        signal: Source::Docker,
    })
}

/// The home a container's labels name, when they name one this host has.
///
/// [`ENV_LABEL`] decides which materialisation; [`UNIT_LABEL`] alone still names the
/// unit, so a container labelled by an older version of Nodal is not lost.
fn labelled<'a>(container: &Container, scope: &'a Scope) -> Option<&'a Home> {
    let unit = UnitId::parse(container.label(UNIT_LABEL)?).ok()?;
    let environment = container.label(ENV_LABEL);
    scope
        .homes
        .iter()
        .find(|home| {
            home.unit == unit && environment == Some(home.environment.to_string().as_str())
        })
        .or_else(|| scope.of(unit))
}

/// The home a container mounts, when it mounts one.
fn mounted<'a>(container: &Container, scope: &'a Scope) -> Option<(&'a Home, Confidence)> {
    let home = container.mounts.iter().find_map(|mount| scope.containing(mount))?;
    Some((home, Confidence::Probable))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::rows;
    use crate::runtime::attribute::fixture::{OTHER, UNIT, scope};
    use crate::runtime::attribute::{Confidence, Kind};
    use crate::services::docker::{Container, ENV_LABEL, UNIT_LABEL};

    /// A container with the labels and mounts named.
    fn container(name: &str, labels: &[(&str, &str)], mounts: &[&str]) -> Container {
        Container {
            name: String::from(name),
            labels: labels
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect::<BTreeMap<String, String>>(),
            mounts: mounts.iter().map(PathBuf::from).collect(),
        }
    }

    #[test]
    fn a_labelled_container_is_certain() {
        let containers = [container("nodal-worker-import-db", &[(UNIT_LABEL, UNIT)], &[])];
        let rows = rows(&containers, &scope());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].slug.as_str(), "worker-import");
        assert_eq!(rows[0].confidence, Confidence::Certain);
        assert_eq!(rows[0].kind, Kind::Container);
        assert_eq!(rows[0].what, "nodal-worker-import-db");
        assert_eq!(rows[0].pid, None);
    }

    #[test]
    fn a_container_that_only_mounts_a_home_is_probable() {
        let containers =
            [container("redis", &[], &["/homes/payroll-export/tmp/redis", "/etc/localtime"])];
        let rows = rows(&containers, &scope());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].slug.as_str(), "payroll-export");
        assert_eq!(rows[0].confidence, Confidence::Probable);
    }

    #[test]
    fn a_container_of_no_unit_here_is_not_a_row() {
        let containers = [
            container("postgres", &[], &["/var/lib/postgresql"]),
            container("elsewhere", &[(UNIT_LABEL, "01ARZ3NDEKTSV4RRFFQ69G5FZZ")], &[]),
            container("nonsense", &[(UNIT_LABEL, "not-an-identifier")], &[]),
        ];
        assert!(rows(&containers, &scope()).is_empty());
    }

    #[test]
    fn the_environment_label_decides_which_materialisation() {
        let scope = scope();
        let environment = scope.of(crate::model::UnitId::parse(OTHER).unwrap()).unwrap();
        let named = environment.environment.to_string();
        let labels = [(UNIT_LABEL, OTHER), (ENV_LABEL, named.as_str())];
        let containers = [container("nodal-payroll-export-db", &labels, &[])];
        let rows = rows(&containers, &scope);
        assert_eq!(rows[0].environment, environment.environment);
        assert_eq!(rows[0].confidence, Confidence::Certain);
    }

    #[test]
    fn a_label_wins_over_a_mount_that_says_otherwise() {
        let containers = [container(
            "nodal-worker-import-db",
            &[(UNIT_LABEL, UNIT)],
            &["/homes/payroll-export"],
        )];
        let rows = rows(&containers, &scope());
        assert_eq!(rows[0].slug.as_str(), "worker-import");
        assert_eq!(rows[0].confidence, Confidence::Certain);
    }
}
