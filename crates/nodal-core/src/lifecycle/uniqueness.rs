//! The one uniqueness check: does this home hold work that exists nowhere else?
//!
//! Every destructive path reads [`crate::lifecycle::assess`] before it removes
//! anything, and there is deliberately only one such reading. A second implementation of
//! "is this safe to delete" is a second answer to a question that has to have one, and
//! the difference between the two is the day somebody loses a morning's work.
//!
//! What lives here is the shape that answer is reported in: the [`Finding`]s a refusal
//! names, and the [`Witness`] that says what the reading earned. The evaluator is over
//! there; the words are here.
//!
//! Three things count as work that is only here, and each is a different kind of
//! loss:
//!
//! - **uncommitted changes**: tracked files that differ from the index or from `HEAD`,
//!   and paths with unresolved merge stages;
//! - **untracked files no ignore rule covers**: a new file a person made and has not
//!   added yet is work, and an ignored one is a build artefact;
//! - **commits nothing else proves it has**: commits reachable from the unit's branch
//!   that no *checked* remote-tracking ref holds, and that the project's own checkout
//!   does not have either.
//!
//! The third one is why the check takes two repositories, and the word *checked* is why
//! it takes them in a particular way.
//!
//! A project with no remote — `git init` and nothing since, a private scratch tree, a
//! clone nobody can push to — has *every* commit on no remote, including the ones the
//! home inherited from the base it was cloned from. Those commits are in the person's
//! own checkout, so removing the home does not remove them. Asking the checkout turns
//! the answer from "every commit in the project" into "the commits this unit made",
//! which is the question that was being asked.
//!
//! # A ref is a record of a push, not a reading of a remote
//!
//! The other half is the same checkout answering a different question. `git rev-list
//! <branch> --not --remotes` reads `refs/remotes/` inside the home, and a home writes
//! those refs when `nodal done` pushes and never corrects them afterwards. A branch
//! somebody deleted on the remote after that push is still named there, and a check
//! that believed the name would call the home's only copy of a commit safe to remove.
//! That is a false safe, and it is the one answer this check must never give.
//!
//! So the home's own refs are believed only where a **witness** confirms them, and the
//! witness is the person's checkout, which is the repository on this machine that
//! actually fetches `origin` ([`super::witness`]). The rule that turns a ref into proof
//! is [`crate::doctor::unique::believed`], which is one function and is the same one the
//! machine survey uses.
//!
//! With no witness, nothing about the remote is proved, and the check says so rather
//! than assuming either answer. The commits then stand or fall on the one question that
//! needs no ref at all: does another tree on this machine hold them? A yes is safe
//! whatever the remote has. A no is a refusal that names its reason, because refusing to
//! remove work that may be reconstructable costs a directory, and removing the only copy
//! of a commit costs the work.
//!
//! One class of path is never work: the files Nodal itself writes into a home. Which
//! those are is stated once, in [`crate::env::files::WRITTEN`], and read here rather
//! than listed again. They are normally invisible to `git status` anyway, because a
//! create puts them in the home's `.git/info/exclude` ([`crate::env::files::hide`]).
//! Leaving them out here as well is what stops a home whose exclude file somebody
//! edited from refusing every reclaim for ever, over files Nodal put there and nobody
//! wrote.
//!
//! The check reads and never writes. What is done about a finding — refuse, or take a
//! snapshot and go on — belongs to the operation.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git::Oid;
use crate::lifecycle::witness::Elsewhere;

/// How many paths or commits one finding names before it says how many more there are.
///
/// A refusal is read by a person who has to decide what to do next, and forty file
/// names is not a decision aid. The count is exact; the list is a sample.
pub const SAMPLE: usize = 10;

/// One reason a home is not safe to remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    /// Tracked paths that differ from `HEAD` or the index, or that are unmerged.
    Uncommitted {
        /// How many there are.
        count: usize,
        /// The first [`SAMPLE`] of them, for the message.
        sample: Vec<PathBuf>,
    },
    /// Paths Git does not track and no ignore rule covers.
    Untracked {
        /// How many there are.
        count: usize,
        /// The first [`SAMPLE`] of them.
        sample: Vec<PathBuf>,
    },
    /// Commits that nothing on this machine can prove exist anywhere else.
    Unpushed {
        /// How many there are.
        count: usize,
        /// The first [`SAMPLE`] of them, newest first.
        sample: Vec<Oid>,
        /// The remotes that were asked. Empty means the repository has none, which is
        /// worth printing: nothing has been pushed because there is nowhere to push.
        remotes: Vec<String>,
        /// Whether anything on this machine could check the home's own remote-tracking
        /// refs. This is what tells "the remote does not have these commits" apart
        /// from "nothing here knows what the remote has", and the two are different
        /// refusals.
        ///
        /// Defaulted on the way in, so that a finding written by an older Nodal reads
        /// as the stricter of the two rather than as a claim it never made.
        #[serde(default)]
        witness: Witness,
    },
}

/// Whether anything on this machine could check a home's own remote-tracking refs.
///
/// A home's `refs/remotes/origin/*` is its record of its own pushes ([`super::witness`]).
/// It is proof only where a repository that reads that remote confirms it, so a finding
/// carries which of the four cases it is, and a report prints the reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Witness {
    /// Nothing here reads this home's remote, so the home's own refs were not believed.
    /// The strictest reading, and the default.
    #[default]
    Unchecked,
    /// A clone that reads the remote said which refs are still on it. A clone can only
    /// say what it last saw, so what it does not reach is unproved rather than absent.
    Checked {
        /// The repositories whose reading was used.
        by: Vec<PathBuf>,
        /// The branches that reading says nothing about, each with which of the two
        /// readings it was ([`Unobserved`]).
        ///
        /// A reading covers the branches of a remote as they were when the fetch was
        /// made, and there are two ways a branch falls outside it. The reading is the
        /// older one, so it answers for a state of the branch from before this work. Or
        /// the reading is the later one and does not name the branch at all, while the
        /// checkout still tracks it, which is what a dropped branch and a prune-less
        /// fetch leave. Either way the reading answers for the remote and not for this
        /// work; what differs is what happened and therefore what to run next, so each
        /// branch carries which it was.
        ///
        /// Defaulted on the way in, so that a finding written by an older Nodal reads as
        /// a reading with nothing to report rather than as a claim it never made.
        #[serde(default)]
        unobserved: Vec<Unobserved>,
    },
    /// The remote itself is on this machine and was read. A project with no remote of
    /// its own is cloned from the person's checkout, so the checkout is the remote, and
    /// what it does not hold the remote does not hold.
    Direct {
        /// Where the remote is.
        by: Vec<PathBuf>,
    },
    /// The repository names no remote, so there is no remote question to answer.
    NoRemote,
}

/// One branch a reading of the remote says nothing about, and which reading it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unobserved {
    /// The branch, as the home's own record of the remote names it.
    pub branch: String,
    /// Which of the two readings left it unanswered.
    pub reading: Unread,
}

/// Why a reading of the remote answers for no state of one branch.
///
/// Two readings and not one, because they are two things that happened and the thing to
/// do about them differs. One report gave the person the first account for the second
/// case: a branch the remote had dropped was described as one this home recorded after
/// the fetch, under an instruction — fetch and read again — that cannot change the
/// answer while the stale ref stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unread {
    /// The reading is not later than this home's own record of the branch. It answers for
    /// a state of the branch from before the work, or from before the branch was there at
    /// all, and a later reading settles it.
    ///
    /// The default, which is what a finding an older Nodal wrote says: that release
    /// recorded the branch without recording which reading it was, and described this one.
    #[default]
    Older,
    /// The reading is the later one and names the branch nowhere, while this checkout
    /// still tracks it. The remote dropped the branch and the fetch did not prune, or the
    /// fetch asked the remote about one branch by name and about this one asked nothing.
    /// No record on this disk tells those two apart; a fetch that prunes settles both.
    Unnamed,
}

impl Unread {
    /// The clause this reading adds about the branches it left unanswered.
    ///
    /// It says what the reading did, and then what to run. The two instructions are
    /// different instructions: a reading that is merely older is corrected by a newer one,
    /// and a reading that does not name a branch the checkout still tracks is not — the
    /// stale ref is what keeps the question open, and only a fetch that prunes removes it.
    fn about(self, branches: &str) -> String {
        match self {
            Self::Older => format!(
                "that reading was taken before this home wrote its own record of \
                 {branches}, so for those branches it is the older one; fetch in the \
                 checkout and read again"
            ),
            Self::Unnamed => format!(
                "that reading is the later one and does not name {branches} at all, while \
                 this checkout still tracks the branch, so what the remote has now is not \
                 known: the remote may have dropped it, or the fetch may have asked about \
                 one branch by name. Fetch with --prune in the checkout and read again"
            ),
        }
    }
}

impl<'de> Deserialize<'de> for Unobserved {
    /// Reads the row this writes, and the bare branch name an older Nodal wrote.
    ///
    /// A finding reaches a deserializer from the journal of a reclaim that was
    /// interrupted, so a run the release before this one started has to be finishable by
    /// this one. That release recorded the branch alone, and [`Unread::Older`] is the
    /// reading it described.
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Form {
            /// What an older Nodal wrote: the branch, and no reading.
            Named(String),
            /// What this one writes.
            Row {
                branch: String,
                #[serde(default)]
                reading: Unread,
            },
        }

        Ok(match Form::deserialize(from)? {
            Form::Named(branch) => Self { branch, reading: Unread::Older },
            Form::Row { branch, reading } => Self { branch, reading },
        })
    }
}

/// What a refusal adds when nothing on this machine could check the home's own refs.
const UNREAD: &str = "this home's own remote-tracking refs record its own pushes, and \
     nothing here read the remote to check them. Fetch in the project checkout and \
     reclaim again.";

impl Witness {
    /// The clause a finding adds to say why the remote could not answer.
    ///
    /// Nothing for a project with no remote: there is nowhere to push, and the label
    /// says the whole of it. The other two are the two ways a remote goes unproved, and
    /// a person deciding what to do next needs to know which one they have.
    ///
    /// Public because the preflight prints the same clause over the same commits
    /// ([`crate::output::view::check`]). A person who reads one and then the other must
    /// not be given two accounts of one reading.
    #[must_use]
    pub fn because(&self) -> Option<String> {
        match self {
            Self::NoRemote | Self::Direct { .. } => None,
            Self::Checked { by, unobserved } => {
                let mut clause = format!(
                    "the newest reading of the remote here is {}, and it does not reach them",
                    names(by.iter().map(|path| path.display().to_string()))
                );
                for (reading, branches) in grouped(unobserved) {
                    clause.push_str("; ");
                    clause.push_str(&reading.about(&branches));
                }
                Some(clause)
            }
            Self::Unchecked => Some(String::from(UNREAD)),
        }
    }

    /// Whether this reading leaves the remote question open, which is the strictest
    /// reading of the four and the default.
    ///
    /// The line every report draws between a commit that is only here and a commit
    /// nothing could check: [`crate::lifecycle::assess`] draws it to choose the
    /// disposition, and the sweep of the trash draws it to choose the words of a line
    /// over the same commits ([`crate::output::view::HeldBack`]). One question, asked
    /// here, so the two cannot answer it differently.
    ///
    /// It is not [`Witness::settled`], which is the finer question of whether the
    /// reading that was made proves anything: a clone read the remote and still only
    /// says what it last saw, so a report that calls such a commit only here says on the
    /// next line which reading that rests on ([`Witness::because`]).
    #[must_use]
    pub fn unchecked(&self) -> bool {
        match self {
            Self::Unchecked => true,
            // A reading that covered some branches of a remote and not others cannot
            // carry "only here" about work on one it did not cover. The verdict is the
            // same either way; the word a person reads is not.
            Self::Checked { unobserved, .. } => !unobserved.is_empty(),
            Self::Direct { .. } | Self::NoRemote => false,
        }
    }

    /// Whether this reading settled the remote question, rather than leaving it open.
    ///
    /// Settled means there is no remote to ask, or the remote is on this disk and was
    /// read. Anything else is a reading of a copy, and a copy can only say what it last
    /// saw, so what it does not reach is unproved rather than absent.
    #[must_use]
    pub const fn settled(&self) -> bool {
        matches!(self, Self::NoRemote | Self::Direct { .. })
    }

    /// The repositories whose reading stands behind this one, which is none where
    /// nothing read the remote and none where there is no remote to read.
    #[must_use]
    pub fn by(&self) -> &[PathBuf] {
        match self {
            Self::Checked { by, .. } | Self::Direct { by } => by,
            Self::Unchecked | Self::NoRemote => &[],
        }
    }

    /// A reading of the remote that had nothing it could not observe.
    ///
    /// The ordinary shape, and the one every caller outside the assessment wants. The
    /// other is made where the observations are read ([`Witness::of`]).
    #[must_use]
    pub const fn checked(by: Vec<PathBuf>) -> Self {
        Self::Checked { by, unobserved: Vec::new() }
    }

    /// Which case this is, for a home with these remotes and this reading of them.
    ///
    /// Public because the assessment that produces every finding is the caller
    /// ([`crate::lifecycle::assess`]), and there must not be a second rule for which of
    /// the four a run has earned.
    #[must_use]
    pub fn of(remotes: &[String], found: &Elsewhere) -> Self {
        if remotes.is_empty() {
            return Self::NoRemote;
        }
        if found.witnesses.is_empty() {
            return Self::Unchecked;
        }
        if found.direct {
            return Self::Direct { by: found.witnesses.clone() };
        }
        Self::Checked { by: found.witnesses.clone(), unobserved: found.unobserved.clone() }
    }
}

impl Finding {
    /// The word this finding is reported under.
    ///
    /// "on no remote" is a statement about the remote, and a run has to have earned it:
    /// either the repository has no remote, or the remote is on this machine and was
    /// read. Nodal makes no network call, so anywhere else a commit left over is one
    /// this machine could not prove a remote has — which is not the same claim, and the
    /// word says so.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Uncommitted { .. } => "uncommitted changes",
            Self::Untracked { .. } => "untracked files",
            Self::Unpushed { witness: Witness::NoRemote | Witness::Direct { .. }, .. } => {
                "commits on no remote"
            }
            Self::Unpushed { .. } => "commits no current reading proves a remote has",
        }
    }

    /// How many things this finding is about.
    #[must_use]
    pub const fn count(&self) -> usize {
        match self {
            Self::Uncommitted { count, .. }
            | Self::Untracked { count, .. }
            | Self::Unpushed { count, .. } => *count,
        }
    }

    /// The commits this finding is about, newest first, and none for a finding that is
    /// about paths.
    ///
    /// A sample and not the whole of them: [`Finding::count`] is the fact, and this is
    /// the first [`SAMPLE`] a message names.
    #[must_use]
    pub fn commits(&self) -> &[Oid] {
        match self {
            Self::Unpushed { sample, .. } => sample,
            Self::Uncommitted { .. } | Self::Untracked { .. } => &[],
        }
    }

    /// What this machine could say about the remote while it read them, and nothing for
    /// a finding the remote has no opinion on.
    ///
    /// A path that differs from `HEAD` is only ever here, so there is no remote question
    /// to answer about one and no reading to report.
    #[must_use]
    pub const fn witness(&self) -> Option<&Witness> {
        match self {
            Self::Unpushed { witness, .. } => Some(witness),
            Self::Uncommitted { .. } | Self::Untracked { .. } => None,
        }
    }

    /// The finding as one line: what it is, how many, and a sample of them.
    #[must_use]
    pub fn describe(&self) -> String {
        let listed = match self {
            Self::Uncommitted { sample, .. } | Self::Untracked { sample, .. } => {
                names(sample.iter().map(|path| path.display().to_string()))
            }
            Self::Unpushed { sample, .. } => {
                names(sample.iter().map(|oid| oid.as_str().chars().take(8).collect()))
            }
        };
        let more = self.count().saturating_sub(SAMPLE);
        let tail = if more == 0 { String::new() } else { format!(" and {more} more") };
        let why = match self {
            Self::Unpushed { witness, .. } => {
                witness.because().map(|clause| format!("; {clause}")).unwrap_or_default()
            }
            _ => String::new(),
        };
        format!("{} ({}): {listed}{tail}{why}", self.label(), self.count())
    }

    /// Every finding as one line, for a message that has to name the reason.
    #[must_use]
    pub fn summarise(findings: &[Self]) -> String {
        names(findings.iter().map(Self::describe))
    }
}

/// What the check found about one home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uniqueness {
    /// The home that was read.
    pub home: PathBuf,
    /// What is only here, in the order the check makes it. Empty means the home holds
    /// nothing that removing it would lose.
    pub findings: Vec<Finding>,
}

impl Uniqueness {
    /// Whether the home holds nothing that exists only here.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Join what a finding lists, in the one form every message here uses.
fn names(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<String>>().join(", ")
}

/// The branches of each reading that left any, in the order [`Unread`] declares them.
///
/// One clause per reading and never one per branch: a fetch that did not prune leaves
/// every dropped branch in the same state, and a sentence repeated for each of them is a
/// sentence a person stops reading.
fn grouped(unobserved: &[Unobserved]) -> Vec<(Unread, String)> {
    [Unread::Older, Unread::Unnamed]
        .into_iter()
        .filter_map(|reading| {
            let branches = unobserved
                .iter()
                .filter(|one| one.reading == reading)
                .map(|one| one.branch.clone());
            let listed = names(branches);
            (!listed.is_empty()).then_some((reading, listed))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Finding, SAMPLE, Uniqueness, Unobserved, Unread, Witness};
    use crate::git::Oid;

    /// A reading of the remote by one checkout, with these branches left unanswered.
    fn checked(unobserved: Vec<Unobserved>) -> Witness {
        Witness::Checked { by: vec![PathBuf::from("/w/project")], unobserved }
    }

    /// One branch of a reading, under the reading that left it unanswered.
    fn branch(name: &str, reading: Unread) -> Unobserved {
        Unobserved { branch: String::from(name), reading }
    }

    fn sample(count: usize) -> Finding {
        Finding::Untracked {
            count,
            sample: (0..count.min(SAMPLE)).map(|n| PathBuf::from(format!("f{n}"))).collect(),
        }
    }

    #[test]
    fn a_home_with_no_finding_is_clear() {
        let clear = Uniqueness { home: PathBuf::from("/h"), findings: Vec::new() };
        assert!(clear.is_clear());
        assert!(!Uniqueness { home: PathBuf::from("/h"), findings: vec![sample(1)] }.is_clear());
    }

    #[test]
    fn a_long_finding_says_how_many_more_it_did_not_name() {
        let described = sample(SAMPLE + 3).describe();
        assert!(described.starts_with("untracked files (13): f0, f1"), "{described}");
        assert!(described.ends_with("and 3 more"), "{described}");
    }

    #[test]
    fn a_short_finding_names_all_of_it_and_promises_no_more() {
        let described = sample(2).describe();
        assert_eq!(described, "untracked files (2): f0, f1");
    }

    /// A commit left over where nothing read the remote is not a commit proved absent
    /// from one. The words say which, and the refusal says what to do next.
    #[test]
    fn a_finding_nothing_checked_does_not_claim_the_remote_has_nothing() {
        let unchecked = unpushed(Witness::Unchecked);
        assert_eq!(unchecked.label(), "commits no current reading proves a remote has");
        let described = unchecked.describe();
        assert!(described.contains("nothing here read the remote to check them"), "{described}");
        assert!(described.contains("Fetch in the project checkout"), "{described}");
    }

    /// A clone that reads the remote can say what it last saw and no more, so the
    /// refusal names the reading rather than claiming the remote is empty.
    #[test]
    fn a_reading_that_does_not_reach_a_commit_names_the_reading() {
        let checked = unpushed(Witness::checked(vec![PathBuf::from("/w/project")]));
        assert_eq!(checked.label(), "commits no current reading proves a remote has");
        let described = checked.describe();
        assert!(described.contains("/w/project"), "{described}");
        assert!(described.contains("does not reach them"), "{described}");
    }

    /// The two cases that have earned the words. There is no remote, or the remote is
    /// here and was read, and either way "on no remote" is a fact rather than a guess.
    #[test]
    fn only_a_read_remote_or_no_remote_earns_the_words_on_no_remote() {
        for witness in [Witness::NoRemote, Witness::Direct { by: vec![PathBuf::from("/w")] }] {
            let finding = unpushed(witness);
            assert_eq!(finding.label(), "commits on no remote");
            assert!(finding.describe().ends_with("abababab"), "{}", finding.describe());
        }
    }

    /// A reading older than this home's own record of the branch is described as the older
    /// one, and the instruction is the one that changes the answer: read the remote again.
    #[test]
    fn a_reading_older_than_the_home_is_described_as_the_older_one() {
        let older = checked(vec![branch("nodal/worker-import", Unread::Older)]).because().unwrap();
        assert!(older.contains("nodal/worker-import"), "{older}");
        assert!(older.contains("was taken before this home wrote its own record"), "{older}");
        assert!(older.contains("it is the older one"), "{older}");
        assert!(older.contains("fetch in the checkout and read again"), "{older}");
    }

    /// A reading that is the newer one and does not name the branch is described as that,
    /// and not as the older reading.
    ///
    /// This is a branch the remote dropped, after which the checkout fetched without
    /// `--prune`. The reading was taken *after* the push, so the account that called it the
    /// older reading named a cause that had not happened, under an instruction — fetch and
    /// read again — that cannot change the answer while the stale ref stands. What does is
    /// a fetch that prunes, and that is what the clause says to run.
    #[test]
    fn a_reading_that_no_longer_names_the_branch_is_described_as_that_and_not_as_an_older_one() {
        let dropped =
            checked(vec![branch("nodal/worker-import", Unread::Unnamed)]).because().unwrap();
        assert!(dropped.contains("nodal/worker-import"), "{dropped}");
        assert!(dropped.contains("is the later one and does not name"), "{dropped}");
        assert!(dropped.contains("--prune"), "the instruction cannot change the answer: {dropped}");
        assert!(
            !dropped.contains("was taken before this home wrote its own record"),
            "the later reading is described as the older one: {dropped}",
        );
    }

    /// A reading with both kinds says both, one clause each, however many branches are in
    /// either.
    #[test]
    fn a_reading_with_both_kinds_gives_one_clause_for_each_and_not_one_per_branch() {
        let both = checked(vec![
            branch("nodal/one", Unread::Older),
            branch("nodal/two", Unread::Unnamed),
            branch("nodal/three", Unread::Unnamed),
        ])
        .because()
        .unwrap();
        assert_eq!(both.matches("was taken before").count(), 1, "{both}");
        assert_eq!(both.matches("is the later one").count(), 1, "{both}");
        assert!(both.contains("nodal/two, nodal/three"), "the branches are not listed: {both}");

        // And a reading with nothing unanswered adds no clause at all.
        let clean = checked(Vec::new()).because().unwrap();
        assert!(clean.ends_with("does not reach them"), "{clean}");
    }

    /// A row an older Nodal wrote is the branch alone, and it reads as the reading that
    /// release described.
    ///
    /// A reclaim interrupted by one release is finished by the next, and the plan it is
    /// rebuilt from carries this value.
    #[test]
    fn a_branch_written_before_this_field_reads_as_the_older_reading() {
        let read: Vec<Unobserved> = serde_json::from_str(r#"["nodal/worker-import"]"#).unwrap();
        assert_eq!(read, vec![branch("nodal/worker-import", Unread::Older)]);

        let row = r#"[{"branch":"nodal/worker-import","reading":"unnamed"}]"#;
        let read: Vec<Unobserved> = serde_json::from_str(row).unwrap();
        assert_eq!(read, vec![branch("nodal/worker-import", Unread::Unnamed)]);
    }

    /// The reason travels in `--json` as well as in the message, under its own key.
    #[test]
    fn the_reason_is_a_field_of_the_finding_and_not_only_of_the_sentence() {
        let written = serde_json::to_string(&unpushed(Witness::Unchecked)).unwrap();
        assert!(written.contains("\"witness\":{\"kind\":\"unchecked\"}"), "{written}");
    }

    /// A finding an older Nodal wrote carries no such field. It reads as the strictest
    /// case, which is a claim it never made rather than one it did not earn.
    #[test]
    fn a_finding_written_before_this_field_reads_as_the_strictest_case() {
        let older = r#"{"kind":"unpushed","count":1,"sample":[],"remotes":["origin"]}"#;
        let read: Finding = serde_json::from_str(older).unwrap();
        assert_eq!(read.label(), "commits no current reading proves a remote has");
    }

    /// One unpushed finding over one commit, for the properties about its words.
    fn unpushed(witness: Witness) -> Finding {
        Finding::Unpushed {
            count: 1,
            sample: vec![Oid::parse(&"ab".repeat(20)).unwrap()],
            remotes: vec![String::from("origin")],
            witness,
        }
    }

    #[test]
    fn a_summary_names_every_reason_the_check_gave() {
        let summary = Finding::summarise(&[
            Finding::Uncommitted { count: 1, sample: vec![PathBuf::from("a.rs")] },
            sample(1),
        ]);
        assert!(summary.contains("uncommitted changes (1): a.rs"), "{summary}");
        assert!(summary.contains("untracked files (1): f0"), "{summary}");
    }
}
