//! What `nodal new` answers with: the unit it made, and where.

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::env::files;
use crate::model::{Environment, Manifest, Missing, Timestamp, Unit};
use crate::output::Render;
use crate::output::human::{Block, Doc, Field, NONE};
use crate::output::view::unit::{self, EnvLine, UnitRow};
use crate::workspace::tracked::Kept;

/// A unit that has just been created or adopted, with the home it was given.
///
/// The objective is on the report because of adoption: a unit made from a checkout an
/// agent left behind has an objective nobody typed, recovered from that session's
/// opening prompt, and the moment to show a person what was recovered is the moment it
/// was.
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
    /// Every default exclusion row the copy kept because the project tracks the path it
    /// names. Empty on all but the rare project that commits into such a directory, and
    /// one line each when it does, because the home is then larger than the table says.
    #[serde(default)]
    pub kept: Vec<Kept>,
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
        Self { now, unit: row, missing: manifest.missing.clone(), kept: Vec::new() }
    }

    /// The same report, with the default exclusion rows the copy kept named on it.
    #[must_use]
    pub fn keeping(mut self, kept: Vec<Kept>) -> Self {
        self.kept = kept;
        self
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
        if self.unit.objective.is_some() {
            fields.push(Field::new("for", unit::objective_cell(&self.unit)));
        }
        if !self.missing.is_empty() {
            fields.push(Field::new("no value", missing_cell(&self.missing)));
        }
        if !self.kept.is_empty() {
            fields.push(Field::new("kept", kept_cell(&self.kept)));
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

/// One line per default exclusion row that yielded, saying which and why.
fn kept_cell(kept: &[Kept]) -> String {
    kept.iter().map(Kept::to_string).collect::<Vec<String>>().join("\n")
}
