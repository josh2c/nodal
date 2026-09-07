//! What `nodal new` answers with: the unit it made, and where.

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::env::files;
use crate::model::{Environment, Manifest, Missing, Timestamp, Unit};
use crate::output::Render;
use crate::output::human::{Block, Doc, Field, NONE};
use crate::output::view::unit::{EnvLine, UnitRow};

/// A unit that has just been created, with the home it was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Created {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The unit itself, and its one environment.
    pub unit: UnitRow,
    /// Every declared name nothing answered. Never a failure, always a line: a working
    /// copy whose mail credential is absent still runs everything that does not send
    /// mail.
    pub missing: Vec<Missing>,
}

impl Created {
    /// The report for a unit whose home is written and whose rows are committed.
    ///
    /// The missing names are read back out of the home's own manifest rather than
    /// carried here from the activation, so what is reported is what the file says.
    ///
    /// # Errors
    /// [`Error::Io`] when the manifest cannot be read, and [`Error::Recipe`] when it is
    /// not a manifest.
    pub fn of(unit: &Unit, environment: &Environment, now: Timestamp) -> Result<Self> {
        let manifest = files::read_manifest(&environment.home)?;
        Ok(Self::from_manifest(unit, environment, &manifest, now))
    }

    /// The same report, over a manifest the caller already has.
    #[must_use]
    pub fn from_manifest(
        unit: &Unit,
        environment: &Environment,
        manifest: &Manifest,
        now: Timestamp,
    ) -> Self {
        let mut row = UnitRow::from_unit(unit);
        row.environment = Some(EnvLine::from_environment(environment));
        Self { now, unit: row, missing: manifest.missing.clone() }
    }
}

impl Render for Created {
    const KIND: &'static str = "created unit";

    fn doc(&self) -> Doc {
        let mut fields = vec![
            Field::new("unit", format!("{}  ({})", self.unit.slug, self.unit.branch)),
            Field::new("home", home_cell(&self.unit)),
            Field::new("ports", ports_cell(&self.unit)),
        ];
        if !self.missing.is_empty() {
            fields.push(Field::new("no value", missing_cell(&self.missing)));
        }
        Doc::from_iter([Block::fields(fields)])
    }
}

/// Where the home is.
fn home_cell(unit: &UnitRow) -> String {
    unit.environment
        .as_ref()
        .map_or_else(|| String::from(NONE), |env| env.home.display().to_string())
}

/// The ports granted, as `name :port`, in the order the recipe named them.
fn ports_cell(unit: &UnitRow) -> String {
    let Some(environment) = &unit.environment else { return String::from(NONE) };
    if environment.ports.0.is_empty() {
        return String::from(NONE);
    }
    environment
        .ports
        .0
        .iter()
        .map(|(name, port)| format!("{name} :{port}"))
        .collect::<Vec<String>>()
        .join(" · ")
}

/// One declared name per line, so a person can see what to fill in.
fn missing_cell(missing: &[Missing]) -> String {
    missing.iter().map(|line| line.name.to_string()).collect::<Vec<String>>().join("\n")
}
