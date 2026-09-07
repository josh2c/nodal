//! The units of a project: the list `nodal ls` prints, and one unit in full.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{
    BranchName, EnvId, EnvState, Environment, Event, FingerprintPart, Objective, Ports,
    ProjectName, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, NONE, Table};
use crate::output::view::event;

/// How current a unit's environment is against the project it came from.
///
/// Staleness is computed when it matters and never stored, so a producer that has not
/// computed it says [`Freshness::Unknown`] rather than guessing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "parts")]
pub enum Freshness {
    /// Not computed for this answer.
    #[default]
    Unknown,
    /// Every fingerprint part matches the project.
    Fresh,
    /// These parts of the fingerprint moved under the unit.
    Stale(Vec<FingerprintPart>),
}

/// What Git says about the unit's branch, at the moment it was asked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTree {
    /// Commits the branch has that its upstream does not.
    pub ahead: u32,
    /// Commits the upstream has that the branch does not.
    pub behind: u32,
    /// Files changed and not committed, staged or not.
    pub uncommitted: u32,
}

/// A process seen running against the unit's environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Running {
    /// The command, as short as it can be while still being recognisable.
    pub command: String,
    /// The port it is listening on, when one was attributed to it.
    pub port: Option<u16>,
}

/// The unit's environment, reduced to what a list or a detail shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvLine {
    /// Identity of the environment.
    pub id: EnvId,
    /// Where it is on disk.
    pub home: PathBuf,
    /// Whether it exists, is stopped or is running.
    pub state: EnvState,
    /// Whether Nodal created the home, or adopted a checkout it must never reclaim.
    pub managed: bool,
    /// What the home occupies, when it has been measured.
    pub disk_bytes: Option<u64>,
    /// The ports allocated to it.
    pub ports: Ports,
    /// Processes attributed to it.
    pub running: Vec<Running>,
}

impl EnvLine {
    /// The parts of an environment row a list shows, with nothing measured yet.
    #[must_use]
    pub fn from_environment(environment: &Environment) -> Self {
        Self {
            id: environment.id,
            home: environment.home.clone(),
            state: environment.state,
            managed: environment.managed,
            disk_bytes: None,
            ports: environment.ports.clone(),
            running: Vec::new(),
        }
    }
}

/// One unit, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitRow {
    /// Identity of the unit.
    pub id: UnitId,
    /// The CLI handle.
    pub slug: Slug,
    /// Where the unit is in its life.
    pub status: UnitStatus,
    /// The branch it owns.
    pub branch: BranchName,
    /// What it is for, when it was stated or recovered.
    pub objective: Option<Objective>,
    /// How current its environment is.
    pub freshness: Freshness,
    /// What Git says about the branch, when it was asked.
    pub work: Option<WorkTree>,
    /// Its environment, when it has one.
    pub environment: Option<EnvLine>,
    /// When something was last seen happening in it.
    pub last_active: Option<Timestamp>,
}

impl UnitRow {
    /// A row for a unit with nothing computed about it yet.
    #[must_use]
    pub fn from_unit(unit: &Unit) -> Self {
        Self {
            id: unit.id,
            slug: unit.slug.clone(),
            status: unit.status,
            branch: unit.branch.clone(),
            objective: unit.objective.clone(),
            freshness: Freshness::Unknown,
            work: None,
            environment: None,
            last_active: None,
        }
    }
}

/// Every unit of a project: what `nodal ls`, and a bare `nodal`, answer with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitList {
    /// The project the units belong to.
    pub project: ProjectName,
    /// The instant the list was taken, which every relative time is measured from.
    pub now: Timestamp,
    /// The units, in the order the producer chose.
    pub units: Vec<UnitRow>,
}

impl Render for UnitList {
    const KIND: &'static str = "unit list";

    fn doc(&self) -> Doc {
        if self.units.is_empty() {
            return Doc::from_iter([Block::line(format!("{}: no units yet", self.project))]);
        }
        Doc::from_iter([Block::table(table(&self.units, self.now))])
    }
}

/// One unit in full: what `nodal show` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitDetail {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The unit itself.
    pub unit: UnitRow,
    /// What has happened in it, most recent last.
    pub history: Vec<Event>,
}

impl Render for UnitDetail {
    const KIND: &'static str = "unit";

    fn doc(&self) -> Doc {
        let mut doc = Doc::from_iter([Block::fields(detail_fields(&self.unit, self.now))]);
        if !self.history.is_empty() {
            doc.push(Block::blank());
            doc.push(Block::table(event::table(&self.history, self.now)));
        }
        doc
    }
}

/// The columns of the unit table, in the order `docs/scenarios.md` prints them.
const COLUMNS: [&str; 6] = ["unit", "state", "branch", "disk", "running", "last"];

/// The unit table, which both the list and the status share.
pub(crate) fn table(units: &[UnitRow], now: Timestamp) -> Table {
    let mut table = Table::new(&COLUMNS);
    for unit in units {
        table.push(vec![
            unit.slug.to_string(),
            state_cell(unit),
            branch_cell(unit),
            disk_cell(unit),
            running_cell(unit),
            last_cell(unit, now),
        ]);
    }
    table
}

/// The fields of a unit shown on its own.
fn detail_fields(unit: &UnitRow, now: Timestamp) -> Vec<Field> {
    let mut fields = vec![
        Field::new("unit", format!("{}  ({})", unit.slug, status_label(unit.status))),
        Field::new("objective", objective_cell(unit)),
        Field::new("branch", branch_cell(unit)),
        Field::new("freshness", freshness_cell(&unit.freshness)),
    ];
    if let Some(environment) = &unit.environment {
        fields.push(Field::new("home", environment.home.display().to_string()));
        fields.push(Field::new("env", environment_label(environment)));
        fields.push(Field::new("ports", ports_cell(environment)));
        fields.push(Field::new("running", running_cell(unit)));
        fields.push(Field::new("disk", disk_cell(unit)));
    }
    fields.push(Field::new("last", last_cell(unit, now)));
    fields
}

/// The name a unit's status carries in output. A table, so the words are in one place.
fn status_label(status: UnitStatus) -> &'static str {
    match status {
        UnitStatus::Open => "open",
        UnitStatus::Review => "review",
        UnitStatus::Merged => "merged",
        UnitStatus::Archived => "archived",
    }
}

/// The name an environment's state carries in output.
fn state_label(state: EnvState) -> &'static str {
    match state {
        EnvState::Absent => "absent",
        EnvState::Stopped => "stopped",
        EnvState::Running => "running",
    }
}

/// The short name a fingerprint part carries in output, which is what a person reads in
/// `stale (deps, schema)`.
fn part_label(part: FingerprintPart) -> &'static str {
    match part {
        FingerprintPart::Toolchain => "toolchain",
        FingerprintPart::Dependencies => "deps",
        FingerprintPart::Schema => "schema",
        FingerprintPart::Services => "services",
        FingerprintPart::Recipe => "recipe",
        FingerprintPart::Generated => "generated",
        FingerprintPart::Secrets => "secrets",
    }
}

/// The state cell: the unit's status, and staleness after it. Staleness is the fact a
/// person needs first, so it sits in the leading column rather than in a detail view.
fn state_cell(unit: &UnitRow) -> String {
    let status = status_label(unit.status);
    match &unit.freshness {
        Freshness::Stale(_) => format!("{status} · {}", freshness_cell(&unit.freshness)),
        Freshness::Unknown | Freshness::Fresh => status.to_owned(),
    }
}

/// Freshness on its own line.
fn freshness_cell(freshness: &Freshness) -> String {
    match freshness {
        Freshness::Unknown => String::from(NONE),
        Freshness::Fresh => String::from("fresh"),
        Freshness::Stale(parts) => {
            let named: Vec<&str> = parts.iter().copied().map(part_label).collect();
            format!("stale ({})", named.join(", "))
        }
    }
}

/// The branch, with what Git has to say about it: `+2` ahead, `-1` behind, `*3` files
/// changed and not committed.
fn branch_cell(unit: &UnitRow) -> String {
    let Some(work) = unit.work else { return unit.branch.to_string() };
    let marks = [(work.ahead, '+'), (work.behind, '-'), (work.uncommitted, '*')]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .fold(String::new(), |marks, (count, mark)| marks + &format!(" {mark}{count}"));
    format!("{}{marks}", unit.branch)
}

/// What the unit's home occupies, when it has been measured.
fn disk_cell(unit: &UnitRow) -> String {
    unit.environment
        .as_ref()
        .and_then(|environment| environment.disk_bytes)
        .map_or_else(|| String::from(NONE), human::bytes)
}

/// The processes attributed to the unit, as `next dev :41231`.
fn running_cell(unit: &UnitRow) -> String {
    let Some(environment) = &unit.environment else { return String::from(NONE) };
    let named: Vec<String> = environment
        .running
        .iter()
        .map(|process| match process.port {
            Some(port) => format!("{} :{port}", process.command),
            None => process.command.clone(),
        })
        .collect();
    human::join(&named)
}

/// The ports allocated to an environment, as `app 41230 · postgrest 54401`.
fn ports_cell(environment: &EnvLine) -> String {
    let named: Vec<String> =
        environment.ports.0.iter().map(|(name, port)| format!("{name} {port}")).collect();
    human::join(&named)
}

/// The environment's state, and whether Nodal owns the directory it is in.
fn environment_label(environment: &EnvLine) -> String {
    let owned = if environment.managed { "managed" } else { "unmanaged: never reclaimed" };
    format!("{} · {owned}", state_label(environment.state))
}

/// The objective, or the placeholder when nothing has stated one.
fn objective_cell(unit: &UnitRow) -> String {
    unit.objective.as_ref().map_or_else(|| String::from(NONE), ToString::to_string)
}

/// How long ago the unit was last active.
fn last_cell(unit: &UnitRow, now: Timestamp) -> String {
    unit.last_active.map_or_else(|| String::from(NONE), |at| human::since(now, at))
}
