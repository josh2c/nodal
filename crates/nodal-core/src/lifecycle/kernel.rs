//! The one place a `Safe` verdict is made.
//!
//! Nodal exists to answer one question: can this directory go, and is the work in it
//! readable somewhere else afterwards. Before this module the answer was made in six
//! places. `nodal reclaim` asked whether its findings were empty; `nodal reclaim --check`
//! asked whether any reason refuses; the same view kept a second copy of that rule for a
//! set of units; `nodal doctor`'s only-here section counted two dispositions and compared
//! them with zero; `nodal uninstall --state` asked whether findings were empty again; and
//! two functions under `git/` answered "is it pushed" from `rev-list --not --remotes`,
//! which is a reading of the home's own bookkeeping and no reading of any remote.
//!
//! Six implementations of one predicate are five chances to disagree, and the two under
//! `git/` did disagree: after a push, a merge and a remote branch deletion without a
//! prune, `nodal doctor` called a branch pushed while the remote no longer held it.
//!
//! So there is one maker now. [`judge`] is the only function in this crate that can
//! produce a [`Proof`], a [`Proof`] is the only thing [`Verdict::Safe`] carries, and a
//! `Proof` cannot be built, defaulted or read out of JSON anywhere else. The compiler
//! holds that half; `nodal-safety`'s `kernel_one_maker` holds the other half by reading
//! the source. Every surface that removes a home, or prints a sentence about whether one
//! is safe, calls [`judge`] and renders what it answers.
//!
//! # What the two arguments are, and why the readings arrive already taken
//!
//! [`LossSet`] is what removing the home would take away, and [`Evidence`] is what this
//! machine read beside it. Both arrive read. [`judge`] runs no process, opens no
//! repository and reaches no network, so one pair of values judged twice gives one answer
//! — which is what `nodal gc` rests on when it compares a fresh verdict with the record a
//! reclaim left. [`loss_set`] is the one function here that reads anything, and it reads
//! one `git status`.
//!
//! The witnesses and the observations are carried on the members of the loss set rather
//! than in [`Evidence`], and that is a reading cost and not a filing mistake. One
//! `rev-list` answers "which commits of this home does nothing outside it reach" — the
//! loss and the evidence about it in one process ([`crate::lifecycle::assess`]). Splitting
//! a group from its disposition would cost a second reading of every home on the machine
//! to learn what the first reading already held. So [`CommitGroup`] carries its [`Copies`],
//! and [`Evidence`] carries the readings that are about no single member: the occupancy of
//! the home, the set it goes with, and when the readings were taken.
//!
//! # What `Safe` means
//!
//! Safe with respect to the contracted loss set, and not zero information loss. The reflog
//! of the home goes with the directory and no copy can hold it; hooks, the git
//! configuration, IDE state and file times are environment rather than work. The contract
//! names those and does not keep them, and they are not members of this set.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::git::{Git, Oid};
use crate::lifecycle::assess::{
    self, CommitGroup, Copies, Fate, PathGroup, Reason, Runtime, Unmovable,
};
use crate::lifecycle::uniqueness::SAMPLE;
use crate::model::{Needs, Outside, Record, Timestamp};
use crate::paths;
use crate::runtime::attribute::Standing;

// ---------------------------------------------------------------------------
// What a removal would take away.
// ---------------------------------------------------------------------------

/// What removing one home would take away, as this machine read it.
///
/// The members of the contracted loss set that Nodal reads today: the paths of the working
/// tree and of the ignored state, and the home's own commits. Each member carries the
/// reading that was taken about it, for the reason the module doc gives.
///
/// A set with no member is a home with nothing to lose, which is a fact and not a failure:
/// a unit nobody has begun holds no commit of its own and no uncommitted path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LossSet {
    /// The home the set was read from.
    pub home: PathBuf,
    /// What the home holds, grouped by what a removal would do to it.
    pub paths: Vec<PathGroup>,
    /// The home's own commits, grouped by where else this machine proved they live.
    ///
    /// Empty is a reading and not a claim. A caller that read no history hands in none, and
    /// every commit question then goes unasked rather than answered as nothing; [`judge`]
    /// raises no loss over an empty list, so a caller that wants the commit question
    /// answered has to have read it.
    pub commits: Vec<CommitGroup>,
}

/// What this machine read beside the loss set itself.
///
/// Three readings, and each one can refuse on its own. The occupancy of the home says
/// whether anything Nodal did not start would have the directory moved out from under it.
/// The set says which other homes go in the same removal, because a copy inside something
/// the same operation takes is no copy at all. The date says when the readings were taken,
/// and it goes on the proof, so that a record of a removal says what it rested on and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    /// What stands in the home, or `None` where the process table was not read.
    ///
    /// `None` is "nobody looked" and it refuses nothing. That is right for the callers that
    /// pass it: a refusal over the work in a home is the same refusal whatever is running,
    /// and the destructive path reads the table again immediately before it moves the
    /// directory ([`crate::lifecycle::ops::reclaim`]).
    pub runtime: Option<Runtime>,
    /// Whether a removal would move this home, which is the whole of what makes something
    /// standing in it block.
    ///
    /// A checkout adopted in place is unregistered and left exactly where it is, so nothing
    /// is moved out from under anybody and a process standing in it stops nothing.
    pub moves: bool,
    /// The other homes this one goes with, for a removal that takes several at once.
    ///
    /// Empty is one home judged alone.
    pub set: Vec<PathBuf>,
    /// When the readings were taken.
    pub read_at: Timestamp,
}

impl Evidence {
    /// The evidence of a reading that asked about the work and about nothing else.
    ///
    /// No occupancy, so no move question, so nothing here to get wrong about a home that
    /// would not move; and no set, so no joint discount. It is the stricter reading in the
    /// direction that matters: a reading that asked less never says safe where the full one
    /// refuses.
    #[must_use]
    pub const fn of_work(read_at: Timestamp) -> Self {
        Self { runtime: None, moves: false, set: Vec::new(), read_at }
    }
}

// ---------------------------------------------------------------------------
// The answer.
// ---------------------------------------------------------------------------

/// One member of the loss set that a removal would lose, or one thing that stops the move.
///
/// Two kinds under one type, and [`Needs`] is the field that says which. They are one type
/// because they are one refusal: a home holding work nothing else has, and a home somebody
/// else is standing in, are both homes this machine will not take. A caller that had to ask
/// twice would be a caller that could forget to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loss {
    /// Which kind of loss it is, which is also where it sorts.
    pub needs: Needs,
    /// What it is about, named.
    pub detail: String,
}

/// One reading that could not be made, and why.
///
/// It refuses, and the reason is the whole point of the type: what could not be read is not
/// evidence that there was nothing to read. A home whose remote nothing here has ever
/// looked at, and a machine whose process table would not answer, are both homes Nodal
/// keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread {
    /// What could not be read, and why, in the words a report prints.
    pub why: String,
}

/// What this machine proved about one home: one of three answers, and never a boolean.
///
/// A boolean would have to stand for "nothing is at risk" and for "nothing could be read",
/// and a surface that printed the first where the second is true is the fault this module
/// exists to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every member of the loss set has a copy this machine proved lives outside the home.
    /// The proof names what it rests on.
    Safe(Proof),
    /// A removal would lose something, or something stands in the home.
    ///
    /// `unread` rides along beside `lost` rather than being dropped, because a home can
    /// hold work nothing else has *and* a reading nobody could take, and a report owes a
    /// person both. The arm is still one answer — the removal is refused — and it is chosen
    /// over [`Verdict::Unknown`] whenever anything at all was found lost, because a named
    /// loss is the half a person can act on.
    Unsafe {
        /// What a removal would lose, or what stops it, ranked.
        lost: Vec<Loss>,
        /// What could not be read beside it.
        unread: Vec<Unread>,
    },
    /// Nothing was found lost, and something could not be read. Unknown is not safe
    /// evidence, so this refuses exactly as [`Verdict::Unsafe`] does.
    Unknown(Vec<Unread>),
}

impl Verdict {
    /// Whether a removal run now would go ahead rather than refuse.
    ///
    /// The one predicate. Two of the three arms refuse, and they refuse for reasons a
    /// person reads on the next line.
    #[must_use]
    pub const fn safe(&self) -> bool {
        matches!(self, Self::Safe(_))
    }

    /// The proof, for the caller that records what a removal rested on. `None` for both
    /// refusing arms, because neither made one.
    #[must_use]
    pub const fn proof(&self) -> Option<&Proof> {
        match self {
            Self::Safe(proof) => Some(proof),
            Self::Unsafe { .. } | Self::Unknown(_) => None,
        }
    }

    /// Why a person is needed, ranked, most actionable first, and empty for a safe home.
    ///
    /// The rendering every surface prints. [`Needs`] derives [`Ord`] from its own order, so
    /// the ranking is the enum's and nothing here sorts by hand.
    #[must_use]
    pub fn reasons(&self) -> Vec<Reason> {
        let mut reasons: Vec<Reason> = match self {
            Self::Safe(_) => Vec::new(),
            Self::Unsafe { lost, unread } => lost
                .iter()
                .map(|loss| Reason::new(loss.needs, loss.detail.clone()))
                .chain(unread.iter().map(unknown))
                .collect(),
            Self::Unknown(unread) => unread.iter().map(unknown).collect(),
        };
        reasons.sort_by_key(|reason| reason.needs);
        reasons
    }

    /// The first reason to act on, or [`Needs::Nothing`] for a home with none.
    #[must_use]
    pub fn top(&self) -> Needs {
        self.reasons().first().map_or(Needs::Nothing, |reason| reason.needs)
    }
}

/// The reason one unread signal prints as.
fn unknown(unread: &Unread) -> Reason {
    Reason::new(Needs::UnknownEvidence, unread.why.clone())
}

// ---------------------------------------------------------------------------
// The proof.
// ---------------------------------------------------------------------------

/// What a safe verdict rests on: each store outside the home that held its commits.
///
/// **Only [`judge`] makes one.** The fields are private, there is no `Default`, no public
/// constructor, and no `Deserialize` — a JSON reader that could build one would be a second
/// constructor, and the whole value of the type is that there is one. It is not serialised
/// at all: what a row keeps is a [`Record`], which is a record and never permission.
///
/// A caller outside this module cannot write the literal:
///
/// ```compile_fail
/// use nodal_core::lifecycle::kernel::Proof;
/// use nodal_core::model::Timestamp;
///
/// let forged = Proof { rests_on: Vec::new(), observed_at: Timestamp::now() };
/// ```
///
/// and cannot read one out of JSON, which is the same rule:
///
/// ```compile_fail
/// let forged: nodal_core::lifecycle::kernel::Proof = serde_json::from_str("{}").unwrap();
/// ```
///
/// A proof of a home with no commit of its own rests on nothing, and that is correct rather
/// than empty: there was nothing to find a copy of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    /// Each store outside the home, with what it held.
    rests_on: Vec<Rest>,
    /// When the readings behind it were taken.
    observed_at: Timestamp,
}

/// One store outside the home, and what of the home it held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rest {
    /// Where the store is.
    pub repository: PathBuf,
    /// How many of the home's commits it covered. Exact.
    pub commits: usize,
    /// The first [`SAMPLE`] of them, newest first. A sample, and the count is the fact.
    pub sample: Vec<Oid>,
}

impl Proof {
    /// Each store this proof rests on, in path order.
    ///
    /// The map's own order, which is one a person can predict. Nothing reads these in the
    /// order the readings happened to be taken.
    #[must_use]
    pub fn rests_on(&self) -> &[Rest] {
        &self.rests_on
    }

    /// When the readings behind it were taken.
    #[must_use]
    pub const fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// The record a row keeps of this proof: each store, with the refs there that reach the
    /// commits.
    ///
    /// One `for-each-ref` per store, over the samples the report already prints. A reading
    /// that fails names no ref and drops no store: the store held the commits either way,
    /// and the refs are how a person finds them.
    ///
    /// It is a reading, so it is here and not in [`judge`]. Only the path that writes a row
    /// pays for it, which is what that path paid before this module existed.
    #[must_use]
    pub fn record(&self) -> Record {
        Record::of(
            self.rests_on
                .iter()
                .map(|rest| Outside {
                    repository: rest.repository.clone(),
                    references: Git::at(&rest.repository).reaching(&rest.sample, SAMPLE),
                    commits: rest.commits,
                })
                .collect(),
        )
    }
}

// ---------------------------------------------------------------------------
// The judgement.
// ---------------------------------------------------------------------------

/// Decide whether the contracted loss set of one home is completely readable outside it.
///
/// The only maker of a [`Proof`], and the one function every surface asks. It runs no
/// process, opens no repository and reaches no remote, so two calls over one pair of values
/// give one answer — which is what `nodal gc` rests on when it compares a fresh verdict
/// with the record a reclaim left. The one thing it does touch is the filesystem's own
/// spelling of a path, because two names for one directory must not count as two stores.
///
/// What refuses, and it is the same list every surface refused over before:
///
/// | found | answer |
/// |---|---|
/// | a tracked path that differs from `HEAD`, or an untracked path no ignore rule covers | lost |
/// | a commit of the home that nothing outside it reaches | lost |
/// | a commit whose only other copy is inside a home the same removal takes | lost |
/// | something Nodal did not start standing in a home that would move | lost |
/// | a commit nothing here checked the remote for | unread |
/// | a process table that would not answer, for a home that would move | unread |
///
/// Ignored state is not a loss. A removal moves the home to the trash rather than deleting
/// it, so the local state a person goes back for is still there afterwards, and refusing
/// over it would refuse every removal of every home that ever held an `.env.local`.
#[must_use]
pub fn judge(set: &LossSet, evidence: &Evidence) -> Verdict {
    let mut refusals: Vec<Loss> = Vec::new();
    let mut unread: Vec<Unread> = Vec::new();
    for group in set.paths.iter().filter(|group| group.held.refuses()) {
        refusals.push(Loss {
            needs: Needs::UniqueLoss,
            detail: format!("{} ({})", group.held.label(), group.count),
        });
    }
    for group in &set.commits {
        match group.copies.needs() {
            Some(Needs::UnknownEvidence) => {
                unread.push(Unread { why: counted(&group.copies, group.count) });
            }
            Some(needs) => {
                refusals.push(Loss { needs, detail: counted(&group.copies, group.count) });
            }
            None => {}
        }
    }
    refusals.extend(joint(set, &evidence.set));
    match blocked(evidence) {
        Some(Blocking::Standing(detail)) => {
            refusals.push(Loss { needs: Needs::BlockingRuntime, detail });
        }
        Some(Blocking::Unread(why)) => unread.push(Unread { why }),
        None => {}
    }
    refusals.sort_by_key(|one| one.needs);
    if !refusals.is_empty() {
        return Verdict::Unsafe { lost: refusals, unread };
    }
    if !unread.is_empty() {
        return Verdict::Unknown(unread);
    }
    Verdict::Safe(proof(set, evidence.read_at))
}

/// Read the loss set of the home at `path`: what its working tree holds.
///
/// One `git status`, which is the reading every caller of this took already. The commits are
/// not read here, because reading them costs between one and three more `rev-list` runs and
/// needs the project's own checkout to read them against; the caller that has a checkout
/// fills them in ([`LossSet::commits`]).
///
/// `fate` is what a removal does with the home — move it to the trash, or unregister a
/// checkout and leave it — and it decides the sentence a report prints over each group of
/// paths. It is a reading of the registry and never a guess, so it is asked for rather than
/// assumed: a report that offered the trash for a home no removal moves would name a
/// directory the person could not go to.
///
/// # Errors
/// [`crate::Error::Git`] when the status could not be read, and
/// [`crate::Error::NotARepository`] when `path` is not a repository. A directory nobody
/// could read has no loss set rather than an empty one, because a reading nobody took is
/// not a reading that found nothing.
pub fn loss_set(path: &Path, fate: Fate) -> crate::Result<LossSet> {
    let git = Git::open(path)?;
    Ok(LossSet {
        home: path.to_path_buf(),
        paths: assess::working(&git.status()?, fate),
        commits: Vec::new(),
    })
}

/// The words a commit group refuses under, which are the words the report prints over it.
fn counted(copies: &Copies, count: usize) -> String {
    format!("{} ({count})", copies.label())
}

/// The losses the set adds, and none of the ones the home already had.
///
/// Per-unit safety is not joint safety. Two units can each be safe because the other holds
/// the copy, and no per-unit reading can see that: each one is true, and the pair is not. A
/// reading of five unit homes on one machine found it — the second copy of one unit's work
/// was inside another unit's home — and nothing answered the joint question.
///
/// [`judge`] asks it, and so does `nodal doctor`, which has a per-home row already and needs
/// only what the set adds to it. One rule, one wording, two widths.
///
/// The wording names no operation, because two of them ask: a removal of the units on a
/// command line, and a person clearing a machine of every open home of a project.
#[must_use]
pub fn joint(loss: &LossSet, set: &[PathBuf]) -> Vec<Loss> {
    loss.commits
        .iter()
        .filter_map(|group| {
            let Copies::SecondLocalCopy { held_by } = &group.copies else { return None };
            if survives(held_by, set) {
                return None;
            }
            Some(Loss {
                needs: Needs::UniqueLoss,
                detail: format!(
                    "{} {} whose only other copy is in {}, which the same removal takes",
                    group.count,
                    if group.count == 1 { "commit" } else { "commits" },
                    held_by.display()
                ),
            })
        })
        .collect()
}

/// Whether a copy this store holds survives a removal that takes `set` as well.
///
/// Paths are resolved on both sides, because a home reached through a symbolic link and the
/// same home reached directly are one directory with two names, and a comparison of the
/// spellings would count it as a second store.
fn survives(store: &Path, set: &[PathBuf]) -> bool {
    let store = paths::resolve(store);
    !set.iter().any(|home| paths::resolve(home) == store)
}

/// Why the occupancy of a home refuses, in the two ways it can.
enum Blocking {
    /// Processes Nodal did not start stand in the home, named.
    Standing(String),
    /// The process table could not be read, and why.
    Unread(String),
}

/// What the occupancy reading refuses over, or nothing when it refuses over nothing.
///
/// A reading nobody took refuses nothing, and a reading of a home that would not move
/// refuses nothing either: nothing is taken out from under anybody standing in a checkout
/// that stays where it is.
fn blocked(evidence: &Evidence) -> Option<Blocking> {
    let runtime = evidence.runtime.as_ref().filter(|_| evidence.moves)?;
    Some(match assess::unmovable(runtime)? {
        Unmovable::Standing(standing) => {
            let named: Vec<String> = standing.iter().take(SAMPLE).map(Standing::label).collect();
            Blocking::Standing(named.join(", "))
        }
        Unmovable::Unread(note) => {
            Blocking::Unread(format!("the process table could not be read: {}", note.why))
        }
    })
}

/// The proof a safe verdict carries: every store that held a commit of the home.
///
/// A second local copy is one store. A commit a witnessed reading of the remote proves is
/// credited to the stores whose reading was believed, which is where the ref that proves it
/// is readable on this disk. A group that credits nothing contributes nothing — which is
/// every group a refusal would have been raised over, and there are none of those here or
/// this would not be a proof.
fn proof(loss: &LossSet, observed_at: Timestamp) -> Proof {
    let mut found: BTreeMap<PathBuf, (usize, Vec<Oid>)> = BTreeMap::new();
    for group in loss.commits.iter().filter(|group| group.copies.survives()) {
        for repository in holders(&group.copies) {
            let seen = found.entry(repository).or_insert((0, Vec::new()));
            seen.0 += group.count;
            seen.1.extend(group.sample.iter().cloned());
        }
    }
    let rests_on = found
        .into_iter()
        .map(|(repository, (commits, sample))| Rest { repository, commits, sample })
        .collect();
    Proof { rests_on, observed_at }
}

/// The stores one disposition credits, and none for a disposition that credits nothing.
fn holders(copies: &Copies) -> Vec<PathBuf> {
    match copies {
        Copies::SecondLocalCopy { held_by } => vec![held_by.clone()],
        Copies::RemoteProved { witness } => witness.by().to_vec(),
        Copies::OnlyHere { .. } | Copies::NotChecked { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Evidence, LossSet, Verdict, judge};
    use crate::lifecycle::assess::{CommitGroup, Copies, Fate, Held, PathGroup};
    use crate::lifecycle::uniqueness::Witness;
    use crate::model::{Needs, Timestamp};

    fn at() -> Timestamp {
        Timestamp::parse("2026-09-23T09:00:00Z").unwrap()
    }

    fn set(commits: Vec<CommitGroup>, paths: Vec<PathGroup>) -> LossSet {
        LossSet { home: PathBuf::from("/w/home"), paths, commits }
    }

    fn commits(copies: Copies) -> CommitGroup {
        CommitGroup { copies, count: 2, sample: Vec::new() }
    }

    fn paths(held: Held) -> PathGroup {
        PathGroup {
            held,
            fate: Fate::Trashed,
            count: 1,
            sample: vec![PathBuf::from("src/main.rs")],
            bytes: None,
        }
    }

    fn judged(loss: &LossSet) -> Verdict {
        judge(loss, &Evidence::of_work(at()))
    }

    /// Only a proved second copy and a proved remote make a home safe. The other two
    /// dispositions are the two halves of a refusal, and they are different refusals.
    #[test]
    fn only_a_second_copy_or_a_proved_remote_is_safe() {
        let safe = [
            Copies::SecondLocalCopy { held_by: PathBuf::from("/w/project") },
            Copies::RemoteProved { witness: Witness::NoRemote },
        ];
        for copies in safe {
            assert!(judged(&set(vec![commits(copies.clone())], Vec::new())).safe(), "{copies:?}");
        }
        let only = judged(&set(
            vec![commits(Copies::OnlyHere { witness: Witness::NoRemote })],
            Vec::new(),
        ));
        assert!(matches!(only, Verdict::Unsafe { .. }), "{only:?}");
        let checked = judged(&set(
            vec![commits(Copies::NotChecked { witness: Witness::default() })],
            Vec::new(),
        ));
        assert!(matches!(checked, Verdict::Unknown(_)), "{checked:?}");
    }

    /// A home with nothing in it to lose is safe, and its proof rests on nothing because
    /// there was nothing to find a copy of.
    #[test]
    fn a_home_with_nothing_to_lose_rests_on_nothing() {
        let verdict = judged(&set(Vec::new(), Vec::new()));
        assert!(verdict.safe());
        let proof = verdict.proof().unwrap();
        assert!(proof.rests_on().is_empty());
        assert_eq!(proof.observed_at(), at());
        assert!(proof.record().copies().is_empty());
    }

    /// The two path groups a removal would lose refuse; the two the trash keeps do not.
    #[test]
    fn only_the_paths_a_removal_loses_refuse() {
        for held in [Held::Uncommitted, Held::Untracked] {
            assert!(!judged(&set(Vec::new(), vec![paths(held)])).safe(), "{held:?}");
        }
        for held in [Held::LocalState, Held::Generated] {
            assert!(judged(&set(Vec::new(), vec![paths(held)])).safe(), "{held:?}");
        }
    }

    /// A named loss is reported as unsafe and carries the unread reading beside it, because
    /// a person is owed both.
    #[test]
    fn a_loss_and_an_unread_reading_are_both_reported() {
        let verdict = judged(&set(
            vec![commits(Copies::NotChecked { witness: Witness::default() })],
            vec![paths(Held::Untracked)],
        ));
        let Verdict::Unsafe { lost, unread } = &verdict else { panic!("{verdict:?}") };
        assert_eq!(lost.len(), 1);
        assert_eq!(unread.len(), 1);
        let ranked: Vec<Needs> = verdict.reasons().iter().map(|reason| reason.needs).collect();
        assert_eq!(ranked, [Needs::UniqueLoss, Needs::UnknownEvidence]);
        assert_eq!(verdict.top(), Needs::UniqueLoss);
    }

    /// A second copy inside a home the same removal takes is no copy afterwards.
    #[test]
    fn a_copy_inside_the_set_is_discounted() {
        let held = PathBuf::from("/w/other");
        let loss =
            set(vec![commits(Copies::SecondLocalCopy { held_by: held.clone() })], Vec::new());
        assert!(judged(&loss).safe(), "safe on its own");
        let joint = judge(&loss, &Evidence { set: vec![held], ..Evidence::of_work(at()) });
        assert!(!joint.safe(), "{joint:?}");
        assert_eq!(joint.top(), Needs::UniqueLoss);
    }

    /// A safe verdict carries a proof, and both refusing arms carry none. Nothing outside
    /// this module can reach the one maker.
    #[test]
    fn only_a_safe_verdict_carries_a_proof() {
        assert!(judged(&set(Vec::new(), Vec::new())).proof().is_some());
        assert!(judged(&set(Vec::new(), vec![paths(Held::Untracked)])).proof().is_none());
        assert!(
            judged(&set(
                vec![commits(Copies::NotChecked { witness: Witness::default() })],
                Vec::new()
            ))
            .proof()
            .is_none()
        );
    }
}
