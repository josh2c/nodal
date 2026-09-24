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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::doctor::size::Bytes;
use crate::git::status::{Entry, State, Summary};
use crate::git::{Git, Oid, union};
use crate::lifecycle::complete::{self, Incomplete, Lacking};
use crate::lifecycle::kernel::{self, Evidence, LossSet, Verdict};
use crate::lifecycle::uniqueness::{Finding, SAMPLE, Witness};
use crate::lifecycle::witness::{self, Checkout};
use crate::model::reading::{self, Answered, Reading, Store, Unchecked};
use crate::model::{Holding, Needs, Timestamp, UnitId};
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

/// Where one home's work is read from.
///
/// A live home is read from every ref it holds, and not from the branch it is on. A
/// person works on that branch most of the time, and most of the time is not a promise a
/// removal may rest on. `git switch -c`, `git stash` and `git tag` each write a ref, and
/// a commit under one of those is work this home holds and nothing else has. A reading
/// of `HEAD` alone reported no commits at all over such a home, called it safe, and the
/// directory went to the trash whole and out of it on a timer.
///
/// A trashed home is read from the tips its caller names instead. Nothing is checked out
/// in the trash, so there is no `HEAD` to start from, and the caller has the tips already
/// from the record the reclaim left.
#[derive(Debug, Clone, Copy, Default)]
pub enum Work<'a> {
    /// Every ref the home holds, plus `HEAD`. The reading every caller but the sweep of
    /// the trash makes, and therefore the default.
    #[default]
    Checkout,
    /// The commits these tips reach, and nothing else. An empty list is a home with no
    /// work on any ref, which is a fact and not a failure.
    Tips(&'a [Oid]),
}

/// What a person made, said as what it is not: everything under Nodal's own namespace.
///
/// Every ref under `refs/nodal/` is one Nodal wrote, and none of them is work this home
/// did. Two are copies of the person's own checkout that the create put there — its
/// reading of the remote and its own branches ([`crate::git::refs::ORIGIN`],
/// [`crate::git::refs::CHECKOUT`]) — and the commits behind those are in the checkout.
/// The rest are records of runs: what one operation wrote before it ran, the branch a
/// squash folded, and the snapshot of the working tree a `done` takes. A record's tree is
/// the home's working tree and its parent is the home's own branch, so it holds no
/// content this reading does not already have from the tree and from the branch.
///
/// Counting them would keep every home that has ever run a command, for ever, which is a
/// leak and not a safety property. `nodal gc` draws the same line over a home in the
/// trash and states it there as well ([`crate::lifecycle::ops::gc`]), with one addition
/// this reading does not need: nothing is checked out in the trash, so the sweep names
/// the snapshot ref itself, because a forced reclaim put a working tree on it that no
/// tree reading can reach any more.
const NODALS_OWN: &str = crate::git::refs::NAMESPACE;

/// What one reading walks, read once and used for every question it asks.
///
/// Three `rev-list` runs at worst ask about the same set of tips, and the tips are one
/// reading of the home's refs. Taking them three times would cost two processes to learn
/// what the first answer held.
struct Walked {
    /// The tips the reading starts from.
    tips: Vec<Oid>,
    /// What the record says was walked.
    walked: Vec<String>,
    /// What the record says was not, and each of those is a reason rather than a gap.
    not_walked: Vec<String>,
}

impl Work<'_> {
    /// Read what this home's work hangs off, for the evidence record and for the walk.
    ///
    /// One `for-each-ref` and one `rev-parse` for a live home, and no process at all for
    /// a home whose tips the caller already holds.
    ///
    /// `HEAD` is asked for separately because `for-each-ref` does not list it, and a home
    /// left on a detached `HEAD` has a commit checked out that no ref of it names. A
    /// `HEAD` nothing answers for is an unborn branch, which is a home with no commit of
    /// its own rather than a reading that failed.
    ///
    /// # Errors
    /// [`crate::Error::Git`] when the refs could not be listed.
    fn resolve(self, git: &Git) -> Result<Walked> {
        if let Self::Tips(tips) = self {
            return Ok(Walked {
                tips: tips.to_vec(),
                walked: tips.iter().map(ToString::to_string).collect(),
                not_walked: Vec::new(),
            });
        }
        let mut tips: Vec<Oid> = git
            .all_refs()?
            .into_iter()
            .filter(|reference| !reference.name.starts_with(NODALS_OWN))
            .map(|reference| reference.oid)
            .collect();
        tips.extend(git.rev_parse(HEAD).ok());
        tips.sort_unstable();
        tips.dedup();
        Ok(Walked {
            tips,
            walked: WALKED.iter().map(|&name| String::from(name)).collect(),
            not_walked: vec![format!("{NODALS_OWN}* (refs Nodal wrote, which hold no work of their own)")],
        })
    }
}

/// What the record says a reading of a live home walked.
///
/// The namespaces rather than the names. A home holds one branch of its own and a copy of
/// every branch the checkout has, so the names would be a list of the checkout's work
/// under a heading about this home.
const WALKED: [&str; 4] = ["HEAD", "refs/heads/*", "refs/tags/*", "refs/stash"];

/// What a revision is called when it is the commit the home has checked out.
const HEAD: &str = "HEAD";

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
    /// Where this home's work is read from: the branch it is on, or the tips a caller
    /// names because nothing is checked out in it.
    pub work: Work<'a>,
    /// What a reclaim does with this home: move it to the trash, or leave the person
    /// the checkout they adopted. It decides what the report says becomes of the paths
    /// in it, and it is a reading of the registry rather than a guess.
    pub fate: Fate,
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
    /// [`kernel::judge`] refuses over, so the verdict is the same verdict either way.
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
    ///
    /// The work is read from the branch the home is on, which is every caller but the
    /// sweep of the trash ([`Work`]). The one caller that reads named tips instead sets
    /// the field over this: `Input { work: Work::Tips(&tips), ..Input::refusal(..) }`.
    ///
    /// [`Input::fate`] is unread by this reading and is not a claim about the home.
    /// `state: false` is what makes that true: the two dispositions whose sentence
    /// depends on the fate are [`Held::LocalState`] and [`Held::Generated`], both of
    /// them made only by the ignored state this does not read, and the two it does make
    /// say the same thing about a home that moves and a home that does not.
    /// `a_refusal_reading_makes_no_group_whose_sentence_depends_on_the_fate` holds it.
    #[must_use]
    pub const fn refusal(
        home: &'a Path,
        checkout: Option<&'a Checkout>,
        siblings: &'a [PathBuf],
    ) -> Self {
        Self {
            home,
            checkout,
            siblings,
            work: Work::Checkout,
            fate: Fate::Trashed,
            state: false,
            dispositions: false,
            runtime: None,
        }
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
    /// Nothing proved a copy of it, and a reading that could have is missing. Kept,
    /// because unknown is not safe evidence.
    NotChecked {
        /// What this machine could say about the remote.
        witness: Witness,
        /// The stores that hold it and could not be proved to hold the work behind it.
        ///
        /// Empty is the ordinary case: nothing here read the remote and no store on this
        /// disk has the commit at all. A store in this list is the other case, and it is
        /// the one a person can act on, so it is named rather than folded into the first.
        ///
        /// Defaulted on the way in, so that a group written by an older Nodal reads as
        /// the first case rather than as a claim it never made.
        #[serde(default)]
        stores: Vec<Incomplete>,
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
            | Self::NotChecked { witness, .. } => Some(witness),
            Self::SecondLocalCopy { .. } => None,
        }
    }

    /// Whether removing this home leaves the commit readable somewhere.
    ///
    /// Read off [`Copies::needs`], and not a second partition of the same four variants: a
    /// disposition survives a removal exactly when nothing about it would stop one.
    #[must_use]
    pub const fn survives(&self) -> bool {
        self.needs().is_none()
    }

    /// The disposition of a commit nothing checked and no store on this disk holds.
    ///
    /// The ordinary way one is made, and the one every caller outside this module wants:
    /// a store that holds the commit is the other case, and it is made where the stores
    /// are read ([`unchecked`]).
    #[must_use]
    pub const fn not_checked(witness: Witness) -> Self {
        Self::NotChecked { witness, stores: Vec::new() }
    }

    /// The words a refusal is raised under, which name the store where there is one.
    ///
    /// The label alone says a reading was not made. A person refused over a store that is
    /// on their own disk needs to know which store and which property it failed, or the
    /// refusal is a wall. It is built from what the group already holds, so the one maker
    /// of a verdict reads nothing to print it ([`kernel::judge`]).
    #[must_use]
    pub fn detail(&self, count: usize) -> String {
        let mut said = format!("{} ({count})", self.label());
        if let Self::NotChecked { stores, .. } = self {
            for store in stores {
                said.push_str("; ");
                said.push_str(&store.because());
            }
        }
        said
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

/// A commit of this home whose tree a remote tip already holds, under another id.
///
/// A force-push that rewrites history leaves exactly this: the remote's new tip and the
/// home's commit are two identifiers over one tree object, so not one byte of the work is
/// at risk and the commit is still nowhere else. Nodal compares commit identity, so it
/// reported the commit as only here and refused, and nothing said why the refusal was
/// about a name rather than about the content. This is what says it.
///
/// It is [`Survival::Reconstructable`] and never evidence of a second copy. Taking the
/// tree from the ref rebuilds the content; it does not rebuild the commit, its message,
/// its author or its parents. So this never reaches a refusal and never moves a verdict
/// ([`kernel::judge`]); `--force` is how a person says the content is
/// enough.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SameContent {
    /// The commit of this home.
    pub commit: Oid,
    /// The ref whose tip holds the same tree, by its full name.
    pub reference: String,
    /// That ref's tip.
    pub tip: Oid,
    /// The tree object both of them name.
    pub tree: Oid,
}

impl Serialize for SameContent {
    /// The row, with the disposition written out.
    ///
    /// Read off [`Survival`] here rather than kept in the struct, for the reason
    /// [`PathGroup`] does the same: a stored disposition is one that can drift from what
    /// it is a disposition of.
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;

        let mut row = out.serialize_struct("SameContent", 5)?;
        row.serialize_field("commit", &self.commit)?;
        row.serialize_field("reference", &self.reference)?;
        row.serialize_field("tip", &self.tip)?;
        row.serialize_field("tree", &self.tree)?;
        row.serialize_field("disposition", &Survival::Reconstructable)?;
        row.end()
    }
}

impl SameContent {
    /// The one line a report prints for this row.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "{}: same content as {} ({}) under a different id — the tree is one object, \
             so nothing of the work is at risk; the commit is still only here, and a \
             reclaim keeps this home",
            short(&self.commit),
            self.reference,
            short(&self.tip)
        )
    }
}

/// The first eight characters of an identifier, which is how a report names one.
fn short(oid: &Oid) -> String {
    oid.as_str().chars().take(8).collect()
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

/// What a reclaim does with the home itself, which decides what it does with the
/// paths in it.
///
/// Two answers and not a boolean, because each one names an operation a person can read
/// about. `Trashed` is a home Nodal made: it moves whole, and the prune takes the build
/// output out of the copy on the way in. `LeftInPlace` is a checkout the person made and
/// Nodal adopted: the unit is unregistered and the directory is never moved, so the
/// build output in it is reachable only through `nodal reclaim --prune`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fate {
    /// A home Nodal made, which a reclaim moves to the trash.
    Trashed,
    /// A checkout adopted in place, which a reclaim unregisters and leaves.
    LeftInPlace,
}

impl Fate {
    /// The fate of a home the registry calls managed, or not.
    #[must_use]
    pub const fn of(managed: bool) -> Self {
        if managed { Self::Trashed } else { Self::LeftInPlace }
    }
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
    ///
    /// Two of the four clauses say what becomes of the path, and what becomes of it
    /// depends on where the home goes. A home Nodal made is moved to the trash, which
    /// keeps the local state and drops the build output. A checkout adopted in place is
    /// not moved at all, so nothing goes to a trash and the build output goes only when
    /// the person asks for it. [`Fate`] is that fact, and it is read rather than
    /// guessed: a report that offered the trash for a home no reclaim moves would name
    /// a directory the person could not go to.
    #[must_use]
    pub const fn why(self, fate: Fate) -> &'static str {
        match (self, fate) {
            (Self::Uncommitted, _) => "no commit holds it, so removing the home loses it",
            (Self::Untracked, _) => "git does not track it and no ignore rule covers it",
            (Self::LocalState, Fate::Trashed) => {
                "an ignore rule covers it and no tool writes it again; the trash keeps it \
                 until nodal gc takes it"
            }
            (Self::LocalState, Fate::LeftInPlace) => {
                "an ignore rule covers it and no tool writes it again; the home is not \
                 moved, so it stays where it is"
            }
            (Self::Generated, Fate::Trashed) => {
                "an ignore rule covers it and the exclusion table calls it regenerable; \
                 the trash does not keep it"
            }
            (Self::Generated, Fate::LeftInPlace) => {
                "an ignore rule covers it and the exclusion table calls it regenerable; \
                 `nodal reclaim --prune` removes it and nothing else does"
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
/// [`PathGroup`]'s own [`Serialize`], read off [`Held`] and [`Fate`] rather than stored
/// beside them. A reader of `--json` gets `disposition` and `why` without this value
/// being able to hold a disposition that disagrees with what it is a group of.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PathGroup {
    /// What they are, which is what decides their disposition.
    pub held: Held,
    /// What a reclaim does with the home these paths are in, which is the other half of
    /// the sentence. Every group of one reading carries the same value, set once from
    /// [`Input::fate`]: a home is moved or it is not, and the paths in it do not each
    /// get to answer that differently.
    pub fate: Fate,
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
        group.serialize_field("why", self.held.why(self.fate))?;
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
    /// How many processes the table held, as far as this account could list it.
    #[serde(default)]
    pub read: usize,
    /// How many of those the host refused to show. Each is a process that could be
    /// standing in this home, and the count is what makes that sentence checkable
    /// rather than a slogan.
    #[serde(default)]
    pub withheld: usize,
    /// Which readings of occupancy this host answered, as
    /// [`processes::OCCUPANCY`] names them. Empty for a table that was not read at all,
    /// because none of them was made.
    ///
    /// It is here rather than in the record beside it because this is the value the
    /// kernel is handed ([`kernel::Evidence::runtime`]), and one fact belongs in one
    /// place: a second copy in the report is a second thing to drift.
    #[serde(default)]
    pub occupancy: Vec<String>,
    /// Where in an operation this reading was taken ([`taken`]).
    ///
    /// A reclaim reads the table twice — once before the teardown, for its own refusal,
    /// and once in the step that decides whether the home may move — and only the second
    /// gated the rename. A record that did not say which reading it held would describe a
    /// machine as it was before the operation touched it.
    #[serde(default)]
    pub at: Option<String>,
}

impl Runtime {
    /// Whether the process table was not read at all: nothing asked for it, or it could
    /// not be listed. Which of those is in [`Reading::not_checked`].
    ///
    /// Derived and never stored, so it cannot disagree with the notes it is derived from.
    #[must_use]
    pub fn unread(&self) -> bool {
        self.notes.iter().any(|note| note.unread(Source::Environment))
    }

    /// How far the process table was read, in the word a report prints.
    ///
    /// Derived, like [`Runtime::unread`], from the notes and the withheld count. A word
    /// and not a third enumeration of the same three cases: nothing branches on the two
    /// that are not `unread`, and the only reader of the distinction is the line a report
    /// prints.
    #[must_use]
    pub fn how_far(&self) -> &'static str {
        if self.unread() {
            "unread"
        } else if self.withheld > 0 {
            "read in part"
        } else {
            "read in full"
        }
    }

    /// Say where this reading was taken, and drop the occupancy a table that was never
    /// read has no business listing.
    ///
    /// A table that was not read answered none of the occupancy questions, so it lists
    /// none as taken; printing them beside `unread` would read as readings made over
    /// nothing.
    ///
    /// On the value itself, because a caller that has a reading and no [`Reading`] to
    /// put beside it still has to stamp it: the step that refuses to move a home makes
    /// exactly that reading, and building a record to throw away was the only way to
    /// reach this rule ([`record_table`]).
    pub fn taken(&mut self, at: &str) {
        self.at = Some(String::from(at));
        if self.unread() {
            self.occupancy.clear();
        }
    }
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
        Ok(read) => {
            seen.processes = read.certain;
            seen.bystanders = read.standing;
            seen.notes = read.notes;
            seen.read = read.read;
            seen.withheld = read.withheld;
            seen.occupancy =
                processes::OCCUPANCY.iter().map(|&reading| String::from(reading)).collect();
        }
        Err(error) => seen.notes.push(unread_table(&error)),
    }
    seen
}

/// The note a scan that failed leaves: the one fact [`unmovable`] reads to say the
/// table went unread.
///
/// Public because `nodal ls` keeps this note for the same `Err` and asks it the same
/// question ([`Note::unread`]), so the list and the preflight cannot read one failed
/// scan two ways.
#[must_use]
pub fn unread_table(error: &crate::Error) -> Note {
    Note::new(Source::Environment, error.to_string())
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
/// The one rule for both callers. [`kernel::judge`] asks it of the runtime a preflight read,
/// and the reclaim's move step asks it of the scan it makes just before the move. A
/// second copy of the rule would let the preflight say safe where the move refuses.
#[must_use]
pub fn unmovable(runtime: &Runtime) -> Option<Unmovable<'_>> {
    if let Some(note) = runtime.notes.iter().find(|note| note.unread(Source::Environment)) {
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
pub fn scan(own: Own<'_>, homes: &[PathBuf]) -> Result<Seen> {
    let placed: Vec<PathBuf> = homes.iter().map(|home| paths::resolve(home)).collect();
    let spared = stop::spared();
    let running = processes::Processes::scan(&processes::Live)?;
    let (certain, standing) = Table::read(&running).sort(own, &placed, &spared, &has_ended);
    Ok(Seen {
        certain,
        standing,
        notes: crate::runtime::attribute::withheld(&running),
        read: running.len(),
        withheld: running.iter().filter(|process| process.withheld.is_some()).count(),
    })
}

/// One reading of the process table, sorted.
///
/// A struct rather than a tuple because the reading grew two counts that are not about
/// any one process: how many rows there were, and how many of them the host refused. Both
/// are evidence, and a fourth and fifth element of a tuple would have been read wrong.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// The processes carrying this unit's identifier.
    pub certain: Vec<u32>,
    /// The processes standing in, or holding something inside, one of its homes.
    pub standing: Vec<Standing>,
    /// What the host refused to show, as notes.
    pub notes: Vec<Note>,
    /// How many processes were in the table.
    pub read: usize,
    /// How many of those were refused.
    pub withheld: usize,
}

/// One reading of the process table, prepared once for however many homes are asked
/// about.
///
/// **Why a type and not a function.** The rule that judges a process this account cannot
/// read needs the whole table: it asks where that process came from, and the answer is a
/// relation to other rows. Working that out from the raw table costs a map of every
/// process's lineage and a pass over every row, and `nodal ls` asks about every home the
/// registry holds. Doing it per home made a listing cost homes × processes twice over.
/// The two things that do not depend on the home — the lineage map and which rows were
/// withheld — are worked out here, once, and every home is answered from them.
pub struct Table<'a> {
    /// The rows themselves.
    running: &'a [processes::Running],
    /// Where each process came from, by identifier.
    lineage: BTreeMap<u32, processes::Lineage>,
    /// The rows this account could not read, which are the only candidates for the
    /// lineage rule. On an ordinary machine this is a few dozen of several hundred.
    withheld: Vec<&'a processes::Running>,
}

impl<'a> Table<'a> {
    /// Prepare one reading. Nothing here asks the machine anything.
    #[must_use]
    pub fn read(running: &'a [processes::Running]) -> Self {
        Self {
            running,
            lineage: running.iter().map(|process| (process.pid, process.lineage)).collect(),
            withheld: running
                .iter()
                .filter(|process| process.withheld == Some(processes::Withheld::AnotherAccount))
                .collect(),
        }
    }

    /// Sort this reading into what a reclaim of one unit signals and what it refuses
    /// over.
    ///
    /// **The one entry point.** `nodal ls`, `nodal reclaim --check` and the move step of
    /// an executed reclaim all reach the answer here, over a table each of them read for
    /// itself, so none of them can print `clear` over a home another refuses on. Asking
    /// [`bystander`] directly answers only half of it: the half about a process this
    /// account can read.
    ///
    /// Split out of [`scan`] so that the whole rule is a function of a table rather than
    /// of this machine. That is what lets a test state the one table no unprivileged test
    /// can make a machine hold — a process whose record this account may not read — and
    /// assert the arm that judges it.
    ///
    /// Nothing here asks the machine anything, so nothing here knows which processes have
    /// ended since the reading. [`scan`] passes that reading in; a caller with only a
    /// table has none to give and answers `ended` for the table as stated.
    ///
    /// **The prune happens before the lineage pass, and that ordering is the rule.** A
    /// scan reads the whole table before any process in it is judged, and a short command
    /// can end in between; a process that has gone is not standing in the home, and it
    /// must not be left anchoring something else to it either.
    #[must_use]
    pub fn sort(
        &self,
        own: Own<'_>,
        placed: &[PathBuf],
        spared: &[u32],
        ended: &dyn Fn(u32) -> bool,
    ) -> (Vec<u32>, Vec<Standing>) {
        let mut certain = Vec::new();
        let mut standing = Vec::new();
        // The processes whose own working directory is in the home, kept apart from the
        // rest of what is standing there. A group leader has to be one of *these* before
        // it vouches for anything ([`descends_from`]).
        let mut by_cwd = BTreeSet::new();
        for process in self.running {
            if owns(process, own) {
                certain.push(process.pid);
                continue;
            }
            if !bystander(process, own, placed, spared) || ended(process.pid) {
                continue;
            }
            if in_one_of(process, placed) {
                by_cwd.insert(process.pid);
            }
            let row = Standing::new(process.pid, process.command.clone());
            standing.push(match held_inside(process, placed) {
                Some(held) => row.holding(held.describe()),
                None => row,
            });
        }
        standing.extend(self.withheld_in(&standing, &by_cwd, spared, ended));
        (certain, standing)
    }
}

/// How far a lineage is followed before a process is called unrelated to everything
/// standing in the home.
///
/// A person's shell, the command they typed and what that started is two or three deep.
/// The bound is here because a lineage is read from a table that was read one process at
/// a time, so a parent that has been replaced can point back down at a child and make a
/// cycle out of two honest readings.
const LINEAGE: usize = 16;

/// The processes this account could not read that a reading of the home cannot rule out.
///
/// **This is the arm a hidden process is judged by.** A process whose `/proc` entry this
/// account may not read — another account's on a shared host, or this account's own
/// running a binary the kernel marks undumpable, which is what a setuid program becomes —
/// used to be left out of the table entirely: no row, no note, nothing for a verdict to
/// rest on. It is now in the table ([`processes::Withheld`]), and this is what a reclaim
/// does with it.
///
/// What it does **not** do is refuse over every one of them. On the machine this was
/// written on, 36 processes are withheld at any moment and 3 of them belong to this
/// account; a rule that refused over unreadability alone would refuse every reclaim on
/// this host, permanently, with nothing a person could do to clear it. "Cannot see,
/// therefore not safe" is a rule about *this home*, not about the machine.
///
/// So the refusal is over the one thing still readable about a process this account is
/// refused: its lineage ([`descends_from`]). One whose lineage reaches nothing in the
/// home is counted in the evidence record and refuses nothing — the residual this
/// reading cannot close, stated rather than hidden.
///
/// **The anchor is what is standing in the home, and never what the unit owns.** A
/// teardown stops the unit's own processes before the move, so anchoring on one would
/// make the preflight refuse over a relation the operation itself dissolves a moment
/// later: the preflight would say refuse and the reclaim would go ahead. A process the
/// teardown reaches is reached through the group the registry recorded, which is the
/// record that exists for exactly that.
impl Table<'_> {
    fn withheld_in(
        &self,
        standing: &[Standing],
        by_cwd: &BTreeSet<u32>,
        spared: &[u32],
        ended: &dyn Fn(u32) -> bool,
    ) -> Vec<Standing> {
        let inside: BTreeSet<u32> = standing.iter().map(|row| row.pid).collect();
        if inside.is_empty() {
            return Vec::new();
        }
        self.withheld
            .iter()
            .filter(|process| !spared.contains(&process.pid))
            .filter(|process| !ended(process.pid))
            .filter(|process| descends_from(process, &inside, by_cwd, &self.lineage))
            .map(|process| Standing::new(process.pid, process.command.clone()).holding(WITHHELD))
            .collect()
    }
}

/// What a refusal says about a process it can name and cannot read.
pub const WITHHELD: &str = "started from inside the home; this account may not read it";

/// Whether a process's lineage says it was started from inside the home.
///
/// Two relations, and each is a fact the kernel publishes about a process this account
/// may not otherwise read. It **descends from** something standing in the home: its
/// parent chain reaches one, bounded by [`LINEAGE`] and by the pids already walked so
/// that two readings taken a moment apart cannot make a cycle that never ends. Or its
/// **group leader** is a process whose own working directory is in the home, which is the
/// command a person typed there and the job it started.
///
/// **A shared session is not occupancy, and matching one was a false refusal.** A shell
/// standing in the home is usually its own session leader, so its identifier is in the
/// set; every other process in that terminal — a `sudo` run an hour ago from another
/// pane, owned by root and standing nowhere near the home — shares the session number and
/// nothing else. Refusing over that told a person their home was occupied by something
/// they could not see and could not find, and the only way out of it is `--force`, which
/// turns off every check in the operation. A rule that sends people to `--force` is worse
/// than the hole it closes. The session is not read here, and
/// [`processes::Lineage`] says so.
///
/// The group is read, and narrowly: the leader has to be standing in the home **by its
/// own working directory**, not merely be something the home refuses over. A process that
/// is in the list only because it holds a descriptor there vouches for nothing, because
/// sharing a process group with it says nothing about where anything was started.
fn descends_from(
    process: &processes::Running,
    inside: &BTreeSet<u32>,
    by_cwd: &BTreeSet<u32>,
    lineage: &BTreeMap<u32, processes::Lineage>,
) -> bool {
    let own = process.lineage;
    if own.group.is_some_and(|group| by_cwd.contains(&group)) {
        return true;
    }
    let mut walked = BTreeSet::from([process.pid]);
    let mut at = own.parent;
    for _ in 0..LINEAGE {
        let Some(pid) = at.filter(|pid| *pid > 1) else { return false };
        if inside.contains(&pid) {
            return true;
        }
        if !walked.insert(pid) {
            return false;
        }
        at = lineage.get(&pid).and_then(|line| line.parent);
    }
    false
}

/// The first thing a process holds inside one of these homes, which is the half of the
/// question [`in_one_of`] does not ask.
///
/// The descriptors it has open for writing, the files it has mapped so that writes reach
/// them, and the root it is held to. This is the half of occupancy a working directory
/// never answered: a test runner started from a terminal that has since changed
/// directory, writing its database into a home, stands nowhere near that home and is
/// holding it open the whole time.
///
/// One walk of the held set, answering both readers of it. The predicate asks whether
/// there is one and the refusal names it, and a version that collected every match
/// allocated a list for a caller that read its first entry — once for every process a
/// listing of forty units refuses over.
///
/// `placed` must already be resolved, for the reason [`bystander`] states.
fn held_inside<'a>(
    process: &'a processes::Running,
    placed: &[PathBuf],
) -> Option<&'a processes::Held> {
    process.held.iter().find(|held| placed.iter().any(|home| held.path.starts_with(home)))
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
        && (in_one_of(process, placed) || held_inside(process, placed).is_some())
        && !vouched_for_by_a_group(process, own)
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
    /// still alive ([`wrappers_of`]). Empty for a caller that took no such reading.
    pub wrappers: &'a [Holding],
}

impl<'a> Own<'a> {
    /// The unit and the groups the registry recorded for it.
    #[must_use]
    pub const fn of(unit: UnitId, groups: &'a [u32]) -> Self {
        Self { unit, groups, wrappers: &[] }
    }

    /// The same, with the `nodal run` each group hangs off already resolved.
    #[must_use]
    pub const fn and_wrappers(mut self, wrappers: &'a [Holding]) -> Self {
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
fn is_still(pid: u32, wrapper: Holding) -> bool {
    pid == wrapper.pid
        && wrapper.started_at.is_some()
        && processes::started_at(pid) == wrapper.started_at
}

/// The `nodal run` each of these groups hangs off, read while the groups are alive.
///
/// Taken by the caller that still can — before anything is stopped — because the relation
/// that identifies the process is the parent of the leader of a recorded group, and a
/// reclaim stops that group before it moves the home. So the relation is resolved once,
/// while it is still readable, and what is carried forward is the answer.
///
/// What is carried is a [`Holding`]: a number and the instant the process wearing it
/// started. A carried identifier on its own is exactly the stale number this module
/// refuses to treat as a name, and the pin is what makes it one again ([`is_still`]).
#[must_use]
pub fn wrappers_of(groups: &[u32]) -> Vec<Holding> {
    groups
        .iter()
        .filter_map(|leader| processes::parent_of(*leader))
        .filter(|pid| *pid > 1)
        .map(|pid| Holding { pid, started_at: processes::started_at(pid) })
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
    /// Every commit nothing proved a copy of whose tree a remote tip already holds, under
    /// another identifier ([`SameContent`]).
    ///
    /// Read only where [`Input::dispositions`] asked for it, and never a reason: it says
    /// what a refusal is about, and it does not answer it.
    #[serde(default)]
    pub content: Vec<SameContent>,
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
    /// What this verdict rests on: what was asked, what answered, and what was not
    /// checked ([`Reading`]).
    ///
    /// **Nothing here changes the verdict.** [`Assessment::safe_to_reclaim`] reads the
    /// reasons and only the reasons, and a reader that gated on a field of this record
    /// would be taking a second opinion where there is one opinion. What it does is make
    /// the first one falsifiable: a safe verdict and a verdict whose evidence fell
    /// outside the predicate used to be byte-identical, and a contract nobody can check
    /// is a slogan.
    #[serde(default)]
    pub reading: Reading,
}

impl Assessment {
    /// What this reading found that a removal of the home would take away.
    ///
    /// The members are cloned rather than borrowed, and that is deliberate: the same type
    /// is what [`kernel::loss_set`] answers a caller which read a directory and holds no
    /// assessment, and one owned shape for both readers is worth more than a borrowed one
    /// here and an owned one there. Every vector in it is a sample of at most [`SAMPLE`]
    /// entries, and one command clones them once per home.
    #[must_use]
    pub fn loss_set(&self) -> LossSet {
        LossSet {
            home: self.home.clone(),
            paths: self.paths.clone(),
            commits: self.commits.clone(),
        }
    }

    /// What this reading took beside the loss set: the occupancy of the home, the set it
    /// goes with, and when it was read.
    ///
    /// `set` is the other homes the same removal takes, and it is empty for a home judged
    /// alone.
    ///
    /// The occupancy is handed over only for a home the removal would move. What stands in a
    /// checkout that is unregistered and left exactly where it is has nothing taken out from
    /// under it, so there is no occupancy question to ask about it, and the kernel is given
    /// none rather than a reading and a second field saying to ignore it.
    #[must_use]
    pub fn evidence(&self, set: Vec<PathBuf>, at: Timestamp) -> Evidence {
        Evidence { runtime: self.moves.then(|| self.runtime.clone()).flatten(), set, read_at: at }
    }

    /// The kernel's answer over this reading, which is the one place a safe verdict is
    /// made.
    ///
    /// Every surface that removes a home, or prints a sentence about whether one is safe,
    /// asks this. It reads nothing and it is the whole of the predicate, so a `nodal
    /// reclaim --check` that says safe and a `nodal reclaim` that refuses cannot both
    /// happen ([`kernel::judge`]).
    #[must_use]
    pub fn verdict(&self, set: Vec<PathBuf>, at: Timestamp) -> Verdict {
        kernel::judge(&self.loss_set(), &self.evidence(set, at))
    }

    /// Fill in the ranked reasons from the kernel's verdict over this reading.
    ///
    /// [`assess`] calls it once, and so does any caller that assembles an assessment from
    /// parts. The reasons are a rendering of the verdict and never a second predicate, so
    /// a reading whose reasons were filled in here cannot say safe where
    /// [`Assessment::verdict`] refuses.
    pub fn ranked(&mut self, at: Timestamp) {
        self.reasons = self.verdict(Vec::new(), at).reasons();
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
            Some(Copies::OnlyHere { witness } | Copies::NotChecked { witness, .. }) => {
                witness.clone()
            }
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
/// **One reading, and one judge of it.** Every path that removes a home asks this function
/// for the reading, and asks [`kernel::judge`] for the answer. A second implementation of
/// "is this safe to delete" is a second answer to a question that has to have one, and the
/// difference between the two is the day somebody loses a morning's work. A refusal asks
/// for the part it rests on and nothing else ([`Input::refusal`]); the read-only preflight
/// asks the same function for the whole of it and prints what a refusal throws away. A
/// `nodal reclaim --check` that says safe and a `nodal reclaim` that refuses therefore
/// cannot both happen: there is one reading and one maker of a [`kernel::Proof`] under
/// both.
///
/// # Errors
/// [`crate::Error::Git`] when the status or a revision could not be read, and
/// [`crate::Error::NotARepository`] when `home` is not one.
pub fn assess(input: &Input<'_>) -> Result<Assessment> {
    let git = Git::open(input.home)?;
    let status = git.status()?;
    let mut paths = working(&status, input.fate);
    let mut notes = Vec::new();
    if input.state {
        paths.extend(ignored(input.home, input.fate, &mut notes));
    }
    let mut reading = Reading::default();
    let (commits, remotes, content) = history(&git, input, &mut notes, &mut reading)?;
    let runtime = input.runtime.map(|asked| running(asked, input.home));
    unread(input, &mut reading);
    let mut assessment = Assessment {
        home: input.home.to_path_buf(),
        moves: input.runtime.is_some_and(|asked| asked.moves),
        remotes,
        commits,
        content,
        paths,
        runtime,
        reasons: Vec::new(),
        notes,
        reading,
    };
    assessment.ranked(Timestamp::now());
    record_table(&mut assessment.reading, assessment.runtime.as_mut(), taken::PREFLIGHT);
    Ok(assessment)
}

/// The readings this assessment was not asked to make, each with the reason it was not.
///
/// Three switches ([`Input`]), each off because the reading it buys costs processes and
/// changes no verdict. That is a good trade and it is invisible in the output, which is
/// the problem this closes: a person cannot tell a group that is empty from a group
/// nobody asked for.
fn unread(input: &Input<'_>, reading: &mut Reading) {
    if !input.state {
        reading.not_checked.push(Unchecked::new(
            "ignored state",
            "not asked for: it is what the trash keeps, and no verdict turns on it",
        ));
    }
    if !input.dispositions {
        reading.not_checked.push(Unchecked::new(
            "where else each commit lives",
            "not asked for: a refusal rests on what nothing proved a copy of, which one reading answers",
        ));
    }
    // The process table's own gap is not pushed here. [`record_table`] owns that line
    // for every caller, because a reading that was asked for and did not answer leaves a
    // gap too and the reason differs; two sites writing it meant one of them was always
    // dead and the two reasons could drift apart.
}

/// What the record calls the process table, in the one place that names it.
///
/// Written once because two sites read it: the gap a reading that did not ask for the
/// table leaves, and the removal of that gap by a reading that did. Two spellings would
/// leave a record that says both that the table was read and that nobody asked for it.
pub const TABLE: &str = "the process table";

/// Say where in an operation the process table was read, and leave the gap a table
/// nobody could read deserves.
///
/// **The counts are not copied anywhere.** They live on the [`Runtime`], which is the
/// value the kernel is handed ([`kernel::Evidence::runtime`]) and the value every report
/// renders; what is set here is the one thing the runtime cannot know about itself, which
/// is which of an operation's readings it is. A reclaim reads the table twice — once
/// before the teardown, for its own refusal, and once in the step that decides whether
/// the home may move — and only the second one gated the rename.
pub fn record_table(reading: &mut Reading, runtime: Option<&mut Runtime>, at: &str) {
    // A reading that was asked for and did not answer is not a reading that was made.
    // The gap stays, and it says which of the two happened, because a record that dropped
    // the gap would describe a table nobody could see as a table with nothing in it.
    reading.not_checked.retain(|gap| gap.what != TABLE);
    let Some(runtime) = runtime else {
        reading.not_checked.push(Unchecked::new(
            TABLE,
            "not asked for: this reading is about the work in the home, not about what is running",
        ));
        return;
    };
    runtime.taken(at);
    if runtime.unread() {
        reading.not_checked.push(Unchecked::new(TABLE, unread_why(runtime)));
    }
}

/// Why a table that was asked for did not answer, in the words the scan gave.
fn unread_why(runtime: &Runtime) -> String {
    runtime
        .notes
        .iter()
        .find(|note| note.unread(Source::Environment))
        .map_or_else(|| String::from("it could not be read"), |note| note.why.clone())
}

/// Where in a reading the process table was read, for the record beside it.
pub mod taken {
    /// By `nodal reclaim --check`, or by the reading an operation refuses on.
    pub const PREFLIGHT: &str = "the preflight";
    /// By the step that refuses to move a home somebody is standing in, which is the
    /// reading that decided the rename.
    pub const BEFORE_THE_MOVE: &str = "before the move";
    /// Before the teardown, by an operation that never reached the move.
    pub const BEFORE_THE_TEARDOWN: &str = "before the teardown";
}

// ---------------------------------------------------------------------------
// The working tree.
// ---------------------------------------------------------------------------

/// The paths of the working tree that carry work, in the two kinds they come in.
///
/// Visible to the kernel, which is where [`kernel::loss_set`] reads the loss set of a
/// directory for a caller that holds no assessment. One reader of a `git status`, so the
/// two entries cannot group one status two ways.
pub(crate) fn working(status: &Summary, fate: Fate) -> Vec<PathGroup> {
    let tracked =
        paths_where(status, |entry| matches!(entry.state, State::Tracked { .. } | State::Unmerged));
    let untracked = paths_where(status, |entry| entry.state == State::Untracked);
    [group(Held::Uncommitted, fate, tracked), group(Held::Untracked, fate, untracked)]
        .into_iter()
        .flatten()
        .collect()
}

/// One group over a list of paths, or nothing when the list is empty.
fn group(held: Held, fate: Fate, paths: Vec<PathBuf>) -> Option<PathGroup> {
    if paths.is_empty() {
        return None;
    }
    let count = paths.len();
    Some(PathGroup {
        held,
        fate,
        count,
        sample: paths.into_iter().take(SAMPLE).collect(),
        bytes: None,
    })
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
fn ignored(home: &Path, fate: Fate, notes: &mut Vec<String>) -> Vec<PathGroup> {
    let surveyed = prune::survey(home);
    notes.extend(surveyed.notes);
    let (generated, local): (Vec<prune::Candidate>, Vec<prune::Candidate>) =
        surveyed.candidates.into_iter().partition(|candidate| candidate.reason.is_some());
    [measured(Held::Generated, fate, generated), measured(Held::LocalState, fate, local)]
        .into_iter()
        .flatten()
        .collect()
}

/// One group over classified paths, with what they hold, or nothing when there are none.
fn measured(held: Held, fate: Fate, candidates: Vec<prune::Candidate>) -> Option<PathGroup> {
    if candidates.is_empty() {
        return None;
    }
    let apparent = candidates.iter().map(|candidate| candidate.bytes).sum();
    let count = candidates.len();
    let sample = candidates.into_iter().take(SAMPLE).map(|candidate| candidate.path).collect();
    Some(PathGroup { held, fate, count, sample, bytes: Some(Bytes::of(apparent, true)) })
}

// ---------------------------------------------------------------------------
// The commits.
// ---------------------------------------------------------------------------

/// The home's own commits, grouped by where else they live, the remotes it names, and
/// every refused commit whose content a remote tip already holds.
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
fn history(
    git: &Git,
    input: &Input<'_>,
    notes: &mut Vec<String>,
    reading: &mut Reading,
) -> Result<(Vec<CommitGroup>, Vec<String>, Vec<SameContent>)> {
    let remotes = git.remotes()?;
    let found = witness::elsewhere(input.home, input.checkout);
    let checkout = input.checkout.map(Checkout::path);
    let work = input.work.resolve(git)?;
    reading.refs.walked = work.walked.clone();
    reading.refs.not_walked = work.not_walked.clone();
    if !input.dispositions {
        let refused = refusing(git, input, &work, &found, &remotes, reading)?;
        return Ok((refused, remotes, Vec::new()));
    }
    let ours = git.among_outside(&work.tips, &found.own)?;
    reading.refs.commits = ours.len();
    if ours.is_empty() {
        reading.stores = unasked(input, NOTHING_TO_LOOK_FOR);
        return Ok((Vec::new(), remotes, Vec::new()));
    }
    let witness = Witness::of(&remotes, &found);
    let off_remote = git.among_outside(&work.tips, &union(&found.own, &found.remote))?;
    let unproved = git.among_outside(&work.tips, &found.tips())?;
    let proved = difference(&ours, &off_remote);
    let second = difference(&off_remote, &unproved);
    let found = local_copies(input.home, checkout, input.siblings, unproved, reading);
    let content = same_content(git, &found.only, notes);
    let mut groups = Vec::new();
    groups.extend(commit_group(Copies::RemoteProved { witness: witness.clone() }, proved));
    groups.extend(second_groups(checkout, second, found.held));
    groups.extend(commit_group(unchecked(&witness, found.unread), found.unchecked));
    groups.extend(commit_group(unreached(&witness), found.only));
    Ok((groups, remotes, content))
}

/// The namespaces a remote tip is read out of, inside the home.
///
/// `refs/nodal/origin/` is the mirror a create writes into every home, and
/// `refs/remotes/` is whatever the home itself fetched. Both are readings of a remote and
/// neither is proof about one, which is the whole reason this row proves nothing on its
/// own: what it says is that the tree is already written down under a name out here, not
/// that any server still holds it.
const REMOTE_NAMESPACES: [&str; 2] = [crate::git::refs::ORIGIN, "refs/remotes/"];

/// Every commit of `kept` whose tree a remote tip already holds.
///
/// `kept` is the whole refused set and not its sample, because a home of fifty only-here
/// commits has fifty commits a rewritten history could have rewritten.
///
/// Two `rev-parse` runs, whatever the size of either list: one for the tips, one for the
/// commits. A home with nothing refused reads neither.
///
/// A reading that fails is a note and never a row. This says what a refusal is about and
/// it does not answer one, so a reading nobody could take leaves the refusal exactly as
/// it was.
fn same_content(git: &Git, kept: &[Oid], notes: &mut Vec<String>) -> Vec<SameContent> {
    if kept.is_empty() {
        return Vec::new();
    }
    match rewritten(git, kept) {
        Ok(rows) => rows,
        Err(why) => {
            notes.push(format!("the trees of this home's commits could not be read: {why}"));
            Vec::new()
        }
    }
}

/// The rows themselves, for a caller that has the commits and wants the failure.
fn rewritten(git: &Git, kept: &[Oid]) -> Result<Vec<SameContent>> {
    let mut tips: Vec<crate::git::refs::Ref> = Vec::new();
    for namespace in REMOTE_NAMESPACES {
        tips.extend(git.list_refs(namespace)?);
    }
    if tips.is_empty() {
        return Ok(Vec::new());
    }
    let tip_oids: Vec<Oid> = tips.iter().map(|one| one.oid.clone()).collect();
    let by_tree: BTreeMap<Oid, &crate::git::refs::Ref> =
        git.trees_of(&tip_oids)?.into_iter().zip(tips.iter()).collect();
    Ok(git
        .trees_of(kept)?
        .into_iter()
        .zip(kept.iter())
        .filter_map(|(tree, commit)| {
            let held = by_tree.get(&tree)?;
            Some(SameContent {
                commit: commit.clone(),
                reference: held.name.clone(),
                tip: held.oid.clone(),
                tree,
            })
        })
        .collect())
}

/// The groups a destructive path acts on, and the stores that made it act on fewer.
///
/// One `rev-list` and not three. A refusal is raised over [`Copies::OnlyHere`] and
/// [`Copies::NotChecked`] and over nothing else, and both are drawn from one reading —
/// the commits of `HEAD` that no tip this machine found reaches. What this path gives up
/// is the split between a commit a witnessed reading of the remote proves and one held a
/// second time on this disk: each of those costs a reading, and [`Input::dispositions`]
/// is the caller saying whether it wants them.
///
/// The [`Copies::SecondLocalCopy`] groups [`local_copies`] found are reported and not
/// thrown away, and that is what makes one joint rule serve both callers
/// ([`kernel::joint`]). They carry no [`Copies::needs`], so [`kernel::judge`] raises no loss
/// over them on its own and [`Assessment::findings`] drops them: the verdict on this path is
/// the verdict it always was. What they add is the name of the store, which is the whole of
/// what a joint question needs and the one thing a reading that discarded them could not
/// supply.
fn refusing(
    git: &Git,
    input: &Input<'_>,
    work: &Walked,
    found: &witness::Elsewhere,
    remotes: &[String],
    reading: &mut Reading,
) -> Result<Vec<CommitGroup>> {
    let unproved = git.among_outside(&work.tips, &found.tips())?;
    reading.refs.commits = unproved.len();
    if unproved.is_empty() {
        reading.stores = unasked(input, NOTHING_TO_LOOK_FOR);
        return Ok(Vec::new());
    }
    let checkout = input.checkout.map(Checkout::path);
    let read = local_copies(input.home, checkout, input.siblings, unproved, reading);
    let witness = Witness::of(remotes, found);
    let mut groups = held_groups(read.held);
    groups.extend(commit_group(unchecked(&witness, read.unread), read.unchecked));
    groups.extend(commit_group(unreached(&witness), read.only));
    Ok(groups)
}

/// How the commits nothing proved are reported: as only here, or as not checked.
///
/// The two are one refusal and they are not one claim. A home whose remote question was
/// settled — there is no remote, or the remote is on this disk and was read — holds the
/// only copy, and the report says so. A home nothing could check may hold the only copy,
/// and the report says that instead. Calling the second the first would be a claim this
/// machine did not earn.
fn unreached(witness: &Witness) -> Copies {
    if witness.unchecked() {
        return Copies::NotChecked { witness: witness.clone(), stores: Vec::new() };
    }
    Copies::OnlyHere { witness: witness.clone() }
}

/// How the commits a store holds and could not vouch for are reported.
///
/// Never as only here, whatever the remote reading said. A directory on this disk has
/// these commits, so "only here" would be a claim this machine did not earn; and it could
/// not be proved to hold the work, so a second copy would be the false safe the guard
/// exists to remove. What is true is that nobody checked, and the stores are named on the
/// group so the report can say which directory and which property.
fn unchecked(witness: &Witness, stores: Vec<Incomplete>) -> Copies {
    Copies::NotChecked { witness: witness.clone(), stores }
}

/// Split the commits no remote reading proved into the ones another object store holds
/// anyway and the ones nothing does.
///
/// A checkout that cannot be read, or that will not answer, holds nothing as far as this
/// is concerned, which is the strict direction.
///
/// **The home being read is never one of the stores.** It turns up in the list on its
/// own account: [`crate::doctor::scan::siblings`] walks the checkout's parent and a
/// checkout adopted in place sits there, so the home is handed its own path back as a
/// repository that may hold a second copy. It holds every one of them, because they are
/// its own commits, and believing it would answer "the second copy is in this very
/// directory" about the directory the removal takes. Both sides are resolved
/// ([`paths::resolve`]), because one directory reached through a symbolic link and
/// reached directly is one directory with two spellings.
fn local_copies(
    home: &Path,
    checkout: Option<&Path>,
    siblings: &[PathBuf],
    unproved: Vec<Oid>,
    reading: &mut Reading,
) -> Found {
    let itself = paths::resolve(home);
    let mut left = unproved;
    let mut held = Vec::new();
    let mut doubtful: Vec<(Incomplete, BTreeSet<Oid>)> = Vec::new();
    for (store, role) in stores(checkout, siblings) {
        if left.is_empty() {
            reading.stores.push(asked(store, role, Answered::NotAsked, ACCOUNTED_FOR));
            continue;
        }
        if paths::resolve(store) == itself {
            reading.stores.push(asked(store, role, Answered::NotAsked, ITSELF));
            continue;
        }
        if let Some(lacking) = complete::admits(store, home) {
            doubtful.extend(doubted(store, lacking, &left, reading, role));
            continue;
        }
        let Some(holds) = holds_of(store, &left) else {
            reading.stores.push(asked(store, role, Answered::No, UNREADABLE));
            continue;
        };
        if holds.is_empty() {
            reading.stores.push(asked(store, role, Answered::Yes, ""));
            continue;
        }
        let claimed: Vec<Oid> = left.iter().filter(|oid| holds.contains(*oid)).cloned().collect();
        if let Some(lacking) = incomplete(store, home, &claimed) {
            doubtful.extend(doubted(store, lacking, &left, reading, role));
            continue;
        }
        reading.stores.push(asked(store, role, Answered::Yes, ""));
        let (found, rest) = left.into_iter().partition(|oid| holds.contains(oid));
        held.push((store.to_path_buf(), found));
        left = rest;
    }
    let unread: Vec<Incomplete> = doubtful
        .iter()
        .filter(|(_, commits)| left.iter().any(|oid| commits.contains(oid)))
        .map(|(store, _)| store.clone())
        .collect();
    let doubted: BTreeSet<&Oid> =
        doubtful.iter().flat_map(|(_, commits)| commits.iter()).collect();
    let (unchecked, only): (Vec<Oid>, Vec<Oid>) =
        left.into_iter().partition(|oid| doubted.contains(oid));
    Found { held, unread, unchecked, only }
}

/// What every store on this machine answered about the commits nothing else proved.
struct Found {
    /// Each store that holds a commit and was proved to hold the work behind it.
    held: Vec<(PathBuf, Vec<Oid>)>,
    /// Each store that holds a commit and could not be proved to hold the work.
    unread: Vec<Incomplete>,
    /// The commits only those stores have, which are the ones nobody checked.
    unchecked: Vec<Oid>,
    /// The commits no store on this machine has at all.
    only: Vec<Oid>,
}

/// Record a store that may not vouch, and say which of the commits it has anyway.
///
/// The commits are read out of it even though it proves nothing, and that is the whole
/// point of the reading. A store that holds a commit and cannot produce the work is a
/// different answer from a store that never had it, and a person owed a refusal is owed
/// the one that names a directory on their own disk.
///
/// The commits are not taken out of what is left, so a store later in the order may still
/// prove them. First hit wins among the stores that can vouch, and a store that cannot
/// never takes a commit away from one that can.
fn doubted(
    store: &Path,
    lacking: Lacking,
    left: &[Oid],
    reading: &mut Reading,
    role: reading::Role,
) -> Option<(Incomplete, BTreeSet<Oid>)> {
    let incomplete = Incomplete { store: store.to_path_buf(), lacking };
    reading.stores.push(asked(store, role, Answered::No, &incomplete.lacking.because()));
    let has: BTreeSet<Oid> =
        Git::at(store).stores(left).unwrap_or_default().into_iter().collect();
    (!has.is_empty()).then_some((incomplete, has))
}

/// Whether a store that admits nothing against itself is missing an object anyway.
///
/// The dear half of the reading, and it is taken only of a store that passed the cheap
/// half and holds something. `boundary_of` is asked in the home, because the home is the
/// repository that has the history these commits sit on; the walk is then made in the
/// store, over what those commits add and nothing more.
///
/// A reading that would not run leaves the store unchecked rather than trusted.
fn incomplete(store: &Path, home: &Path, claimed: &[Oid]) -> Option<Lacking> {
    let Ok(boundary) = Git::at(home).boundary_of(claimed) else {
        return Some(Lacking::Unreadable);
    };
    match complete::missing(store, claimed, &boundary) {
        Some(0) => None,
        Some(objects) => Some(Lacking::Missing { objects }),
        None => Some(Lacking::Unreadable),
    }
}

/// Why a store was not asked: every commit was already accounted for by the time its
/// turn came, so opening it would have cost a process and changed nothing.
const ACCOUNTED_FOR: &str = "every commit was already accounted for";

/// Why a store was not asked: it is the home being read, which is never a second copy of
/// itself.
const ITSELF: &str = "this is the home being read";

/// Why a store did not answer.
const UNREADABLE: &str = "not a readable repository, or its own reading failed";

/// Why no store was asked at all: the home holds no commit that needs a copy found.
const NOTHING_TO_LOOK_FOR: &str = "the home holds no commit the project does not already reach";

/// Every store a reading may ask, in the order it asks them, each with what it is.
fn stores<'a>(
    checkout: Option<&'a Path>,
    siblings: &'a [PathBuf],
) -> impl Iterator<Item = (&'a Path, reading::Role)> {
    checkout
        .into_iter()
        .map(|path| (path, reading::Role::Checkout))
        .chain(siblings.iter().map(|path| (path.as_path(), reading::Role::Sibling)))
}

/// One store's row of the evidence record.
fn asked(path: &Path, role: reading::Role, answered: Answered, why: &str) -> Store {
    let why = (!why.is_empty()).then(|| String::from(why));
    Store { path: path.to_path_buf(), role, answered, why }
}

/// Every store, each recorded as not asked for one reason.
///
/// The reading that took no commits to any store still asked the question, and a record
/// with an empty store list would read as a machine with no stores on it.
fn unasked(input: &Input<'_>, why: &str) -> Vec<Store> {
    stores(input.checkout.map(Checkout::path), input.siblings)
        .map(|(path, role)| asked(path, role, Answered::NotAsked, why))
        .collect()
}

/// Which of these commits one repository really holds.
///
/// Holds means a ref of that repository reaches the commit, and not that the object is
/// in its store ([`crate::git::Git::held`]). An object under no ref is what `git gc`
/// removes, so counting it would call this home safe over a copy one ordinary command
/// takes away.
///
/// A repository that will not open, and a `rev-list` that would not run, both answer
/// with nothing. That is the stricter reading and it is the safe direction: a store
/// nobody could read has proved no second copy of anything, and the commit stays in the
/// group a refusal is raised over.
fn holds_of(store: &Path, commits: &[Oid]) -> Option<BTreeSet<Oid>> {
    let git = Git::open(store).ok()?;
    git.owned(commits).map(|held| held.into_iter().collect()).ok()
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
    held_groups(held)
}

/// One group per object store that was found to hold a second copy.
///
/// Both paths through [`history`] end here, so the store a group names is the same fact
/// whichever reading produced it and [`together`] can ask one question of either.
fn held_groups(held: Vec<(PathBuf, Vec<Oid>)>) -> Vec<CommitGroup> {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use std::collections::BTreeMap;

    use super::{
        Assessment, CommitGroup, Copies, Fate, Held, Holding, Needs, Own, PathGroup, Reason,
        Timestamp, bystander, owns, processes,
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
    /// A process that a live wrapper started is vouched for by the same carried reading:
    /// it is the `git` a wrapper runs to record its run after the group ends.
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
        let asked = |wrappers: &[Holding]| {
            bystander(
                &standing,
                Own::of(unit, &[]).and_wrappers(wrappers),
                std::slice::from_ref(&home),
                &[],
            )
        };

        let started_at = processes::started_at(pid).unwrap();

        // No group to read: the reclaim stopped it. The carried answer is what is left.
        assert!(
            !asked(&[Holding { pid, started_at: Some(started_at) }]),
            "the wrapper is not a stranger once its group has gone"
        );

        // The instant is the whole of the proof. An identifier that came round again
        // belongs to a process that started later, so it matches nothing.
        let earlier = Timestamp::from_unix_seconds(started_at.unix_seconds() - 60).ok();
        assert!(
            asked(&[Holding { pid, started_at: earlier }]),
            "a number that came round again proves nothing"
        );

        // And an undated reading is not evidence either.
        assert!(asked(&[Holding { pid, started_at: None }]), "an undated reading is not evidence");

        // What the wrapper started is its own too, while the wrapper is the one read. The
        // process that started this test stands in for the wrapper, and this test for the
        // process it started.
        let parent = crate::runtime::processes::parent_of(pid).unwrap();
        let carried = [Holding { pid: parent, started_at: processes::started_at(parent) }];
        let vouched = !asked(&carried);
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
        assessment.ranked(at());
        assessment
    }

    /// The instant every reading in these properties was taken. Fixed, because a property
    /// about a verdict must not depend on the clock.
    fn at() -> Timestamp {
        Timestamp::parse("2026-09-23T09:00:00Z").unwrap()
    }

    /// Whether a reclaim of this reading would go ahead, asked of the one judge.
    fn goes_ahead(assessment: &Assessment) -> bool {
        assessment.verdict(Vec::new(), at()).safe()
    }

    /// One commit group of one commit.
    fn commits(copies: Copies) -> CommitGroup {
        CommitGroup { copies, count: 1, sample: vec![oid("ab")] }
    }

    /// One path group of one path.
    fn paths(held: Held) -> PathGroup {
        PathGroup {
            held,
            fate: Fate::Trashed,
            count: 1,
            sample: vec![PathBuf::from("a.rs")],
            bytes: None,
        }
    }

    /// The two dispositions that mean the work survives are the two that let a reclaim
    /// go ahead. Anything else keeps the home, including the one that means "I do not
    /// know", because unknown is not safe evidence.
    #[test]
    fn only_a_second_copy_or_a_proved_remote_lets_a_reclaim_go_ahead() {
        let checkout = PathBuf::from("/w/project");
        let by = vec![checkout.clone()];
        for safe in [
            Copies::SecondLocalCopy { held_by: checkout },
            Copies::RemoteProved { witness: Witness::Checked { by: by.clone() } },
        ] {
            assert!(safe.survives(), "{safe:?}");
            assert!(goes_ahead(&assessed(vec![commits(safe)], Vec::new())));
        }
        for kept in [
            Copies::OnlyHere { witness: Witness::NoRemote },
            Copies::not_checked(Witness::Unchecked),
        ] {
            assert!(!kept.survives(), "{kept:?}");
            assert!(!goes_ahead(&assessed(vec![commits(kept)], Vec::new())));
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
        moving.ranked(at());
        assert!(!goes_ahead(&moving));
        assert_eq!(moving.verdict(Vec::new(), at()).top(), Needs::UnknownEvidence);
        assert_eq!(
            moving.reasons[0].detail,
            "the process table could not be read: a process scan reads /proc, which macos \
             does not have"
        );

        let mut in_place = moving.clone();
        in_place.moves = false;
        in_place.ranked(at());
        assert!(goes_ahead(&in_place), "{:?}", in_place.reasons);
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
            super::Runtime {
                notes: vec![Note::part(Source::Environment, crate::runtime::attribute::RESTRICTED)],
                ..super::Runtime::default()
            },
        ];
        for runtime in readings {
            let mut moving = assessed(Vec::new(), Vec::new());
            moving.runtime = Some(runtime.clone());
            moving.ranked(at());
            assert_eq!(goes_ahead(&moving), super::unmovable(&runtime).is_none(), "{runtime:?}");
        }
    }

    /// A scan that ran over part of the table is a reading. What the host refused to show
    /// is a note, and it is not the unread table: a home still moves over it.
    #[test]
    fn a_table_read_in_part_is_not_an_unread_table() {
        use crate::runtime::attribute::{ANOTHER_ACCOUNT, Note, Source};

        let runtime = super::Runtime {
            notes: vec![
                Note::part(Source::Environment, ANOTHER_ACCOUNT),
                Note::part(Source::Cwd, ANOTHER_ACCOUNT),
            ],
            ..super::Runtime::default()
        };
        assert_eq!(super::unmovable(&runtime), None);
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
            vec![commits(Copies::not_checked(Witness::Unchecked))],
            vec![paths(Held::Uncommitted)],
        );
        assert_eq!(assessment.verdict(Vec::new(), at()).top(), Needs::UniqueLoss);
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
        assert!(goes_ahead(&assessed(Vec::new(), vec![paths(Held::LocalState)])));
        assert!(!goes_ahead(&assessed(Vec::new(), vec![paths(Held::Untracked)])));
    }

    /// A refusal reading makes no group whose sentence depends on the fate, which is
    /// what lets [`Input::refusal`] state one without knowing the answer.
    ///
    /// The two dispositions that read the fate come only from the ignored state, and a
    /// refusal reading does not ask for it. This asserts the half that is a property of
    /// the type: the sentence of each of the other two is the same either way. The other
    /// half is `state: false`, which the constructor writes and the compiler holds.
    #[test]
    fn a_refusal_reading_makes_no_group_whose_sentence_depends_on_the_fate() {
        for held in [Held::Uncommitted, Held::Untracked] {
            assert_eq!(
                held.why(Fate::Trashed),
                held.why(Fate::LeftInPlace),
                "{held:?} is a disposition a refusal reading makes, so it must not read the fate"
            );
        }
        for held in [Held::LocalState, Held::Generated] {
            assert_ne!(
                held.why(Fate::Trashed),
                held.why(Fate::LeftInPlace),
                "{held:?} says what becomes of the path, which a home that moves and a home \
                 that does not answer differently"
            );
        }
    }

    /// Every disposition says why it is that disposition. A report that allowed a
    /// removal without a reason would be asking to be believed.
    #[test]
    fn every_disposition_gives_a_reason() {
        for held in [Held::Uncommitted, Held::Untracked, Held::LocalState, Held::Generated] {
            for fate in [Fate::Trashed, Fate::LeftInPlace] {
                assert!(!held.why(fate).is_empty(), "{held:?} {fate:?}");
            }
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
                commits(Copies::not_checked(Witness::Unchecked)),
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
        moving.ranked(at());
        assert_eq!(moving.verdict(Vec::new(), at()).top(), Needs::BlockingRuntime);
        assert!(!goes_ahead(&moving));

        let mut in_place = moving.clone();
        in_place.moves = false;
        in_place.ranked(at());
        assert!(goes_ahead(&in_place), "{:?}", in_place.reasons);
    }

    /// A reason always names what it is about. The rank alone is not an answer.
    #[test]
    fn a_reason_names_what_it_is_about() {
        let reason = Reason::new(Needs::UniqueLoss, "untracked files (1)");
        assert!(!reason.detail.is_empty());
        assert_eq!(reason.needs.label(), "unique loss");
    }
}
