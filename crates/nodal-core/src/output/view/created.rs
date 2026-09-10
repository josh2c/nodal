//! What `nodal new` answers with: the unit it made, and where.

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::env::files;
use crate::model::{EnvName, Environment, Manifest, Missing, Origin, Timestamp, Unit};
use crate::output::Render;
use crate::output::human::{Block, Doc, Field, NONE};
use crate::output::view::unit::{self, EnvLine, UnitRow};
use crate::workspace::tracked::Kept;

/// How a unit came to be, which is the one thing its report cannot work out for itself.
///
/// A create and an adoption answer with the same value, because they produce the same
/// thing: a unit with a home. What they did to get there is different, and a person who
/// has just adopted a checkout needs to be told which of the two happened to their
/// directory — that Nodal wrote into it where it stood, or that it made a home
/// elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrival {
    /// `nodal new`: a home cloned from a base.
    Created,
    /// `nodal adopt --in-place`: the checkout stayed where it was.
    AdoptedInPlace,
    /// `nodal adopt` of a branch: a home was made for work that already had a name.
    Adopted,
}

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
    /// How the unit came to be.
    pub arrival: Arrival,
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
    /// Every generated name Nodal gave a stand-in, because no adapter answered it.
    ///
    /// The create names every one. A value that looks real and is not is worse than no
    /// value, so a person is told at the moment the unit is made
    /// ([`crate::env::stand_in`]).
    #[serde(default)]
    pub stand_ins: Vec<EnvName>,
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
    pub fn of(
        unit: &Unit,
        environment: &Environment,
        arrival: Arrival,
        now: Timestamp,
    ) -> Result<Self> {
        let manifest = files::read_manifest(&environment.home)?;
        Ok(Self::from_manifest(unit, environment, &manifest, arrival, now))
    }

    /// The same report, over a manifest the caller already has.
    #[must_use]
    pub fn from_manifest(
        unit: &Unit,
        environment: &Environment,
        manifest: &Manifest,
        arrival: Arrival,
        now: Timestamp,
    ) -> Self {
        let mut row = UnitRow::from_unit(unit);
        row.environment = Some(EnvLine::from_environment(environment));
        Self {
            now,
            arrival,
            unit: row,
            missing: manifest.missing.clone(),
            kept: Vec::new(),
            stand_ins: stand_ins_of(manifest),
        }
    }

    /// The same report, with the default exclusion rows the copy kept named on it.
    ///
    /// A builder rather than an argument, because the rows are read back out of a step's
    /// output and the report is built before that value is in hand
    /// ([`crate::lifecycle::step::Outputs`]). How the unit arrived is known where the
    /// report is made, so it stays an argument.
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
        if !self.missing.is_empty() && self.arrival == Arrival::Created {
            fields.push(Field::new("no value", missing_cell(&self.missing)));
        }
        if !self.stand_ins.is_empty() {
            fields.push(Field::new("stand-in", stand_in_cell(&self.stand_ins)));
        }
        if !self.kept.is_empty() {
            fields.push(Field::new("kept", kept_cell(&self.kept)));
        }
        let mut doc = Doc::from_iter([Block::fields(fields)]);
        // An adoption ends with a sentence, not with a column. The field above is read
        // as part of a form a person is filling in, which is right for a home that has
        // just been built and wrong for a checkout that was already theirs: the report
        // there has to say what happened to their directory, and the names that follow
        // have to be introduced or they are a list of words with no heading
        // (reported from the first day of field use).
        if let Some(summary) = self.summary() {
            doc.push(Block::blank());
            doc.push(Block::line(summary));
            for name in &self.missing {
                doc.push(Block::line(name.name.to_string()).at(2));
            }
        }
        doc
    }
}

impl Created {
    /// The line an adoption closes with, and nothing for a create.
    ///
    /// A create has no directory of the person's to report on and its own report already
    /// names every field, so a sentence under it would say twice what the fields say
    /// once.
    fn summary(&self) -> Option<String> {
        let what = match self.arrival {
            Arrival::Created => return None,
            Arrival::AdoptedInPlace => "in place",
            Arrival::Adopted => "into a home of its own",
        };
        Some(format!("adopted {} {what}; {}", self.unit.slug, self.shortfall()))
    }

    /// How many declared names this machine has no value for, in words.
    fn shortfall(&self) -> String {
        match self.missing.len() {
            0 => String::from("every declared env name has a value here"),
            1 => String::from("1 declared env name missing locally"),
            count => format!("{count} declared env names missing locally"),
        }
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

/// Every name the manifest marks as a stand-in, in the order the manifest holds them.
fn stand_ins_of(manifest: &Manifest) -> Vec<EnvName> {
    manifest
        .env
        .iter()
        .filter(|(_, origin)| **origin == Origin::StandIn)
        .map(|(name, _)| name.clone())
        .collect()
}

/// One name per line, and the sentence that says what a stand-in is.
///
/// The sentence is on the report rather than in the documentation because the person
/// reading it has just made the unit and is about to run something in it. What they
/// need to know is that the value parses and that nothing answers on it.
fn stand_in_cell(names: &[EnvName]) -> String {
    let mut lines: Vec<String> = names.iter().map(ToString::to_string).collect();
    lines.push(String::from("nodal made these values so a generate step can run"));
    lines.push(String::from("no service answers on them; an adapter replaces them"));
    lines.join("\n")
}

/// One line per default exclusion row that yielded, saying which and why.
fn kept_cell(kept: &[Kept]) -> String {
    kept.iter().map(Kept::to_string).collect::<Vec<String>>().join("\n")
}
