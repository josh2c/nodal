//! The one reading behind every answer about what a reclaim would take away.
//!
//! [`crate::lifecycle::uniqueness`] answers one question — does this home hold work that
//! exists nowhere else — and answers it in the two words a destructive operation needs:
//! go on, or refuse. That is the right answer for the operation and it is not the answer
//! a person wants before they run it. "Refused: 3 commits no current reading proves a
//! remote has" does not say whether the work is on their laptop twice, on a server, or
//! about to be the only copy of a morning.
//!
//! So the reading is made once, here, and it keeps what it learned:
//!
//! > What is only here, what has another local copy, what a trustworthy local reading
//! > proves the remote has, what is reconstructable, what must survive, and would
//! > reclaim proceed or refuse right now?
//!
//! Both callers are this module's. `nodal reclaim` asks for the part its refusal rests
//! on and projects it back to [`Finding`]s ([`Assessment::findings`]); `nodal reclaim
//! --check` asks for all of it and prints it. There is one evaluator, so the preflight
//! cannot say safe where the reclaim refuses, or the other way about. That is the whole
//! reason this is a module and not a second reading beside the first.
//!
//! # Nothing here writes, signals, or reaches a network
//!
//! Every reading is `git`, the process table, the container daemon and `stat`. No hook
//! runs, no operation row is opened, no ref is fetched and no remote is asked anything.
//!
//! Liveness in particular is *not* asked. A recorded process group is reported as
//! recorded, because the only portable way to ask whether a group still holds a process
//! is to signal it, and a command a person runs to find out what would happen must not
//! be a command that does something. What is running is read from the process table,
//! which is a reading; a host that has none says so and claims nothing.
//!
//! # Four dispositions for a commit, and unknown is not one of the safe ones
//!
//! A commit of this home is in exactly one of four states, and the difference between
//! the middle two is what a person actually wants to know:
//!
//! | disposition | what it means | does removing the home lose it |
//! |---|---|---|
//! | [`Copies::RemoteProved`] | a witnessed reading of the remote reaches it | no, while the remote keeps the branch |
//! | [`Copies::SecondLocalCopy`] | another object store on this disk holds it | no, and no server is involved |
//! | [`Copies::NotChecked`] | nothing here read the remote, and nothing here holds it | unknown, so it is kept |
//! | [`Copies::OnlyHere`] | the remote question is settled and it is still nowhere else | yes |
//!
//! The set they are drawn from is the commits this home has that the project's own
//! checkout does not reach. That denominator is the point of it. Every commit of the
//! project's history is on the remote, and a report that called ninety thousand of them
//! `remote_proved` would bury the three that are not.
//!
//! `NotChecked` is not `RemoteProved` and it is not zero. A home whose only evidence is
//! its own `refs/remotes/origin/*` has evidence of a push it made, not a reading of a
//! remote ([`crate::lifecycle::witness`]), and a reclaim refuses over it. A ref name
//! with no object behind it proves nothing either, which is settled before any of this:
//! every tip is looked for in the store it was read out of.
//!
//! A branch a reviewer squash-merged is the case that makes this worth stating. Its
//! content is on `main`, so the diff is safe; its commit objects are in this home and
//! nowhere else, so the objects are not. This module reports the objects, and a reclaim
//! refuses. That is a conservative keep and it is deliberate: `--force` is how a person
//! says they have looked.
//!
//! # What must survive, what a tool writes again, and what nobody can price
//!
//! [`Held`] is the same split the trash prune makes, read without removing anything
//! ([`prune::survey`]). An ignore rule has to cover a path before it is a candidate at
//! all, so nothing a commit holds is ever in the answer, and the exclusion table is what
//! calls a directory regenerable. A `target` is reconstructable; a `.env.local` is not,
//! and the trash keeps it until `nodal gc` takes it.
//!
//! Bytes are reported apparent and said to be apparent. A unit home is a copy that
//! shares blocks with the base it came from, so what a removal gives back is not the sum
//! of the file sizes, and no portable call answers what it is. [`Bytes`] says the figure
//! it has and why it is not the other one.
//!
//! # Owned runtime and the bystander that blocks
//!
//! The two levels are attribution's own ([`crate::runtime::attribute`]) and this is the
//! same reading a reclaim makes, so the preflight names the processes the reclaim would
//! stop and the ones it would refuse to move the home out from under. A process matched
//! by its working directory alone is never the unit's to stop — a tmux pane, an editor
//! server over SSH and a teammate's shell all match — and it is the one that turns a
//! reclaim into a refusal.
//!
//! It blocks only where there is a move to block. A checkout adopted in place is
//! unregistered and left exactly where it is, so nothing is moved out from under
//! anybody, and a bystander there is reported and is not a reason.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::status::{Entry, State, Summary};
use crate::git::{Git, Oid};
use crate::lifecycle::uniqueness::{Finding, SAMPLE, Witness};
use crate::lifecycle::{guard, witness};
use crate::model::UnitId;
use crate::runtime::attribute::{Note, Source, Standing};
use crate::runtime::stop;
use crate::runtime::processes;
use crate::services::docker;
use crate::workspace::prune;
use crate::Result;

/// The label a unit's containers carry, which is how they are found again.
pub const UNIT_LABEL: &str = "nodal.unit";

/// What removing a directory gives back, and why that is not the figure printed.
const SHARED: &str = "a home shares blocks with the base it was copied from, and no \
     portable call says how many of these bytes are its own";

// ---------------------------------------------------------------------------
// What one assessment is asked to read.
// ---------------------------------------------------------------------------

/// The unit whose runtime an assessment should attribute.
///
/// Absent from an [`Input`] means the process table and the container daemon are not
/// read at all, which is what the destructive path asks for: it makes that reading
/// later, inside its own plan, where the answer decides what to signal.
#[derive(Debug, Clone, Copy)]
pub struct Attribution<'a> {
    /// The unit, which is the identifier a process carries when it is certainly the
    /// unit's.
    pub unit: UnitId,
    /// The process groups the registry recorded for it: a tether, or a group a recipe
    /// hook left behind.
    pub groups: &'a [u32],
}

/// One home, and how much of it to read.
///
/// The two switches are not a preference. Each reading past the refusal costs processes
/// — a listing of every ignored path, a walk of each one for its size, a scan of the
/// process table — and a reclaim must not pay for an answer it does not act on.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The home to read.
    pub home: &'a Path,
    /// The project's own checkout, when this machine still has one. A checkout that is
    /// not there is simply not asked, and the answer is then the stricter one.
    pub checkout: Option<&'a Path>,
    /// Whether the home is one Nodal made, and would therefore be moved to the trash.
    /// A checkout adopted in place is not moved, which is what decides whether a
    /// bystander blocks.
    pub managed: bool,
    /// Whether to classify the ignored state the home holds.
    pub state: bool,
    /// The unit whose runtime to attribute, or nothing to leave it unread.
    pub runtime: Option<Attribution<'a>>,
}

impl<'a> Input<'a> {
    /// What a destructive operation asks: the refusal, and nothing it does not act on.
    #[must_use]
    pub const fn refusal(home: &'a Path, checkout: Option<&'a Path>) -> Self {
        Self { home, checkout, managed: true, state: false, runtime: None }
    }
}

// ---------------------------------------------------------------------------
// Why a unit needs a person.
// ---------------------------------------------------------------------------

/// Why a unit needs a person, most actionable first.
///
/// The order of the variants is the ranking, and [`Ord`] is derived from it, so "the top
/// reason" is `min` and nothing anywhere sorts these by hand.
///
/// One enum serves two readers. The preflight emits the first three, which are the three
/// a reclaim refuses over; `nodal ls` emits all six, because its question is which unit
/// to open next rather than which unit is safe to end. A reader that saw two enums here
/// would have to be told that `unique_loss` in one is `unique_loss` in the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Work that may exist only here: uncommitted paths, or commits nothing else holds.
    UniqueLoss,
    /// Something Nodal did not start is standing in the home, so the home cannot move.
    BlockingRuntime,
    /// Nothing on this machine read the remote, so what it has is not known.
    UnknownEvidence,
    /// Merging would conflict, or the base has moved a long way under the branch.
    Diverged,
    /// The work is done, or is out for review, and the unit is a person's to end.
    Review,
    /// Nothing.
    #[default]
    Nothing,
}

impl Needs {
    /// The word a report prints for it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UniqueLoss => "unique loss",
            Self::BlockingRuntime => "blocked",
            Self::UnknownEvidence => "unknown",
            Self::Diverged => "diverged",
            Self::Review => "review",
            Self::Nothing => "—",
        }
    }

    /// Whether a reclaim refuses rather than going ahead for this reason.
    ///
    /// The three that do are the three a reclaim already refuses over today: the
    /// uniqueness check's findings, and a process standing in the home it would move.
    #[must_use]
    pub const fn refuses(self) -> bool {
        matches!(self, Self::UniqueLoss | Self::BlockingRuntime | Self::UnknownEvidence)
    }
}

/// One reason, ranked, in the words a report prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reason {
    /// Which reason it is, which is also where it sorts.
    pub needs: Needs,
    /// What it is about, named.
    pub detail: String,
}

impl Reason {
    /// A reason of a kind, naming what it is about.
    pub fn new(needs: Needs, detail: impl Into<String>) -> Self {
        Self { needs, detail: detail.into() }
    }
}

// ---------------------------------------------------------------------------
// Commits.
// ---------------------------------------------------------------------------

/// Where else a commit of this home lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Copies {
    /// Nowhere. The remote question was settled and the commit is still only here.
    OnlyHere {
        /// How the remote question was settled, which is what makes this a fact.
        witness: Witness,
    },
    /// Another object store on this machine holds it, whatever any remote has.
    SecondLocalCopy {
        /// The repository that holds it.
        held_by: PathBuf,
    },
    /// A witnessed reading of the remote reaches it.
    RemoteProved {
        /// Which reading, and who took it.
        witness: Witness,
    },
    /// Nothing on this machine read the remote and nothing here holds it, so whether it
    /// survives is not known. Kept, because unknown is not safe evidence.
    NotChecked {
        /// Why the reading could not be made.
        witness: Witness,
    },
}

impl Copies {
    /// The words a report prints over the group.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::OnlyHere { .. } => "only here",
            Self::SecondLocalCopy { .. } => "second local copy",
            Self::RemoteProved { .. } => "proved on the remote",
            Self::NotChecked { .. } => "not checked",
        }
    }

    /// Whether removing this home leaves the commit readable somewhere.
    #[must_use]
    pub const fn survives(&self) -> bool {
        matches!(self, Self::SecondLocalCopy { .. } | Self::RemoteProved { .. })
    }

    /// Why a reclaim would stop over it, when it would.
    #[must_use]
    pub const fn needs(&self) -> Option<Needs> {
        match self {
            Self::OnlyHere { .. } => Some(Needs::UniqueLoss),
            Self::NotChecked { .. } => Some(Needs::UnknownEvidence),
            Self::SecondLocalCopy { .. } | Self::RemoteProved { .. } => None,
        }
    }
}

/// The home's commits under one disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitGroup {
    /// Where else they live.
    pub copies: Copies,
    /// How many there are. Exact.
    pub count: usize,
    /// The first [`SAMPLE`] of them, newest first. A sample, and the count is the fact.
    pub sample: Vec<Oid>,
}

// ---------------------------------------------------------------------------
// Files and state.
// ---------------------------------------------------------------------------

/// What a reclaim would do to a group of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Survival {
    /// Nothing else holds it. It has to survive, or the work is gone.
    MustSurvive,
    /// A tool writes it again from the tree that is still here.
    Reconstructable,
}

/// What a home holds that a reclaim has an opinion about.
///
/// The disposition and the sentence are read off this rather than stored beside it, so
/// there is one place a path's fate is decided and no way for two fields to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Held {
    /// Tracked paths that differ from `HEAD` or the index, or that are unmerged.
    Uncommitted,
    /// Paths Git does not track and no ignore rule covers.
    Untracked,
    /// Ignored state no tool writes again: a local database, an `.env.local`.
    LocalState,
    /// Ignored state the exclusion table calls regenerable.
    Generated,
}

impl Held {
    /// What a reclaim would do to it.
    #[must_use]
    pub const fn survival(self) -> Survival {
        match self {
            Self::Uncommitted | Self::Untracked | Self::LocalState => Survival::MustSurvive,
            Self::Generated => Survival::Reconstructable,
        }
    }

    /// The words a report prints over the group.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Uncommitted => "uncommitted changes",
            Self::Untracked => "untracked files",
            Self::LocalState => "local state no tool writes again",
            Self::Generated => "build output and installed dependencies",
        }
    }

    /// Why it has that disposition, in one clause. Never empty: a disposition without a
    /// reason is a claim a person cannot check.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::Uncommitted => "no commit holds it, so removing the home loses it",
            Self::Untracked => "git does not track it and no ignore rule covers it",
            Self::LocalState => {
                "an ignore rule covers it and no tool writes it again; the trash keeps it \
                 until nodal gc takes it"
            }
            Self::Generated => {
                "an ignore rule covers it and the exclusion table calls it regenerable"
            }
        }
    }

    /// Whether a reclaim refuses rather than going ahead over it.
    ///
    /// Only the two that a removal would lose outright. A reclaim moves a home to the
    /// trash rather than deleting it, so the local state a person goes back for is still
    /// there afterwards, and refusing over it would refuse every reclaim of every home
    /// that ever held an `.env.local`.
    #[must_use]
    pub const fn refuses(self) -> bool {
        matches!(self, Self::Uncommitted | Self::Untracked)
    }
}

/// What a group of paths holds, and what is not known about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bytes {
    /// Apparent bytes: the sum of the file sizes, as the source counts them.
    pub apparent: u64,
    /// Whether every entry was counted. `false` makes [`Bytes::apparent`] a floor.
    pub complete: bool,
    /// Why this is not what removing the paths would give back to the disk.
    pub exclusive_unknown: String,
}

impl Bytes {
    /// What a walk of these paths measured.
    fn of(apparent: u64, complete: bool) -> Self {
        Self { apparent, complete, exclusive_unknown: String::from(SHARED) }
    }
}

/// The paths of one home under one disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathGroup {
    /// What they are, which is what decides their disposition.
    pub held: Held,
    /// How many there are. Exact.
    pub count: usize,
    /// The first [`SAMPLE`] of them, in the order they were found.
    pub sample: Vec<PathBuf>,
    /// What they hold, when they were measured. `None` for working-tree paths, which
    /// are read for what they are and not for what they weigh.
    pub bytes: Option<Bytes>,
}

// ---------------------------------------------------------------------------
// Runtime.
// ---------------------------------------------------------------------------

/// What is running against one home, split the way a reclaim splits it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    /// The process groups the registry recorded for the unit. A reclaim signals these
    /// first, as groups, so one signal reaches a server and everything it started.
    ///
    /// Whether a group still holds a process is not asked. The portable way to ask is
    /// to signal it, and a preflight that signalled would be doing the thing it is
    /// there to describe.
    pub groups: Vec<u32>,
    /// The processes carrying the unit's own identifier. A reclaim signals these.
    pub processes: Vec<u32>,
    /// The containers carrying the unit's label. A reclaim removes these.
    pub containers: Vec<String>,
    /// The processes matched by working directory alone. A reclaim never signals one,
    /// and refuses to move the home out from under it.
    pub bystanders: Vec<Standing>,
    /// The signals that could not be read, and why. A note is the difference between
    /// "nothing is running" and "I could not look".
    pub notes: Vec<Note>,
}

/// What this machine can see of one unit, at attribution's two levels.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    /// The processes that carry the unit's identifier. The certain level, and the only
    /// processes a teardown signals.
    pub certain: Vec<u32>,
    /// The processes a scan matched by working directory alone. The probable level,
    /// which is reported and never signalled.
    pub standing: Vec<Standing>,
    /// The containers the unit labelled as its own.
    pub containers: Vec<String>,
    /// The signals that could not be read.
    pub notes: Vec<Note>,
}

/// Read both signals for one unit. Never fails, for the reason an
/// [`crate::runtime::attribute::Attributor`] never fails: a machine Nodal cannot look at
/// is still a machine whose home can be reclaimed, and what it could not look at is a
/// line of the report.
///
/// Read directly rather than through `nodal ps`, and that is deliberate. Attribution
/// answers about the homes the registry calls live, and a reclaim's whole business is
/// making one of them not live. A verification built on it would go quiet at exactly the
/// moment it is supposed to speak: after the registry write, `ps` would attribute nothing
/// to the unit whether or not anything was still running.
#[must_use]
pub fn attributed(unit: UnitId, homes: &[PathBuf]) -> Seen {
    let mut seen = Seen::default();
    match scan(unit, homes) {
        Ok((certain, standing)) => {
            seen.certain = certain;
            seen.standing = standing;
        }
        Err(error) => seen.notes.push(Note::new(Source::Environment, error.to_string())),
    }
    match docker::survey(&docker::Cli) {
        Ok(docker::Survey::Ran(containers)) => seen.containers = labelled(containers, unit),
        Ok(docker::Survey::Unavailable { why }) => {
            seen.notes.push(Note::new(Source::Docker, why));
        }
        Err(error) => seen.notes.push(Note::new(Source::Docker, error.to_string())),
    }
    seen
}

/// The containers among these that carry this unit's label.
fn labelled(containers: Vec<docker::Container>, unit: UnitId) -> Vec<String> {
    let id = unit.to_string();
    containers
        .into_iter()
        .filter(|container| container.label(UNIT_LABEL) == Some(&id))
        .map(|container| container.name)
        .collect()
}

/// Read the process table once and sort what it says about this unit into the two levels
/// attribution has ([`crate::runtime::attribute::Confidence`]).
///
/// The first list is certain: each process carries `NODAL_ID`, which Nodal wrote into the
/// home's environment and nothing else writes. The second is probable: each process
/// stands in the home and says nothing about which unit it is working on.
///
/// `home` is resolved ([`guard::resolve`]), because the working directory the kernel
/// reports has every symbolic link on the way to it already taken out. A home reached
/// through a link — macOS reaches everything under `/var` that way, and so does anyone
/// whose state directory is a link — would otherwise match no process at all.
///
/// The two processes a stop spares ([`stop::spared`]) are left out of the probable list
/// altogether. A person who typed the command inside the home is standing in it, and
/// their own command must not be the reason their reclaim refuses.
///
/// # Errors
/// Whatever the process table reported, which on a host that has none is
/// [`Error::ProcessScanUnsupported`].
pub fn scan(unit: UnitId, homes: &[PathBuf]) -> Result<(Vec<u32>, Vec<Standing>)> {
    let placed: Vec<PathBuf> = homes.iter().map(|home| guard::resolve(home)).collect();
    let id = unit.to_string();
    let spared = stop::spared();
    let mut certain = Vec::new();
    let mut standing = Vec::new();
    for process in processes::Processes::scan(&processes::Live)? {
        if process.var(crate::env::vars::ID) == Some(id.as_str()) {
            certain.push(process.pid);
        } else if in_one_of(&process, &placed) && !spared.contains(&process.pid) {
            standing.push(Standing::new(process.pid, process.command.clone()));
        }
    }
    Ok((certain, standing))
}

/// Whether a process stands in one of these directories, which is the whole of the
/// probable signal.
fn in_one_of(process: &processes::Running, homes: &[PathBuf]) -> bool {
    let Some(cwd) = process.cwd.as_deref() else { return false };
    homes.iter().any(|home| cwd.starts_with(home))
}

// ---------------------------------------------------------------------------
// The assessment.
// ---------------------------------------------------------------------------

/// Everything one reading of one home found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    /// The home that was read.
    pub home: PathBuf,
    /// Whether it is a home Nodal made, and would therefore move.
    pub managed: bool,
    /// The remotes the home names. Empty means there is nowhere it could have pushed
    /// to, which is worth printing rather than reading as "it did not push".
    pub remotes: Vec<String>,
    /// The home's own commits, grouped by where else they live. Empty means the home
    /// has no commit the project's checkout does not already reach.
    pub commits: Vec<CommitGroup>,
    /// What the home holds, grouped by what a reclaim would do to it.
    pub paths: Vec<PathGroup>,
    /// What is running against it, when that was asked for. `None` is "not read", which
    /// a report must never print as "nothing".
    pub runtime: Option<Runtime>,
    /// Why a person is needed, ranked, most actionable first.
    pub reasons: Vec<Reason>,
    /// The readings of the repository that could not be made, and why. A note is never
    /// a failure, and it is never silence either: a group missing because a listing
    /// failed must not read as a group that is empty.
    pub notes: Vec<String>,
}

impl Assessment {
    /// Whether a reclaim run now would go ahead rather than refuse.
    ///
    /// Read off the reasons, so the verdict and the reasons cannot disagree, and the
    /// reasons are made by the same predicates the reclaim refuses on.
    #[must_use]
    pub fn safe_to_reclaim(&self) -> bool {
        !self.reasons.iter().any(|reason| reason.needs.refuses())
    }

    /// The one reason to act on first, or [`Needs::Nothing`] when there is none.
    #[must_use]
    pub fn top(&self) -> Needs {
        self.reasons.first().map_or(Needs::Nothing, |reason| reason.needs)
    }

    /// The assessment as the findings a destructive operation refuses with.
    ///
    /// This is a projection and never a second reading. [`Finding`] is the shape an
    /// older Nodal journalled and the shape [`crate::Error::NotUnique`] carries, so it
    /// is kept exactly as it was and built from here.
    #[must_use]
    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        findings.extend(self.path_finding(Held::Uncommitted, |count, sample| {
            Finding::Uncommitted { count, sample }
        }));
        findings.extend(self.path_finding(Held::Untracked, |count, sample| Finding::Untracked {
            count,
            sample,
        }));
        findings.extend(self.unpushed());
        findings
    }

    /// One finding over the paths of one kind, or nothing when the home holds none.
    fn path_finding(
        &self,
        held: Held,
        build: impl Fn(usize, Vec<PathBuf>) -> Finding,
    ) -> Option<Finding> {
        let group = self.paths.iter().find(|group| group.held == held)?;
        Some(build(group.count, group.sample.clone()))
    }

    /// The one finding the commits make, which is the two groups a reclaim refuses over
    /// read as the one sentence it has always printed.
    ///
    /// They are one finding and not two because they are one refusal: a home holding a
    /// commit only it has and a home holding a commit nothing checked are both homes
    /// this machine will not remove, and [`Witness`] is the field that says which.
    fn unpushed(&self) -> Option<Finding> {
        let kept: Vec<&CommitGroup> =
            self.commits.iter().filter(|group| !group.copies.survives()).collect();
        let count = kept.iter().map(|group| group.count).sum();
        if count == 0 {
            return None;
        }
        let sample: Vec<Oid> =
            kept.iter().flat_map(|group| group.sample.clone()).take(SAMPLE).collect();
        let witness = match kept.first().map(|group| &group.copies) {
            Some(Copies::OnlyHere { witness } | Copies::NotChecked { witness }) => witness.clone(),
            _ => Witness::default(),
        };
        Some(Finding::Unpushed { count, sample, remotes: self.remotes.clone(), witness })
    }
}

/// Read `home`, and report everything a reclaim of it would have an opinion about.
///
/// One `git status`, at most three `rev-list` runs, and whatever [`Input`] asked for
/// beyond them. Nothing is written, nothing is signalled, and no remote is reached.
///
/// # Errors
/// [`crate::Error::Git`] when the status or a revision could not be read, and
/// [`crate::Error::NotARepository`] when `home` is not one.
pub fn assess(input: &Input<'_>) -> Result<Assessment> {
    let git = Git::open(input.home)?;
    let status = git.status()?;
    let mut paths = working(&status);
    let mut notes = Vec::new();
    if input.state {
        paths.extend(ignored(input.home, &mut notes));
    }
    let (commits, remotes) = history(&git, input.home, input.checkout)?;
    let mut assessment = Assessment {
        home: input.home.to_path_buf(),
        managed: input.managed,
        remotes,
        commits,
        paths,
        runtime: input.runtime.map(|asked| running(asked, input.home)),
        reasons: Vec::new(),
        notes,
    };
    assessment.reasons = reasons(&assessment);
    Ok(assessment)
}

// ---------------------------------------------------------------------------
// The working tree.
// ---------------------------------------------------------------------------

/// The paths of the working tree that carry work, in the two kinds they come in.
fn working(status: &Summary) -> Vec<PathGroup> {
    let tracked = paths_where(status, |entry| {
        matches!(entry.state, State::Tracked { .. } | State::Unmerged)
    });
    let untracked = paths_where(status, |entry| entry.state == State::Untracked);
    [group(Held::Uncommitted, tracked), group(Held::Untracked, untracked)]
        .into_iter()
        .flatten()
        .collect()
}

/// One group over a list of paths, or nothing when the list is empty.
fn group(held: Held, paths: Vec<PathBuf>) -> Option<PathGroup> {
    if paths.is_empty() {
        return None;
    }
    let count = paths.len();
    Some(PathGroup { held, count, sample: paths.into_iter().take(SAMPLE).collect(), bytes: None })
}

/// The paths of the entries a rule selects, in the order Git listed them, leaving out
/// the files Nodal wrote itself.
fn paths_where(status: &Summary, wanted: impl Fn(&Entry) -> bool) -> Vec<PathBuf> {
    status
        .entries
        .iter()
        .filter(|entry| wanted(entry) && !is_nodals_own(entry))
        .map(|entry| entry.path.clone())
        .collect()
}

/// Whether an entry is a file Nodal wrote into the home rather than work a person did.
///
/// Two halves, and the second is what keeps this from ever hiding somebody's work.
///
/// The name has to be one Nodal owns ([`crate::env::files::is_own`]). And the entry has
/// to be **untracked**, because every file Nodal writes into a home is untracked there:
/// it is written after the clone and it is hidden from `git status` through the home's
/// `.git/info/exclude`. A path of that name which Git tracks is the project's own file,
/// carried by the clone, and a change to it is a change somebody made.
///
/// That distinction is the whole of the difference between the two settings files. A
/// project that commits `.claude/settings.json` gets a home whose copy is tracked and
/// which the adapter never writes to; a project that does not gets one Nodal wrote
/// ([`crate::adapters::claude_code`]).
fn is_nodals_own(entry: &Entry) -> bool {
    entry.state == State::Untracked && crate::env::files::is_own(&entry.path)
}

// ---------------------------------------------------------------------------
// The ignored state.
// ---------------------------------------------------------------------------

/// The ignored state of the home, in the two kinds the trash contract splits it into.
///
/// The classification is [`prune::survey`] and not a copy of it, so what the preflight
/// calls reconstructable is exactly what the prune would drop.
fn ignored(home: &Path, notes: &mut Vec<String>) -> Vec<PathGroup> {
    let surveyed = prune::survey(home);
    notes.extend(surveyed.notes);
    let (generated, local): (Vec<prune::Candidate>, Vec<prune::Candidate>) =
        surveyed.candidates.into_iter().partition(|candidate| candidate.reason.is_some());
    [measured(Held::Generated, generated), measured(Held::LocalState, local)]
        .into_iter()
        .flatten()
        .collect()
}

/// One group over classified paths, with what they hold, or nothing when there are none.
fn measured(held: Held, candidates: Vec<prune::Candidate>) -> Option<PathGroup> {
    if candidates.is_empty() {
        return None;
    }
    let apparent = candidates.iter().map(|candidate| candidate.bytes).sum();
    let count = candidates.len();
    let sample = candidates.into_iter().take(SAMPLE).map(|candidate| candidate.path).collect();
    Some(PathGroup { held, count, sample, bytes: Some(Bytes::of(apparent, true)) })
}

// ---------------------------------------------------------------------------
// The commits.
// ---------------------------------------------------------------------------

/// The home's own commits, grouped by where else they live, and the remotes it names.
///
/// Three `rev-list` runs at worst, and each answers for the whole history at once.
///
/// The first fixes the denominator: the commits reachable from `HEAD` that the project's
/// own checkout does not reach by a ref of its own. Everything the checkout already has
/// is the project's history rather than this unit's work, and grouping it would print
/// ninety thousand commits under `remote_proved` and hide the three that matter.
///
/// The second takes out what a witnessed reading of the remote proves. What it removes
/// is [`Copies::RemoteProved`]; with no witness it removes nothing, because a home's own
/// remote-tracking refs are the record of a push it made and not a reading of anything
/// ([`witness`]).
///
/// The third is asked only of what is left, and it is the one question that needs no ref
/// at all: does the checkout's object store hold the commit anyway? A commit fetched into
/// a repository by identifier sits there with no ref on it, and a person rescuing work
/// out of a home makes exactly one of those.
fn history(
    git: &Git,
    home: &Path,
    checkout: Option<&Path>,
) -> Result<(Vec<CommitGroup>, Vec<String>)> {
    let remotes = git.remotes()?;
    let found = witness::elsewhere(home, checkout);
    let ours = git.commits_outside("HEAD", &found.local)?;
    if ours.is_empty() {
        return Ok((Vec::new(), remotes));
    }
    let witness = Witness::of(&remotes, &found);
    let unproved = git.commits_outside("HEAD", &found.tips())?;
    let proved = difference(&ours, &unproved);
    let (second, only) = local_copies(checkout, unproved);
    let mut groups = Vec::new();
    groups.extend(commit_group(Copies::RemoteProved { witness: witness.clone() }, proved));
    groups.extend(second_group(checkout, second));
    groups.extend(commit_group(unreached(&witness), only));
    Ok((groups, remotes))
}

/// How the commits nothing proved are reported: as only here, or as not checked.
///
/// The two are one refusal and they are not one claim. A home whose remote question was
/// settled — there is no remote, or the remote is on this disk and was read — holds the
/// only copy, and the report says so. A home nothing could check may hold the only copy,
/// and the report says that instead. Calling the second the first would be a claim this
/// machine did not earn.
fn unreached(witness: &Witness) -> Copies {
    match witness {
        Witness::Unchecked => Copies::NotChecked { witness: witness.clone() },
        settled => Copies::OnlyHere { witness: settled.clone() },
    }
}

/// Split the commits no remote reading proved into the ones the checkout's object store
/// holds anyway and the ones it does not.
///
/// A checkout that cannot be read, or that will not answer, holds nothing as far as this
/// is concerned, which is the strict direction.
fn local_copies(checkout: Option<&Path>, unproved: Vec<Oid>) -> (Vec<Oid>, Vec<Oid>) {
    let Some(git) = checkout.and_then(|path| Git::open(path).ok()) else {
        return (Vec::new(), unproved);
    };
    let Ok(held) = git.held(&unproved) else {
        return (Vec::new(), unproved);
    };
    let held: BTreeSet<Oid> = held.into_iter().collect();
    unproved.into_iter().partition(|oid| held.contains(oid))
}

/// The members of `all` that `fewer` does not have, in the order `all` has them.
fn difference(all: &[Oid], fewer: &[Oid]) -> Vec<Oid> {
    let fewer: BTreeSet<&Oid> = fewer.iter().collect();
    all.iter().filter(|oid| !fewer.contains(oid)).cloned().collect()
}

/// One commit group, or nothing when the disposition covers no commit.
fn commit_group(copies: Copies, commits: Vec<Oid>) -> Option<CommitGroup> {
    if commits.is_empty() {
        return None;
    }
    let count = commits.len();
    Some(CommitGroup { copies, count, sample: commits.into_iter().take(SAMPLE).collect() })
}

/// The group of commits a second object store on this machine holds.
fn second_group(checkout: Option<&Path>, commits: Vec<Oid>) -> Option<CommitGroup> {
    let held_by = checkout?.to_path_buf();
    commit_group(Copies::SecondLocalCopy { held_by }, commits)
}

// ---------------------------------------------------------------------------
// The runtime.
// ---------------------------------------------------------------------------

/// What is running against this home, at attribution's two levels.
fn running(asked: Attribution<'_>, home: &Path) -> Runtime {
    let seen = attributed(asked.unit, std::slice::from_ref(&home.to_path_buf()));
    Runtime {
        groups: asked.groups.to_vec(),
        processes: seen.certain,
        containers: seen.containers,
        bystanders: seen.standing,
        notes: seen.notes,
    }
}

// ---------------------------------------------------------------------------
// The verdict.
// ---------------------------------------------------------------------------

/// Why a person is needed, ranked, most actionable first.
///
/// Every reason a reclaim refuses over is made here, from the same groups the refusal is
/// projected from, which is what makes [`Assessment::safe_to_reclaim`] agree with what
/// the operation would actually do.
fn reasons(assessment: &Assessment) -> Vec<Reason> {
    let mut reasons = Vec::new();
    reasons.extend(assessment.paths.iter().filter(|group| group.held.refuses()).map(|group| {
        Reason::new(Needs::UniqueLoss, format!("{} ({})", group.held.label(), group.count))
    }));
    reasons.extend(assessment.commits.iter().filter_map(|group| {
        Some(Reason::new(
            group.copies.needs()?,
            format!("{} ({})", group.copies.label(), group.count),
        ))
    }));
    reasons.extend(blocked(assessment));
    reasons.sort_by_key(|reason| reason.needs);
    reasons
}

/// The reason a bystander gives, when there is a move for it to block.
///
/// A checkout adopted in place is unregistered and left exactly where it is, so nothing
/// is moved out from under anybody and a process standing in it stops nothing. Reporting
/// it as blocking would refuse a reclaim the operation itself would not refuse.
fn blocked(assessment: &Assessment) -> Option<Reason> {
    let runtime = assessment.runtime.as_ref().filter(|_| assessment.managed)?;
    let named: Vec<String> =
        runtime.bystanders.iter().take(SAMPLE).map(Standing::label).collect();
    (!named.is_empty()).then(|| Reason::new(Needs::BlockingRuntime, named.join(", ")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Assessment, CommitGroup, Copies, Held, Needs, PathGroup, Reason, reasons};
    use crate::git::Oid;
    use crate::git::status::{Change, Entry, State, Submodule};
    use crate::lifecycle::uniqueness::{Finding, Witness};

    #[test]
    fn the_files_nodal_writes_into_a_home_are_not_a_persons_work() {
        for own in
            [".nodal/id", ".nodal/env", ".nodal/manifest.toml", ".envrc", ".claude/settings.json"]
        {
            assert!(super::is_nodals_own(&untracked(own)), "{own}");
        }
        assert!(!super::is_nodals_own(&untracked("app/main.txt")));
        assert!(!super::is_nodals_own(&untracked(".nodal-notes")));
    }

    /// The one case where the name is Nodal's and the file is not: a project that
    /// commits its own settings gets a home whose copy Git tracks, and an edit to it is
    /// work nowhere else has.
    #[test]
    fn a_settings_file_the_project_commits_is_the_projects_and_not_nodals() {
        let tracked = Entry {
            path: PathBuf::from(".claude/settings.json"),
            state: State::Tracked { index: Change::Unmodified, worktree: Change::Modified },
            origin: None,
            submodule: Submodule::No,
        };
        assert!(!super::is_nodals_own(&tracked));
    }

    /// An entry as `git status` reports an untracked path.
    fn untracked(path: &str) -> Entry {
        Entry {
            path: PathBuf::from(path),
            state: State::Untracked,
            origin: None,
            submodule: Submodule::No,
        }
    }

    /// One commit, for the properties about the words a group is reported under.
    fn oid(byte: &str) -> Oid {
        Oid::parse(&byte.repeat(20)).unwrap()
    }

    /// An assessment of a home holding these commit groups and these path groups.
    fn assessed(commits: Vec<CommitGroup>, paths: Vec<PathGroup>) -> Assessment {
        let mut assessment = Assessment {
            home: PathBuf::from("/h"),
            managed: true,
            commits,
            paths,
            ..Assessment::default()
        };
        assessment.reasons = reasons(&assessment);
        assessment
    }

    /// One commit group of one commit.
    fn commits(copies: Copies) -> CommitGroup {
        CommitGroup { copies, count: 1, sample: vec![oid("ab")] }
    }

    /// One path group of one path.
    fn paths(held: Held) -> PathGroup {
        PathGroup { held, count: 1, sample: vec![PathBuf::from("a.rs")], bytes: None }
    }

    /// The two dispositions that mean the work survives are the two that let a reclaim
    /// go ahead. Anything else keeps the home, including the one that means "I do not
    /// know", because unknown is not safe evidence.
    #[test]
    fn only_a_second_copy_or_a_proved_remote_makes_a_home_safe_to_reclaim() {
        let checkout = PathBuf::from("/w/project");
        let by = vec![checkout.clone()];
        for safe in [
            Copies::SecondLocalCopy { held_by: checkout },
            Copies::RemoteProved { witness: Witness::Checked { by: by.clone() } },
        ] {
            assert!(safe.survives(), "{safe:?}");
            assert!(assessed(vec![commits(safe)], Vec::new()).safe_to_reclaim());
        }
        for kept in [
            Copies::OnlyHere { witness: Witness::NoRemote },
            Copies::NotChecked { witness: Witness::Unchecked },
        ] {
            assert!(!kept.survives(), "{kept:?}");
            assert!(!assessed(vec![commits(kept)], Vec::new()).safe_to_reclaim());
        }
    }

    /// "Only here" is a claim about a remote and a run has to have earned it. Where
    /// nothing read the remote, the answer is that nothing read it.
    #[test]
    fn a_commit_nothing_checked_is_never_reported_as_the_only_copy() {
        assert_eq!(super::unreached(&Witness::Unchecked).label(), "not checked");
        assert_eq!(super::unreached(&Witness::NoRemote).label(), "only here");
        let direct = Witness::Direct { by: vec![PathBuf::from("/w")] };
        assert_eq!(super::unreached(&direct).label(), "only here");
    }

    /// The top reason is the most actionable one, whatever order the groups came in.
    #[test]
    fn the_reasons_are_ranked_and_the_top_one_is_the_first() {
        let assessment = assessed(
            vec![commits(Copies::NotChecked { witness: Witness::Unchecked })],
            vec![paths(Held::Uncommitted)],
        );
        assert_eq!(assessment.top(), Needs::UniqueLoss);
        assert_eq!(
            assessment.reasons.iter().map(|reason| reason.needs).collect::<Vec<Needs>>(),
            vec![Needs::UniqueLoss, Needs::UnknownEvidence]
        );
    }

    /// Local state the trash keeps must survive, and it is not a reason to refuse: a
    /// reclaim moves the home rather than deleting it, and refusing over an `.env.local`
    /// would refuse every reclaim of every home that ever held one.
    #[test]
    fn state_the_trash_keeps_must_survive_and_still_lets_the_reclaim_go_ahead() {
        assert_eq!(Held::LocalState.survival(), super::Survival::MustSurvive);
        assert!(!Held::LocalState.refuses());
        assert!(assessed(Vec::new(), vec![paths(Held::LocalState)]).safe_to_reclaim());
        assert!(!assessed(Vec::new(), vec![paths(Held::Untracked)]).safe_to_reclaim());
    }

    /// Every disposition says why it is that disposition. A report that allowed a
    /// removal without a reason would be asking to be believed.
    #[test]
    fn every_disposition_gives_a_reason() {
        for held in
            [Held::Uncommitted, Held::Untracked, Held::LocalState, Held::Generated]
        {
            assert!(!held.why().is_empty(), "{held:?}");
            assert!(!held.label().is_empty(), "{held:?}");
        }
    }

    /// The findings a reclaim refuses with are this same reading, projected. The two
    /// groups it keeps are one refusal, and the witness field says which of them it is.
    #[test]
    fn the_findings_are_a_projection_of_the_groups_and_not_a_second_reading() {
        let mut assessment = assessed(
            vec![
                commits(Copies::RemoteProved { witness: Witness::NoRemote }),
                commits(Copies::NotChecked { witness: Witness::Unchecked }),
            ],
            vec![paths(Held::Untracked), paths(Held::Generated)],
        );
        assessment.remotes = vec![String::from("origin")];
        let findings = assessment.findings();
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(matches!(findings[0], Finding::Untracked { count: 1, .. }), "{findings:?}");
        let Finding::Unpushed { count, witness, .. } = &findings[1] else {
            panic!("{findings:?}");
        };
        assert_eq!(*count, 1, "the proved commit is not a finding");
        assert_eq!(*witness, Witness::Unchecked);
    }

    /// A bystander blocks a home that would move, and only that. A checkout adopted in
    /// place is left where it is, so nothing is moved out from under anybody.
    #[test]
    fn a_bystander_blocks_a_home_that_moves_and_not_a_checkout_left_in_place() {
        let runtime = super::Runtime {
            bystanders: vec![crate::runtime::attribute::Standing::new(
                7,
                Some(String::from("tmux")),
            )],
            ..super::Runtime::default()
        };
        let mut managed = assessed(Vec::new(), Vec::new());
        managed.runtime = Some(runtime.clone());
        managed.reasons = reasons(&managed);
        assert_eq!(managed.top(), Needs::BlockingRuntime);
        assert!(!managed.safe_to_reclaim());

        let mut in_place = managed.clone();
        in_place.managed = false;
        in_place.reasons = reasons(&in_place);
        assert!(in_place.safe_to_reclaim(), "{:?}", in_place.reasons);
    }

    /// A reason always names what it is about. The rank alone is not an answer.
    #[test]
    fn a_reason_names_what_it_is_about() {
        let reason = Reason::new(Needs::UniqueLoss, "untracked files (1)");
        assert!(!reason.detail.is_empty());
        assert_eq!(reason.needs.label(), "unique loss");
    }
}
