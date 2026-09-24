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
        /// The branches that reading is older than, and so says nothing about.
        ///
        /// A reading covers the branches of a remote as they were when the fetch was made.
        /// A branch this home wrote its own record of after that fetch is outside it: a
        /// branch absent from such a reading is one that did not exist yet, and a branch
        /// present in it stands at a commit from before the work. Either way the reading
        /// answers for the remote and not for this work, so the branch is named and the
        /// person is told what to run.
        ///
        /// Defaulted on the way in, so that a finding written by an older Nodal reads as
        /// a reading with nothing to report rather than as a claim it never made.
        #[serde(default)]
        unobserved: Vec<String>,
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
                let read = format!(
                    "the newest reading of the remote here is {}, and it does not reach them",
                    names(by.iter().map(|path| path.display().to_string()))
                );
                if unobserved.is_empty() {
                    return Some(read);
                }
                Some(format!(
                    "{read}; that reading was taken before this home wrote its own record of \
                     {}, so for those branches it is the older one; fetch in the checkout and \
                     read again",
                    names(unobserved.iter().cloned())
                ))
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Finding, SAMPLE, Uniqueness, Witness};
    use crate::git::Oid;

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
