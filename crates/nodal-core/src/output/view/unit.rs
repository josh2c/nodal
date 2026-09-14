//! The units of a project: the list `nodal ls` prints, and one unit in full.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::doctor::size::Bytes;
use crate::git::integration::{Divergence, Integration};
use crate::model::{
    ActorName, BranchName, EnvId, EnvState, Environment, Epistemic, Event, FingerprintPart,
    HostName, Lock, Needs, Objective, Ports, ProjectName, Slug, Timestamp, Unit, UnitId,
    UnitStatus, Version,
};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, NONE, Table};
use crate::output::view::event;
use crate::output::view::verdict::{RowKind, WorktreeRow};

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

/// How a branch stands against its upstream on a remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    /// The upstream ref, as Git names it.
    pub upstream: String,
    /// Commits to push, and commits to pull.
    pub divergence: Divergence,
}

/// What Git says about the unit's branch, at the moment it was asked.
///
/// Two branches are compared with, and they answer different questions. The branch the
/// work merges into says whether the work is done and whether it would conflict. The
/// upstream on the remote says whether the work is anywhere but this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTree {
    /// Paths changed in the working tree and not staged.
    pub dirty: u32,
    /// Paths staged and not committed.
    pub staged: u32,
    /// Paths Git does not track and no ignore rule covers.
    pub untracked: u32,
    /// Whether HEAD names a commit rather than a branch.
    pub detached: bool,
    /// The revision the work merges into, as it was named.
    pub base: String,
    /// How far the branch has moved from that revision.
    pub main: Divergence,
    /// What merging the branch into that revision would do.
    pub integration: Integration,
    /// How the branch stands against its upstream, when it has one.
    pub remote: Option<Remote>,
}

impl WorkTree {
    /// Paths that carry work a commit would capture.
    #[must_use]
    pub const fn uncommitted(&self) -> u32 {
        self.dirty + self.staged + self.untracked
    }
}

/// The actors attached to a unit's home right now, counted by the tool each one is.
///
/// A tool is what the attribution signals name it (`crate::runtime::actor`): an agent by
/// its own variable, a person by the account the process runs as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSessions {
    /// What the tool is called.
    pub tool: ActorName,
    /// How many of its processes are in the unit's home.
    pub count: u32,
}

/// What became of the process that took a hold.
///
/// A lock row is a record and a record outlives the process that wrote it. The row says
/// who holds the unit; this says whether the process that took it is still there, so
/// that a report never prints "holds" about a session that ended. Nothing here releases
/// a hold: a hold lapses on its own two clocks and `--take` moves it, and neither of
/// those is a reading (`docs/contracts.md`, Locks).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HolderState {
    /// The process that took the hold is in this host's process table.
    Live,
    /// It is not there. The hold stands until it lapses, and the work in the home is
    /// whatever that session left.
    Gone,
    /// Nothing here can say, and [`Unknowable`] says what stopped the reading. This is
    /// not "gone": a reading that could not be taken proves nothing.
    Unknown {
        /// What stopped the reading.
        why: Unknowable,
    },
}

/// Why liveness could not be read for a hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unknowable {
    /// The hold was taken on another machine, where a process identifier of this one
    /// means nothing.
    AnotherHost,
    /// The row records no process. A row written before locks carried one, or a hold
    /// taken by something that did not say.
    NoPid,
    /// This host publishes no process table this account may read.
    NoProcessTable,
}

impl Unknowable {
    /// Why the reading could not be taken, in one clause.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::AnotherHost => "the hold was taken on another machine",
            Self::NoPid => "the lock row records no process",
            Self::NoProcessTable => "this host has no process table to read",
        }
    }
}

impl HolderState {
    /// The word a report puts between the actor and the clock.
    ///
    /// "holds" is said for [`HolderState::Live`] and for nothing else, which is the
    /// whole of this type: a session that was killed must not read as one at work.
    #[must_use]
    pub const fn verb(&self) -> &'static str {
        match self {
            Self::Live => "holds",
            Self::Gone => "gone,",
            Self::Unknown { .. } => "held,",
        }
    }
}

/// The actor holding the write on a unit, as a report shows it.
///
/// This is the first half of WHO, and it is read from the registry rather than from the
/// process table. The process table does not cross Linux accounts, so on a host two
/// engineers share it cannot see the other person at all. A lock row can.
///
/// The process table is read for one thing and one thing only: whether the process the
/// row names is still there ([`HolderState`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    /// Who holds it. A row that records no actor holds nobody, so it never becomes a
    /// holder and this is never absent.
    pub actor: ActorName,
    /// The host the hold was taken from.
    pub host: HostName,
    /// The process that took it. Recorded so a person can look; nothing signals it.
    pub pid: Option<u32>,
    /// Whether that process is still on this host, when this host can say.
    ///
    /// The identifier is carried once, above, rather than again inside the state: two
    /// fields for one process are two fields that can disagree.
    #[serde(flatten)]
    pub state: HolderState,
    /// When the hold began.
    pub taken_at: Timestamp,
    /// When an entry last touched the home, which the idle window runs from.
    pub refreshed_at: Timestamp,
    /// When the hold lapses: the earlier of the absolute expiry and the end of the idle
    /// window, because either one releases it.
    pub expires_at: Timestamp,
}

impl Holder {
    /// The holder of a lock, with the project's idle window applied to its clock.
    ///
    /// `None` for a lock that holds nobody: a row written before locks carried an actor
    /// names a host rather than a writer, and a report that printed it as a holder would
    /// be naming somebody the registry never recorded.
    ///
    /// `state` is read by the producer ([`crate::runtime::lock::liveness`]), because a
    /// view holds what was read and takes no reading of its own.
    #[must_use]
    pub fn from_lock(lock: &Lock, idle_hours: u32, state: HolderState) -> Option<Self> {
        let idle =
            Timestamp::from_unix_seconds(lock.idle_deadline(idle_hours)).unwrap_or(lock.expires_at);
        Some(Self {
            actor: lock.actor.as_ref()?.name.clone(),
            host: lock.host.clone(),
            pid: lock.pid,
            state,
            taken_at: lock.taken_at,
            refreshed_at: lock.refreshed_at,
            expires_at: lock.expires_at.min(idle),
        })
    }
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
    /// What the home occupies, or why nothing measured it.
    pub disk: Disk,
    /// The ports allocated to it.
    pub ports: Ports,
    /// Processes attributed to it.
    pub running: Vec<Running>,
    /// The Nodal that made this home, as its own manifest records it. `None` when the
    /// manifest cannot be read, which is what a home made before homes recorded one,
    /// and a home whose manifest has gone, both look like.
    #[serde(default)]
    pub made_by: Option<Version>,
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
            disk: Disk::unmeasured(environment.state),
            ports: environment.ports.clone(),
            running: Vec::new(),
            made_by: made_by(&environment.home),
        }
    }
}

/// The version of Nodal a home's own manifest says made it.
///
/// One small read of one file per row, which is the same cost as the `stat` calls a
/// list already makes of every home, and no process. A home with no readable manifest
/// answers nothing rather than answering a guess.
fn made_by(home: &std::path::Path) -> Option<Version> {
    crate::env::files::read_manifest(home).ok().map(|manifest| manifest.binary_version)
}

/// What a home occupies, or why the figure is not there.
///
/// A byte count of a home costs a walk of it, and the list is the command with a startup
/// budget, so the list does not take one. What it must not do is print an empty column
/// that reads as "nothing": an absent figure carries the reason it is absent, and
/// `nodal show` takes the walk.
///
/// The figure itself is [`Bytes`], which is the type `nodal reclaim --check` reports, so
/// the three readings are the same kind of claim in the same words. They are not the same
/// number and are not meant to be: this is the whole home, and a preflight's groups are
/// the paths a reclaim has an opinion about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Disk {
    /// What a walk of the home found.
    Measured {
        /// The figure, with what is not known about it.
        #[serde(flatten)]
        bytes: Bytes,
    },
    /// Nothing walked it, and this is why.
    Unmeasured {
        /// What stopped the reading.
        why: Unmeasured,
    },
}

/// Why a home was not measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unmeasured {
    /// The reader did not ask for one. A list does not walk a home.
    NotAsked,
    /// There is no home on the disk to walk.
    NoHome,
}

impl Unmeasured {
    /// Why the figure is not there, in one clause.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::NotAsked => "a list does not walk a home; nodal show measures it",
            Self::NoHome => "the unit has no home on this disk",
        }
    }

    /// The same, in the two words a table column has room for.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotAsked => "not measured",
            Self::NoHome => "no home",
        }
    }
}

impl Disk {
    /// The state of a home nothing has measured, which is every home a list reads.
    #[must_use]
    pub const fn unmeasured(state: EnvState) -> Self {
        let why = match state {
            EnvState::Absent => Unmeasured::NoHome,
            EnvState::Stopped | EnvState::Running => Unmeasured::NotAsked,
        };
        Self::Unmeasured { why }
    }

    /// The same home, walked.
    #[must_use]
    pub const fn measured(bytes: Bytes) -> Self {
        Self::Measured { bytes }
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
    /// How that is known: stated by a person or an agent, or observed by an adoption
    /// that recovered it from the records of the session that made the checkout.
    pub objective_epistemic: Option<Epistemic>,
    /// How current its environment is.
    pub freshness: Freshness,
    /// Why this unit needs a person, ranked ([`Needs`]).
    ///
    /// The same enum `nodal reclaim --check` answers with, so the word in this column
    /// and the word in that report mean one thing. A list must not start a per-unit
    /// survey to fill it in, so it is decided from the readings the list has already
    /// taken; the proof behind `unique loss` and `unknown` is the preflight's, and this
    /// says which unit to point it at.
    ///
    /// `None` is **not computed**, which every producer but the list is. It is not
    /// [`Needs::Nothing`], and a report must not print it as one: a create has not
    /// asked, and answering "nothing" for a question nobody put is the one thing this
    /// column must not do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<Needs>,
    /// What Git says about the branch, when it was asked.
    pub work: Option<WorkTree>,
    /// Its environment, when it has one.
    pub environment: Option<EnvLine>,
    /// When the unit was created, which its age is measured from.
    pub created_at: Timestamp,
    /// Who holds the write on it, when anybody does.
    #[serde(default)]
    pub holder: Option<Holder>,
    /// Who is attached to it right now, by tool.
    pub sessions: Vec<ToolSessions>,
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
            objective_epistemic: unit.objective_epistemic,
            freshness: Freshness::Unknown,
            needs: None,
            work: None,
            environment: None,
            created_at: unit.created_at,
            holder: None,
            sessions: Vec::new(),
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
    /// The worktrees of the project's repository that are not units: folders another
    /// tool made, in the same table as the units and marked as what they are.
    ///
    /// They share the table because a person looking at a project wants one answer to
    /// "what checkouts of this repository are on my disk", and two tables make them do
    /// the joining. They carry the word `worktree` in the leading column because the
    /// one thing that must never happen is a directory Nodal did not make being
    /// presented as a unit it did.
    #[serde(default)]
    pub worktrees: Vec<WorktreeRow>,
    /// What a signal could not answer. A note is not a failure: it is the difference
    /// between "nothing is attached" and "I could not see".
    pub notes: Vec<String>,
}

impl UnitList {
    /// Whether the list holds anything but units.
    ///
    /// The leading column is only printed when it distinguishes something. A project
    /// whose repository has no other worktrees prints the table it always printed.
    #[must_use]
    pub fn is_mixed(&self) -> bool {
        !self.worktrees.is_empty()
    }
}

impl Render for UnitList {
    const KIND: &'static str = "unit list";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        if self.units.is_empty() && self.worktrees.is_empty() {
            doc.push(Block::line(format!("{}: no units yet", self.project)));
        } else {
            doc.push(Block::table(list_table(self)));
        }
        for note in &self.notes {
            doc.push(Block::line(note.clone()));
        }
        doc
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
const COLUMNS: [&str; 7] = ["unit", "state", "branch", "main", "disk", "running", "last"];

/// The columns of the list, in the order `nodal ls` prints them.
///
/// The list answers one question: which unit needs a person next. So it carries what
/// Git says about each branch rather than what each home occupies, and `nodal status`
/// keeps the disk and runtime columns.
const LIST_COLUMNS: [&str; 9] =
    ["unit", "needs", "state", "branch", "main", "remote", "who", "age", "objective"];

/// The columns of a list that holds worktrees as well as units.
///
/// Two differences, and both exist so that no row is described by a word that is not
/// true of it. A leading column says which kind each row is, and the heading over the
/// names becomes `name` rather than `unit`, because a folder Nodal did not make must
/// never appear under the word `unit`.
///
/// A project whose repository names no other worktrees prints [`LIST_COLUMNS`] and is
/// unchanged by any of this. A column whose every cell reads `unit` tells a person
/// nothing and costs them the width.
const MIXED_COLUMNS: [&str; 10] =
    ["kind", "name", "needs", "state", "branch", "main", "remote", "who", "age", "objective"];

/// The table `nodal ls` prints: every unit, and every worktree of the project's
/// repository that is not one.
pub(crate) fn list_table(list: &UnitList) -> Table {
    let mixed = list.is_mixed();
    let mut table = Table::new(if mixed { &MIXED_COLUMNS[..] } else { &LIST_COLUMNS[..] });
    for unit in &list.units {
        let mut cells = vec![
            unit.slug.to_string(),
            unit.needs.map_or(NONE, Needs::label).to_owned(),
            state_cell(unit),
            branch_cell(unit),
            main_cell(unit),
            remote_cell(unit),
            who_cell(unit, list.now),
            human::span(list.now, unit.created_at),
            objective_cell(unit),
        ];
        if mixed {
            cells.insert(0, RowKind::Unit.label().to_owned());
        }
        table.push(cells);
    }
    for found in &list.worktrees {
        table.push(worktree_cells(found, list.now));
    }
    table
}

/// One foreign worktree in the columns of the unit table.
///
/// Three of those columns are questions only a unit has an answer to — what its
/// environment's state is, who is attached to it, and what a remote has of it beyond
/// the commits it holds — and they carry the placeholder rather than a guess. The rest
/// line up: the branch and its uncommitted paths, what merging would do, the age, and
/// what the worktree was made for.
///
/// The reason a row was not read further — `locked`, `prunable` — goes in the state
/// column and not in the verdict column, which is where the verdict table puts it. Here
/// there is a column for it, so a row that says `locked` twice would be saying one
/// thing in two places.
fn worktree_cells(found: &WorktreeRow, now: Timestamp) -> Vec<String> {
    let branch = found.branch.clone().unwrap_or_else(|| String::from("detached"));
    let dirty =
        if found.uncommitted > 0 { format!(" *{}", found.uncommitted) } else { String::new() };
    let unpushed =
        if found.unpushed > 0 { format!("^{}", found.unpushed) } else { String::from(NONE) };
    vec![
        RowKind::Worktree.label().to_owned(),
        found.name.clone(),
        String::from(NONE),
        found.note.clone().unwrap_or_else(|| String::from(NONE)),
        format!("{branch}{dirty}"),
        found.done.label(),
        unpushed,
        String::from(NONE),
        found.age_cell(now),
        found.for_cell(),
    ]
}

/// The unit table, which both the list and the status share.
pub(crate) fn table(units: &[UnitRow], now: Timestamp) -> Table {
    let mut table = Table::new(&COLUMNS);
    for unit in units {
        table.push(vec![
            unit.slug.to_string(),
            state_cell(unit),
            branch_cell(unit),
            main_cell(unit),
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
        Field::new("needs", needs_cell(unit)),
        Field::new("freshness", freshness_cell(&unit.freshness)),
    ];
    fields.push(Field::new("main", main_cell(unit)));
    fields.push(Field::new("remote", remote_cell(unit)));
    fields.push(Field::new("who", who_cell(unit, now)));
    if let Some(line) = hold_cell(unit.holder.as_ref()) {
        fields.push(Field::new("hold", line));
    }
    fields.push(Field::new("age", human::span(now, unit.created_at)));
    if let Some(environment) = &unit.environment {
        fields.push(Field::new("home", environment.home.display().to_string()));
        fields.push(Field::new("env", environment_label(environment)));
        fields.push(Field::new("ports", ports_cell(environment)));
        fields.push(Field::new("running", running_cell(unit)));
        fields.push(Field::new("disk", disk_field(environment)));
        fields.push(Field::new(
            "made by",
            environment
                .made_by
                .as_ref()
                .map_or_else(|| String::from(NONE), |version| format!("nodal {version}")),
        ));
    }
    fields.push(Field::new("last", last_cell(unit, now)));
    fields
}

/// The name a unit's status carries in output. A table, so the words are in one place.
#[must_use]
pub fn status_label(status: UnitStatus) -> &'static str {
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

/// What the unit needs next, said in words rather than in the column's one label.
///
/// The detail has room for the sentence, and the sentence says where the whole answer
/// is. A ranked word is enough to pick a unit out of a list of eight; it is not enough
/// to act on, and the command that is enough to act on is named.
fn needs_cell(unit: &UnitRow) -> String {
    match unit.needs {
        None => String::from(NONE),
        Some(Needs::Nothing) => Needs::Nothing.label().to_owned(),
        Some(needs) => format!(
            "{} — nodal reclaim {} --check says what a reclaim would take",
            needs.label(),
            unit.slug
        ),
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

/// The branch, with the state of the working tree after it: `*2` changed and not
/// staged, `+1` staged, `?3` untracked. A detached HEAD is said in words, because a
/// branch name is what every other column is about.
fn branch_cell(unit: &UnitRow) -> String {
    let Some(work) = &unit.work else { return unit.branch.to_string() };
    let name =
        if work.detached { format!("{} (detached)", unit.branch) } else { unit.branch.to_string() };
    format!("{name}{}", marks(&[(work.dirty, '*'), (work.staged, '+'), (work.untracked, '?')]))
}

/// The counted marks a cell ends with, each left out when its count is zero.
fn marks(counts: &[(u32, char)]) -> String {
    counts
        .iter()
        .filter(|(count, _)| *count > 0)
        .fold(String::new(), |marks, (count, mark)| marks + &format!(" {mark}{count}"))
}

/// What merging the branch would do, and how far it has moved from the branch it merges
/// into: `open +2 -5`, `done (absorbed)`, `conflict +1 -4`.
fn main_cell(unit: &UnitRow) -> String {
    let Some(work) = &unit.work else { return String::from(NONE) };
    let counts = marks(&[(work.main.ahead, '+'), (work.main.behind, '-')]);
    format!("{}{counts}", work.integration.label())
}

/// What the remote has and what it does not: `^2 v1`. A branch with no upstream carries
/// the placeholder, because nothing about it is known rather than nothing is different.
fn remote_cell(unit: &UnitRow) -> String {
    let Some(remote) = unit.work.as_ref().and_then(|work| work.remote.as_ref()) else {
        return String::from(NONE);
    };
    let counts = marks(&[(remote.divergence.ahead, '^'), (remote.divergence.behind, 'v')]);
    if counts.is_empty() { String::from("even") } else { counts.trim_start().to_owned() }
}

/// Who has the unit: the writer first, then who is attached, as
/// `ada holds 6 h · claude-code 2`.
///
/// The order is the point. The lock row is the one signal that crosses Linux accounts,
/// so on a host two engineers share it is the only answer to "is somebody else in this
/// unit"; the process table comes second because it can only see this account's
/// processes. A unit nobody holds shows the attachments alone, exactly as before.
///
/// The holder carries how long the hold has left, because "who has it" and "for how
/// much longer" are one question to a person deciding whether to wait or to take it.
fn who_cell(unit: &UnitRow, now: Timestamp) -> String {
    let mut named = Vec::new();
    if let Some(holder) = &unit.holder {
        named.push(holds_cell(holder, now));
    }
    named.extend(unit.sessions.iter().map(|each| format!("{} {}", each.tool, each.count)));
    human::join(&named)
}

/// One holder: who, whether their process is still there, and how long the hold has
/// left.
///
/// `ada holds 6 h` is said of a live session and of nothing else. A session that is gone
/// reads `ada gone, 6 h left`, which is the same two facts and neither of them a claim
/// that somebody is working. What the hold does is unchanged: it stands until it lapses.
fn holds_cell(holder: &Holder, now: Timestamp) -> String {
    format!(
        "{} {} {}",
        holder.actor,
        holder.state.verb(),
        left(&holder.state, human::span(holder.expires_at, now))
    )
}

/// What is known about the holder's process, for the one report that has room for it.
///
/// A live hold says nothing here: the WHO line already says it. The other two states are
/// the ones a person acts on, so each states the process it is about or the reason the
/// reading could not be taken.
fn hold_cell(holder: Option<&Holder>) -> Option<String> {
    let holder = holder?;
    let named = holder.pid.map_or_else(|| String::from("the process"), |pid| format!("pid {pid}"));
    match &holder.state {
        HolderState::Live => None,
        HolderState::Gone => Some(format!(
            "{named} is not on this host any more; the hold stands until it lapses"
        )),
        HolderState::Unknown { why } => Some(format!("liveness not read: {}", why.why())),
    }
}

/// The clock half of a holder cell: how long the hold has, said as what it is.
fn left(state: &HolderState, span: String) -> String {
    match state {
        HolderState::Live => span,
        HolderState::Gone | HolderState::Unknown { .. } => format!("{span} left"),
    }
}

/// What the unit's home occupies, or why that is not known.
///
/// The figure is apparent bytes and the cell says so, in the words
/// `nodal reclaim --check` uses for the same claim. An unmeasured home prints its reason
/// rather than the placeholder, because the placeholder reads as "nothing here".
pub(crate) fn disk_cell(unit: &UnitRow) -> String {
    let Some(environment) = unit.environment.as_ref() else { return String::from(NONE) };
    match &environment.disk {
        Disk::Measured { bytes } => measured_cell(bytes),
        Disk::Unmeasured { why } => why.label().to_owned(),
    }
}

/// A measured home, as a column shows it: apparent bytes, said to be apparent, and
/// marked as a floor where the walk could not read everything.
fn measured_cell(bytes: &Bytes) -> String {
    let floor = if bytes.complete { "" } else { "at least " };
    format!("{floor}{} apparent", human::bytes(bytes.apparent))
}

/// The same figure where there is room for the whole of it: what it is, and the one
/// thing it is not.
///
/// A detail is about one unit, so it carries the sentence a column cannot: apparent
/// bytes are not what removing the home gives back to the disk, and a home that was not
/// walked says who walks it.
fn disk_field(environment: &EnvLine) -> String {
    match &environment.disk {
        Disk::Measured { bytes } => {
            format!("{}{}{}", measured_cell(bytes), human::JOIN, bytes.exclusive_unknown)
        }
        Disk::Unmeasured { why } => format!("{}: {}", why.label(), why.why()),
    }
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
pub(crate) fn ports_cell(environment: &EnvLine) -> String {
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
///
/// A recovered objective is marked as one wherever it is printed. It is a reading of an
/// opening prompt rather than a statement of intent, and a person choosing what to do
/// with the unit has to be able to see the difference at a glance.
pub(crate) fn objective_cell(unit: &UnitRow) -> String {
    let Some(objective) = &unit.objective else { return String::from(NONE) };
    match unit.objective_epistemic {
        Some(Epistemic::Observed) => format!("{objective} (recovered)"),
        Some(Epistemic::Stated) | None => objective.to_string(),
    }
}

/// How long ago the unit was last active.
pub(crate) fn last_cell(unit: &UnitRow, now: Timestamp) -> String {
    unit.last_active.map_or_else(|| String::from(NONE), |at| human::since(now, at))
}
