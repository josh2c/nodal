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
//! The process table follows the same rule. A home that would move is not safe while the
//! table is unread, because what could not be read is not evidence that nothing stands
//! in the home. [`unmovable`] states this once: the preflight reports it as
//! [`Needs::UnknownEvidence`], and the reclaim refuses the move over it unless `--force`
//! is given.
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
//! An unread table blocks the same move ([`unmovable`]). It blocks only where there is a
//! move to block. A checkout adopted in place is
//! unregistered and left exactly where it is, so nothing is moved out from under
//! anybody, and a bystander there is reported and is not a reason.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::doctor::size::Bytes;
use crate::git::status::{Entry, State, Summary};
use crate::git::{Git, Oid, union};
use crate::lifecycle::uniqueness::{Finding, SAMPLE, Witness};
use crate::lifecycle::witness::{self, Checkout};
use crate::model::{Needs, Timestamp, UnitId};
use crate::paths;
use crate::runtime::attribute::{Note, Source, Standing};
use crate::runtime::processes::{self, Processes as _};
use crate::runtime::stop;
use crate::services::docker;
use crate::workspace::prune;

/// The label a unit's containers carry, which is how they are found again.
pub const UNIT_LABEL: &str = "nodal.unit";

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
    /// The unit, the groups the registry recorded for it, and the wrapper each group
    /// hangs off.
    ///
    /// One value rather than the unit and the groups apart, because it is one value:
    /// [`scan`] asks it as one, and a second shape here would be a conversion that can
    /// drop a field the reading needs ([`Own`]).
    pub own: Own<'a>,
    /// Whether a reclaim would move this home, which is the whole of what decides
    /// whether a bystander blocks.
    ///
    /// It lives here rather than on [`Input`] because it is only ever consulted about a
    /// reading of the runtime. A caller that does not ask for the runtime is not asking
    /// a question this could answer, and a field it had to fill in anyway would be a
    /// value nothing reads — which is how a hardcoded one gets in.
    pub moves: bool,
}

/// One home, and how much of it to read.
///
/// The three switches are not a preference. Each reading past the refusal costs processes
/// — a listing of every ignored path, a walk of each one for its size, a scan of the
/// process table, two more `rev-list` runs — and a reclaim must not pay for an answer it
/// does not act on.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The home to read.
    pub home: &'a Path,
    /// The project's own checkout, read once, when this machine still has one. A
    /// checkout that is not there is simply not asked, and the answer is then the
    /// stricter one.
    ///
    /// It arrives read rather than as a path because what an assessment asks of it is
    /// the same of every home: its git directory, its refs, its `origin`, and which of
    /// its tips it really holds. A caller that assesses many homes reads it once
    /// ([`Checkout::read`]) and pays for it once.
    pub checkout: Option<&'a Checkout>,
    /// The other repositories on this machine that may hold a copy of a commit, beside
    /// the project's own checkout.
    ///
    /// The reading used to ask the project's checkout and nothing else, so a commit a
    /// clone two directories away held was reported as the only copy, while the help of
    /// `nodal reclaim --check` promised the machine. These are the stores that answer
    /// the rest of that promise.
    ///
    /// Read by the caller and handed in, so that a caller assessing many homes discovers
    /// them once, and so that a caller which cannot afford the reading passes none and
    /// gets the stricter answer. An empty slice is exactly the reading Nodal made before.
    ///
    /// A store is only ever believed when it holds the object, proved by `git rev-list`
    /// in that repository ([`local_copies`]). A name never counts.
    pub siblings: &'a [PathBuf],
    /// Whether to classify the ignored state the home holds.
    pub state: bool,
    /// Whether to say where else each commit lives, rather than only which commits
    /// nothing proved live anywhere else.
    ///
    /// The whole difference is two `rev-list` runs. A refusal rests on
    /// [`Copies::OnlyHere`] and [`Copies::NotChecked`], and one reading answers for both
    /// of those at once; the groups that say a commit is proved on the remote or held a
    /// second time on this disk cost a reading each and are what a person reads rather
    /// than what an operation acts on. So `nodal reclaim --check` asks for them and a
    /// reclaim does not.
    ///
    /// It changes what is reported and never what is decided. Every group it adds is one
    /// [`Copies::survives`] is true of, which neither [`Assessment::findings`] nor
    /// [`reasons`] reads, so the verdict is the same verdict either way.
    pub dispositions: bool,
    /// The unit whose runtime to attribute, or nothing to leave it unread.
    pub runtime: Option<Attribution<'a>>,
}

impl<'a> Input<'a> {
    /// What a destructive operation asks: the refusal, and nothing it does not act on.
    ///
    /// No runtime, so no move question, so nothing here to get wrong about a home that
    /// would not move. The refusal a reclaim raises is about the work in a home, and it
    /// is the same refusal for a home Nodal made and for a checkout adopted in place.
    #[must_use]
    pub const fn refusal(
        home: &'a Path,
        checkout: Option<&'a Checkout>,
        siblings: &'a [PathBuf],
    ) -> Self {
        Self { home, checkout, siblings, state: false, dispositions: false, runtime: None }
    }
}

// ---------------------------------------------------------------------------
// Why a unit needs a person.
// ---------------------------------------------------------------------------

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
    ///
    /// "Only here" over a settled reading is a fact: there is no remote to ask, or the
    /// remote is on this disk and was read. Over a witness's reading it is the best this
    /// machine can say and no more, and the words say which, for the reason
    /// [`Finding::label`] draws the same line: a clone can only say what it last saw.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::OnlyHere { witness } if witness.settled() => "only here",
            Self::OnlyHere { .. } => "only here by the newest reading",
            Self::SecondLocalCopy { .. } => "second local copy",
            Self::RemoteProved { .. } => "proved on the remote",
            Self::NotChecked { .. } => "not checked",
        }
    }

    /// The reading that stands behind the disposition, when one does.
    #[must_use]
    pub const fn witness(&self) -> Option<&Witness> {
        match self {
            Self::OnlyHere { witness }
            | Self::RemoteProved { witness }
            | Self::NotChecked { witness } => Some(witness),
            Self::SecondLocalCopy { .. } => None,
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

/// The paths of one home under one disposition.
///
/// The disposition and the sentence that justifies it are written out by
/// [`PathGroup`]'s own [`Serialize`], read off [`Held`] rather than stored beside it. A
/// reader of `--json` gets `disposition` and `why` without this value being able to hold
/// a disposition that disagrees with what it is a group of.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

impl Serialize for PathGroup {
    /// The group, with the disposition and the reason for it written out.
    ///
    /// Both are read off [`Held`] here rather than kept in the struct, which is what
    /// makes it impossible for a stored disposition to drift away from the paths it is
    /// about. [`Deserialize`] ignores the two derived keys and reads the rest.
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;

        let mut group = out.serialize_struct("PathGroup", 6)?;
        group.serialize_field("held", &self.held)?;
        group.serialize_field("disposition", &self.held.survival())?;
        group.serialize_field("why", self.held.why())?;
        group.serialize_field("count", &self.count)?;
        group.serialize_field("sample", &self.sample)?;
        group.serialize_field("bytes", &self.bytes)?;
        group.end()
    }
}

// ---------------------------------------------------------------------------
// Runtime.
// ---------------------------------------------------------------------------

/// What is running against one unit, split the way a reclaim splits it.
///
/// One struct for one reading. The teardown, the verification and the preflight all ask
/// the same two signals about the same unit and want the same four answers out of them,
/// so there is one value rather than one per reader.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    /// The process groups the registry recorded for the unit. A reclaim signals these
    /// first, as groups, so one signal reaches a server and everything it started.
    ///
    /// Empty from [`attributed`], which reads the machine: a recorded group is a row of
    /// the registry and not a reading, and the caller that has the rows fills it in.
    ///
    /// Whether a group still holds a process is not asked. The portable way to ask is
    /// to signal it, and a preflight that signalled would be doing the thing it is
    /// there to describe.
    pub groups: Vec<u32>,
    /// The processes carrying the unit's own identifier. Attribution's certain level,
    /// and the only processes a teardown signals.
    pub processes: Vec<u32>,
    /// The containers carrying the unit's label. A reclaim removes these.
    pub containers: Vec<String>,
    /// The processes matched by working directory alone. Attribution's probable level: a
    /// reclaim never signals one, and refuses to move the home out from under it.
    pub bystanders: Vec<Standing>,
    /// The signals that could not be read, and why. A note is the difference between
    /// "nothing is running" and "I could not look".
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
pub fn attributed(own: Own<'_>, homes: &[PathBuf]) -> Runtime {
    let mut seen = processes_of(own, homes);
    match docker::survey(&docker::Cli) {
        Ok(docker::Survey::Ran(containers)) => seen.containers = labelled(containers, own.unit),
        Ok(docker::Survey::Unavailable { why }) => {
            seen.notes.push(Note::new(Source::Docker, why));
        }
        Err(error) => seen.notes.push(Note::new(Source::Docker, error.to_string())),
    }
    seen
}

/// Read the process table alone for one unit: the half of [`attributed`] a move asks.
///
/// A scan that failed is a [`Source::Environment`] note, and that note is what
/// [`unmovable`] reads. The move step and the preflight both get their answer here, so
/// they cannot read an unread table two ways.
#[must_use]
pub fn processes_of(own: Own<'_>, homes: &[PathBuf]) -> Runtime {
    let mut seen = Runtime::default();
    match scan(own, homes) {
        Ok((processes, bystanders)) => {
            seen.processes = processes;
            seen.bystanders = bystanders;
        }
        Err(error) => seen.notes.push(Note::new(Source::Environment, error.to_string())),
    }
    seen
}

/// Why a home cannot be moved over what the process table said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmovable<'a> {
    /// Processes Nodal did not start stand in the home.
    Standing(&'a [Standing]),
    /// The process table could not be read. What could not be read is not evidence that
    /// nothing stands in the home.
    Unread(&'a Note),
}

/// Why a move of the home would refuse over this reading, or nothing when it would not.
///
/// The one rule for both callers. [`reasons`] asks it of the runtime a preflight read,
/// and the reclaim's move step asks it of the scan it makes just before the move. A
/// second copy of the rule would let the preflight say safe where the move refuses.
#[must_use]
pub fn unmovable(runtime: &Runtime) -> Option<Unmovable<'_>> {
    if let Some(note) = runtime.notes.iter().find(|note| note.signal == Source::Environment) {
        return Some(Unmovable::Unread(note));
    }
    (!runtime.bystanders.is_empty()).then_some(Unmovable::Standing(&runtime.bystanders))
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
/// The first list is certain: each process carries this unit's `NODAL_ID`, which Nodal
/// wrote into the home's environment and nothing else writes. The second is probable:
/// each process stands in the home and says nothing about working on *this* unit.
///
/// Both halves are one predicate each, [`owns`] and [`bystander`], so that `nodal ls`
/// reaches the same rule rather than writing a second one.
///
/// # Errors
/// Whatever the process table reported, which on a host that has none is
/// [`Error::ProcessScanUnsupported`].
pub fn scan(own: Own<'_>, homes: &[PathBuf]) -> Result<(Vec<u32>, Vec<Standing>)> {
    let placed: Vec<PathBuf> = homes.iter().map(|home| paths::resolve(home)).collect();
    let spared = stop::spared();
    let mut certain = Vec::new();
    let mut standing = Vec::new();
    for process in processes::Processes::scan(&processes::Live)? {
        if owns(&process, own) {
            certain.push(process.pid);
        } else if bystander(&process, own, &placed, &spared) && !has_ended(process.pid) {
            standing.push(Standing::new(process.pid, process.command.clone()));
        }
    }
    Ok((certain, standing))
}

/// Whether this process is something standing in one of `unit`'s homes that a reclaim of
/// `unit` would never signal — attribution's probable level, and the thing a reclaim
/// refuses to move the home out from under.
///
/// One predicate, public, because one word has to mean one thing. `nodal ls` marks a row
/// `blocking_runtime` with it and `nodal reclaim --check` refuses with it, and the two
/// used to disagree over the case that makes the distinction worth drawing: a process of
/// **another** unit standing in this one's home. It carries a `NODAL_ID`, so a rule that
/// asked only whether one was there read it as something Nodal started and said nothing;
/// it does not carry *this* unit's, so a reclaim here will not signal it and will move
/// the home out from under it. That is a bystander by every part of the definition, and
/// both readings now say so.
///
/// `placed` must already be resolved ([`paths::resolve`]), because the working directory
/// the kernel reports has every symbolic link on the way to it taken out. A home reached
/// through a link — macOS reaches everything under `/var` that way, and so does anyone
/// whose state directory is a link — would otherwise match no process at all.
///
/// The two processes a stop spares ([`stop::spared`]) are never bystanders. A person who
/// typed the command inside the home is standing in it, and their own command must not be
/// the reason their unit reads as blocked.
#[must_use]
pub fn bystander(
    process: &processes::Running,
    own: Own<'_>,
    placed: &[PathBuf],
    spared: &[u32],
) -> bool {
    !owns(process, own)
        && !spared.contains(&process.pid)
        && in_one_of(process, placed)
        && !vouched_for_by_a_group(process, own)
}

/// A unit, and the process groups the registry recorded for it.
///
/// The unit's identifier is what says a process is the unit's own. The groups are not a
/// second way of saying that, and nothing here ever makes one into a signal target: they
/// answer a narrower question, which is whether a process standing in the home is a
/// stranger. A process inside a group Nodal recorded is not a stranger, and a reclaim
/// already reaches it as [`crate::runtime::stop::Target::Group`].
///
/// **Why a group and not a process identifier.** A session row records the leader of the
/// group it opened, and the leader is replaced while the group lives, so the recorded
/// number stops naming the process it named. A process identifier is also reused. Reading
/// that number back as "this is the unit's own" would claim whatever wears it now — and,
/// because the certain level is what a teardown signals, it would make a stranger's
/// process a signal target. So the number is never read as a name. It is read as a group:
/// the machine is asked which group a process is in, and which process a group hangs off
/// ([`vouched_for_by_a_group`]). The answer only ever takes a process out of the stranger
/// list.
#[derive(Debug, Clone, Copy)]
pub struct Own<'a> {
    /// The unit.
    pub unit: UnitId,
    /// The process groups the registry recorded for this unit's materialisation: a
    /// tether, or a group a recipe hook left behind.
    pub groups: &'a [u32],
    /// The `nodal run` that each of those groups hangs off, resolved while the group was
    /// still alive ([`Wrapper`]). Empty for a caller that took no such reading.
    pub wrappers: &'a [Wrapper],
}

/// A `nodal run` that started a group Nodal recorded, and when it started.
///
/// **Why the instant is here.** The relation that identifies this process — it is the
/// parent of the leader of a recorded group — can only be read while that leader is
/// alive, and a reclaim stops the group before it moves the home. So the relation is
/// resolved once, while it is still readable, and what is carried forward is the answer.
///
/// A carried process identifier is exactly the stale number this module refuses to treat
/// as a name, so it is not treated as one: it counts only when the process wearing it now
/// started at the instant this one did. An identifier that came round again belongs to a
/// process that started later, so it cannot match, and a host that does not date its
/// processes matches nothing at all. Being wrong in that direction leaves a refusal
/// standing, which is the safe one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wrapper {
    /// Its process identifier when the reading was taken.
    pub pid: u32,
    /// When it started, from this host's own record. `None` where the host does not say,
    /// and then nothing matches it.
    pub started_at: Option<Timestamp>,
}

impl<'a> Own<'a> {
    /// The unit and the groups the registry recorded for it.
    #[must_use]
    pub const fn of(unit: UnitId, groups: &'a [u32]) -> Self {
        Self { unit, groups, wrappers: &[] }
    }

    /// The same, with the `nodal run` each group hangs off already resolved.
    #[must_use]
    pub const fn and_wrappers(mut self, wrappers: &'a [Wrapper]) -> Self {
        self.wrappers = wrappers;
        self
    }
}

/// Whether a process is this unit's own, which is attribution's certain level.
///
/// It carries `NODAL_ID`, Nodal wrote that into the home's environment, and nothing else
/// writes it. The identifier has to be this unit's: another unit's is another unit's.
///
/// This is the whole of the rule, and it stays the whole of it, because this is the
/// predicate a teardown signals on. Nothing that is not the unit's own identifier may
/// widen it ([`Own`]).
///
/// Public for the same reason [`bystander`] is: `nodal ls` and `nodal reclaim --check`
/// say the same word over the same process, so they ask one predicate rather than
/// writing two.
#[must_use]
pub fn owns(process: &processes::Running, own: Own<'_>) -> bool {
    process.var(crate::env::vars::ID).is_some_and(|carried| carried == own.unit.to_string())
}

/// Whether a process the scan read has ended since.
///
/// A scan reads the whole table before any process in it is judged, and a short command
/// can end in between. Its group and its parent are then unreadable, so nothing can vouch
/// for it, and a refusal would name a process that is no longer in the home. Only an
/// answer of gone counts. A reading that fails leaves the process standing.
fn has_ended(pid: u32) -> bool {
    processes::Live
        .presences(&[pid])
        .is_ok_and(|seen| seen.get(&pid) == Some(&processes::Presence::Gone))
}

/// Whether a group the registry recorded vouches for this process.
///
/// Two ways, and both are readings of this machine taken now. The process is **in** the
/// group: the operating system is asked which group it is in ([`stop::group_of`]) and the
/// answer is one Nodal recorded. Or the process **leads to** the group: it is the parent
/// of the group's leader ([`processes::parent_of`]), which is what `nodal run --tether`
/// is — the wrapper that started the group and stands in the home while it runs.
///
/// A process that the wrapper started is vouched for too. When the group ends, the wrapper
/// records the run, and the `git` it starts for that stands in the home while the reclaim
/// that stopped the group reads the table before its move.
///
/// A recorded number is never read as a name. A process identifier is reused and a group
/// leader is replaced while its group lives, so the number alone proves nothing; what
/// proves something is the relation the machine reports between a process now and that
/// number now.
///
/// **This takes a process out of the stranger list and puts it in no other.** It is not
/// [`owns`], it never reaches [`Runtime::processes`], and nothing signals a process
/// because of it. A reclaim already reaches the group as
/// [`crate::runtime::stop::Target::Group`], which is the target the registry recorded,
/// and the wrapper ends when the group it is waiting on does.
fn vouched_for_by_a_group(process: &processes::Running, own: Own<'_>) -> bool {
    if stop::group_of(process.pid).is_some_and(|group| own.groups.contains(&group)) {
        return true;
    }
    if own.groups.iter().any(|leader| processes::parent_of(*leader) == Some(process.pid)) {
        return true;
    }
    let parent = processes::parent_of(process.pid);
    own.wrappers.iter().any(|wrapper| {
        is_still(process.pid, *wrapper) || parent.is_some_and(|parent| is_still(parent, *wrapper))
    })
}

/// Whether the process wearing this identifier now is the one the reading was taken of.
///
/// The instant it started is the whole of the proof, and both sides have to have one: an
/// identifier that came round again belongs to a process that started later, and a host
/// that dates nothing proves nothing. Either way the answer is no, which leaves the
/// stricter reading standing.
fn is_still(pid: u32, wrapper: Wrapper) -> bool {
    pid == wrapper.pid && wrapper.started_at.is_some() && started_at(pid) == wrapper.started_at
}

/// When the process wearing this identifier now started, from this host's own record.
///
/// `None` where the host publishes no process table, where the process has gone, and
/// where the host dates no process. Each of those is "I could not read it", and every
/// caller here treats that as proof of nothing.
fn started_at(pid: u32) -> Option<Timestamp> {
    match processes::Live.presences(&[pid]).ok()?.get(&pid) {
        Some(processes::Presence::Running { started_at }) => *started_at,
        _ => None,
    }
}

/// The `nodal run` each of these groups hangs off, read while the groups are alive.
///
/// Taken by the caller that still can — before anything is stopped — because the relation
/// is unreadable afterwards ([`Wrapper`]).
#[must_use]
pub fn wrappers_of(groups: &[u32]) -> Vec<Wrapper> {
    groups
        .iter()
        .filter_map(|leader| processes::parent_of(*leader))
        .filter(|pid| *pid > 1)
        .map(|pid| Wrapper { pid, started_at: started_at(pid) })
        .collect()
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
    /// Whether a reclaim would move this home, as the caller that asked for the runtime
    /// said. `false` where the runtime was not read at all, because the question only
    /// arises about something standing in a home that is about to move.
    pub moves: bool,
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
        findings.extend(
            self.path_finding(Held::Untracked, |count, sample| Finding::Untracked {
                count,
                sample,
            }),
        );
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
/// One `git status`, one `rev-list` for the refusal, and whatever [`Input`] asked for
/// beyond them — two more `rev-list` runs for the dispositions, a listing and a walk for
/// the ignored state, a scan for the runtime. Nothing is written, nothing is signalled,
/// and no remote is reached.
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
    let (commits, remotes) = history(&git, input)?;
    let mut assessment = Assessment {
        home: input.home.to_path_buf(),
        moves: input.runtime.is_some_and(|asked| asked.moves),
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
    let tracked =
        paths_where(status, |entry| matches!(entry.state, State::Tracked { .. } | State::Unmerged));
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
/// Three `rev-list` runs at worst, and each answers for the whole history at once. One
/// of the three is the refusal and the other two are the dispositions, so a caller that
/// did not ask for those takes the shorter path through [`refusing`] and pays for one.
///
/// The first fixes the denominator: the commits reachable from `HEAD` that the project's
/// own checkout does not reach from a branch, a tag or a stash of its own
/// ([`witness::Elsewhere::own`]). Everything it already reaches is the project's history
/// rather than this unit's work, and grouping it would print ninety thousand commits
/// under `remote_proved` and hide the three that matter.
///
/// The second takes out what a witnessed reading of the remote proves. With no witness it
/// takes out nothing, because a home's own remote-tracking refs are the record of a push
/// it made and not a reading of anything ([`witness`]).
///
/// The third takes out what the checkout holds without a ref of its own on it: a commit
/// under an unvouched-for `origin/*`, or one fetched by identifier and named by nothing.
///
/// What survives all three is a commit this machine cannot find a second copy of, and how
/// it is reported turns on whether the remote question was ever asked ([`unreached`]).
fn history(git: &Git, input: &Input<'_>) -> Result<(Vec<CommitGroup>, Vec<String>)> {
    let remotes = git.remotes()?;
    let found = witness::elsewhere(input.home, input.checkout);
    let checkout = input.checkout.map(Checkout::path);
    if !input.dispositions {
        let refused = refusing(git, checkout, input.siblings, &found, &remotes)?;
        return Ok((refused, remotes));
    }
    let ours = git.commits_outside("HEAD", &found.own)?;
    if ours.is_empty() {
        return Ok((Vec::new(), remotes));
    }
    let witness = Witness::of(&remotes, &found);
    let off_remote = git.commits_outside("HEAD", &union(&found.own, &found.remote))?;
    let unproved = git.commits_outside("HEAD", &found.tips())?;
    let proved = difference(&ours, &off_remote);
    let second = difference(&off_remote, &unproved);
    let (held, only) = local_copies(checkout, input.siblings, unproved);
    let mut groups = Vec::new();
    groups.extend(commit_group(Copies::RemoteProved { witness: witness.clone() }, proved));
    groups.extend(second_groups(checkout, second, held));
    groups.extend(commit_group(unreached(&witness), only));
    Ok((groups, remotes))
}

/// The one group a destructive path acts on: the commits nothing here proved survive.
///
/// One `rev-list` and not three. A refusal is raised over [`Copies::OnlyHere`] and
/// [`Copies::NotChecked`] and over nothing else, and both are drawn from one reading —
/// the commits of `HEAD` that no tip this machine found reaches. The other two groups say
/// *where else* a commit lives, which is the question a person asks and not one an
/// operation acts on: each costs a reading, and [`Input::dispositions`] is the caller
/// saying whether it wants them.
///
/// The split [`local_copies`] makes is still made, because a commit the checkout's object
/// store holds survives the removal and must not be refused over. What it takes out is
/// dropped rather than reported, which is the whole of what this path gives up.
fn refusing(
    git: &Git,
    checkout: Option<&Path>,
    siblings: &[PathBuf],
    found: &witness::Elsewhere,
    remotes: &[String],
) -> Result<Vec<CommitGroup>> {
    let unproved = git.commits_outside("HEAD", &found.tips())?;
    if unproved.is_empty() {
        return Ok(Vec::new());
    }
    let (_, only) = local_copies(checkout, siblings, unproved);
    let witness = Witness::of(remotes, found);
    Ok(commit_group(unreached(&witness), only).into_iter().collect())
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
fn local_copies(
    checkout: Option<&Path>,
    siblings: &[PathBuf],
    unproved: Vec<Oid>,
) -> (Vec<(PathBuf, Vec<Oid>)>, Vec<Oid>) {
    let mut left = unproved;
    let mut found = Vec::new();
    for store in checkout.into_iter().chain(siblings.iter().map(PathBuf::as_path)) {
        if left.is_empty() {
            break;
        }
        let holds = holds_of(store, &left);
        if holds.is_empty() {
            continue;
        }
        let (held, rest) = left.into_iter().partition(|oid| holds.contains(oid));
        found.push((store.to_path_buf(), held));
        left = rest;
    }
    (found, left)
}

/// Which of these commits one repository's object store really holds.
///
/// A repository that will not open, and a `rev-list` that would not run, both answer
/// with nothing. That is the stricter reading and it is the safe direction: a store
/// nobody could read has proved no second copy of anything, and the commit stays in the
/// group a refusal is raised over.
fn holds_of(store: &Path, commits: &[Oid]) -> BTreeSet<Oid> {
    let Some(git) = Git::open(store).ok() else { return BTreeSet::new() };
    git.held(commits).map(|held| held.into_iter().collect()).unwrap_or_default()
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

/// One group per object store on this machine that holds a second copy.
///
/// The checkout's own group carries the commits it names under a ref nothing vouched for
/// as well as the ones its store was found to hold, because both are the same claim
/// about the same repository and two rows would read as two findings.
fn second_groups(
    checkout: Option<&Path>,
    named_by_checkout: Vec<Oid>,
    mut held: Vec<(PathBuf, Vec<Oid>)>,
) -> Vec<CommitGroup> {
    if let Some(checkout) = checkout
        && !named_by_checkout.is_empty()
    {
        match held.iter_mut().find(|(store, _)| store == checkout) {
            Some((_, commits)) => *commits = union(commits, &named_by_checkout),
            None => held.insert(0, (checkout.to_path_buf(), named_by_checkout)),
        }
    }
    held.into_iter()
        .filter_map(|(held_by, commits)| commit_group(Copies::SecondLocalCopy { held_by }, commits))
        .collect()
}

// ---------------------------------------------------------------------------
// The runtime.
// ---------------------------------------------------------------------------

/// What is running against this home, at attribution's two levels, with the groups the
/// registry recorded added to it.
fn running(asked: Attribution<'_>, home: &Path) -> Runtime {
    let seen = attributed(asked.own, std::slice::from_ref(&home.to_path_buf()));
    Runtime { groups: asked.own.groups.to_vec(), ..seen }
}

// ---------------------------------------------------------------------------
// The verdict.
// ---------------------------------------------------------------------------

/// Why a person is needed, ranked, most actionable first.
///
/// Public so that a caller which assembles an [`Assessment`] from parts — a test, and
/// `nodal ls`, which reads the cheap half of this for every unit at once — reaches the
/// one ranking rather than writing a second one.
///
/// Every reason a reclaim refuses over is made here, from the same groups the refusal is
/// projected from, which is what makes [`Assessment::safe_to_reclaim`] agree with what
/// the operation would actually do.
#[must_use]
pub fn reasons(assessment: &Assessment) -> Vec<Reason> {
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

/// The reason the process table gives, when there is a move for it to block.
///
/// A bystander blocks the move. A table that could not be read blocks it too, as
/// unknown evidence, because nothing standing in the home was not read, it was not
/// proved.
///
/// A checkout adopted in place is unregistered and left exactly where it is, so nothing
/// is moved out from under anybody and a process standing in it stops nothing. Reporting
/// it as blocking would refuse a reclaim the operation itself would not refuse.
fn blocked(assessment: &Assessment) -> Option<Reason> {
    let runtime = assessment.runtime.as_ref().filter(|_| assessment.moves)?;
    Some(match unmovable(runtime)? {
        Unmovable::Standing(standing) => {
            let named: Vec<String> = standing.iter().take(SAMPLE).map(Standing::label).collect();
            Reason::new(Needs::BlockingRuntime, named.join(", "))
        }
        Unmovable::Unread(note) => Reason::new(
            Needs::UnknownEvidence,
            format!("the process table could not be read: {}", note.why),
        ),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use std::collections::BTreeMap;

    use super::{
        Assessment, CommitGroup, Copies, Held, Needs, Own, PathGroup, Reason, Timestamp, Wrapper,
        bystander, owns, reasons,
    };
    use crate::git::Oid;
    use crate::git::status::{Change, Entry, State, Submodule};
    use crate::lifecycle::uniqueness::{Finding, Witness};
    use crate::model::UnitId;

    /// A recorded group never widens what a teardown signals.
    ///
    /// The certain level is the list a teardown sends signals to, so anything that could
    /// put a process there has to be the unit's own identifier and nothing else. A group
    /// number is a record, it is reused, and the leader it named is replaced while the
    /// group lives — so reading it as a name would hand a stranger's process to the stop
    /// ladder. It never reaches this predicate.
    #[test]
    fn a_recorded_group_never_makes_a_process_the_units_own() {
        let unit = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let stranger = crate::runtime::processes::Running::new(4_023_598, BTreeMap::new())
            .in_directory("/homes/worker-import")
            .running("somebody else's shell");

        // Its identifier is one the registry recorded as a group.
        let groups = [4_023_598];
        assert!(
            !owns(&stranger, Own::of(unit, &groups)),
            "a recorded number is not an identity, and this is the list a teardown signals"
        );
        assert!(
            !owns(&stranger, Own::of(unit, &[])),
            "and without the record the answer is the same"
        );
    }

    /// The identifier is the whole of the certain level.
    #[test]
    fn a_process_carrying_the_units_identifier_is_the_units_own() {
        let unit = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let mut vars = BTreeMap::new();
        vars.insert(String::from("NODAL_ID"), unit.to_string());
        let theirs = crate::runtime::processes::Running::new(11, vars).running("node dev");
        assert!(owns(&theirs, Own::of(unit, &[])));

        let other = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FB1").unwrap();
        assert!(!owns(&theirs, Own::of(other, &[])), "another unit's is another unit's");
    }

    /// The reading that has to survive the group it came from.
    ///
    /// A reclaim stops the recorded groups and then moves the home. Between those two
    /// things the `nodal run` that started a group is still alive and the group is not,
    /// so the relation that identifies it — parent of that group's leader — can no longer
    /// be read. The answer is therefore resolved before the stop and carried, pinned to
    /// the instant the process started.
    ///
    /// This test is the mechanism rather than the timing: the group's leader does not
    /// exist here at all, which is exactly the state the move sees.
    ///
    /// **Both hosts are asserted.** A host that publishes no process table dates no
    /// process, so a carried reading proves nothing there and the stricter answer stands.
    /// That is the same rule as everywhere else here: what could not be read is not
    /// evidence.
    #[test]
    fn a_wrapper_resolved_before_its_group_was_stopped_is_still_not_a_stranger() {
        let unit = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let home = std::path::PathBuf::from("/homes/worker-import");
        // This test's own process stands in for the wrapper, because it is the one
        // process here whose start time the host will answer for if it answers at all.
        let pid = std::process::id();
        let standing = crate::runtime::processes::Running::new(pid, BTreeMap::new())
            .in_directory(&home)
            .running("nodal run");
        let asked = |wrappers: &[Wrapper]| {
            bystander(
                &standing,
                Own::of(unit, &[]).and_wrappers(wrappers),
                std::slice::from_ref(&home),
                &[],
            )
        };

        let started_at = super::started_at(pid);
        let Some(started_at) = started_at else {
            // No process table, so nothing is dated and nothing can be vouched for.
            assert!(
                asked(&[Wrapper { pid, started_at: None }]),
                "a host that dates no process vouches for none"
            );
            return;
        };

        // No group to read: the reclaim stopped it. The carried answer is what is left.
        assert!(
            !asked(&[Wrapper { pid, started_at: Some(started_at) }]),
            "the wrapper is not a stranger once its group has gone"
        );

        // The instant is the whole of the proof. An identifier that came round again
        // belongs to a process that started later, so it matches nothing.
        let earlier = Timestamp::from_unix_seconds(started_at.unix_seconds() - 60).ok();
        assert!(
            asked(&[Wrapper { pid, started_at: earlier }]),
            "a number that came round again proves nothing"
        );

        // And an undated reading is not evidence either.
        assert!(asked(&[Wrapper { pid, started_at: None }]), "an undated reading is not evidence");

        // What the wrapper started is its own too, while the wrapper is the one read.
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let started = crate::runtime::processes::Running::new(child.id(), BTreeMap::new())
            .in_directory(&home)
            .running("git");
        let carried = [Wrapper { pid, started_at: Some(started_at) }];
        let vouched = !bystander(
            &started,
            Own::of(unit, &[]).and_wrappers(&carried),
            std::slice::from_ref(&home),
            &[],
        );
        let _ = child.kill();
        let _ = child.wait();
        assert!(vouched, "a process the wrapper started is not a stranger");
    }

    /// A stranger in the home still blocks a move, whatever the registry recorded.
    #[test]
    fn a_stranger_in_the_home_is_still_a_bystander() {
        let unit = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let home = std::path::PathBuf::from("/homes/worker-import");
        let stranger = crate::runtime::processes::Running::new(5_000, BTreeMap::new())
            .in_directory(&home)
            .running("somebody else's editor");
        // A group number nothing on this machine is in or hangs off, so the readings
        // both answer no and the process is what it looks like.
        let groups = [4_294_967_000];
        assert!(bystander(&stranger, Own::of(unit, &groups), &[home], &[]));
    }

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
            moves: true,
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

    /// The process table follows the commit rule: a home that would move is not safe
    /// while the table is unread. A checkout left in place is not moved, so the same
    /// reading keeps nothing there.
    ///
    /// The assessment is built here and the host is not read, so both runners assert the
    /// same thing.
    #[test]
    fn an_unread_process_table_keeps_a_home_that_moves_and_not_one_left_in_place() {
        let mut moving = assessed(Vec::new(), Vec::new());
        moving.runtime =
            Some(super::Runtime { notes: vec![unread_table()], ..super::Runtime::default() });
        moving.reasons = reasons(&moving);
        assert!(!moving.safe_to_reclaim());
        assert_eq!(moving.top(), Needs::UnknownEvidence);
        assert_eq!(
            moving.reasons[0].detail,
            "the process table could not be read: a process scan reads /proc, which macos \
             does not have"
        );

        let mut in_place = moving.clone();
        in_place.moves = false;
        in_place.reasons = reasons(&in_place);
        assert!(in_place.safe_to_reclaim(), "{:?}", in_place.reasons);
    }

    /// The preflight and the move step refuse on one rule. The step refuses where
    /// [`super::unmovable`] answers; the preflight's verdict for a home that moves is
    /// read off the same answer, for every shape of runtime reading.
    #[test]
    fn the_check_refuses_the_move_exactly_where_the_move_step_does() {
        use crate::runtime::attribute::{Note, Source, Standing};

        let readings = [
            super::Runtime::default(),
            super::Runtime { notes: vec![unread_table()], ..super::Runtime::default() },
            super::Runtime {
                bystanders: vec![Standing::new(7, Some(String::from("tmux")))],
                ..super::Runtime::default()
            },
            super::Runtime {
                notes: vec![Note::new(Source::Docker, "the daemon is not running")],
                ..super::Runtime::default()
            },
        ];
        for runtime in readings {
            let mut moving = assessed(Vec::new(), Vec::new());
            moving.runtime = Some(runtime.clone());
            moving.reasons = reasons(&moving);
            assert_eq!(
                moving.safe_to_reclaim(),
                super::unmovable(&runtime).is_none(),
                "{runtime:?}"
            );
        }
    }

    /// The note a host with no process table leaves, in the words the scan gives.
    fn unread_table() -> crate::runtime::attribute::Note {
        crate::runtime::attribute::Note::new(
            crate::runtime::attribute::Source::Environment,
            "a process scan reads /proc, which macos does not have",
        )
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
        for held in [Held::Uncommitted, Held::Untracked, Held::LocalState, Held::Generated] {
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
        let mut moving = assessed(Vec::new(), Vec::new());
        moving.runtime = Some(runtime.clone());
        moving.reasons = reasons(&moving);
        assert_eq!(moving.top(), Needs::BlockingRuntime);
        assert!(!moving.safe_to_reclaim());

        let mut in_place = moving.clone();
        in_place.moves = false;
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
