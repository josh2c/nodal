//! Whether a clone holds commits nothing else on this machine holds.
//!
//! The question doctor's uniqueness column answers is "if I delete this folder, does
//! anything survive that was only here". Until this module existed, the answer came from
//! `git rev-list HEAD --not --remotes`, which is a question about `refs/remotes/` inside
//! the clone itself. A clone writes those refs when it fetches or pushes and never
//! corrects them, so a clone that pushed a branch once reports that branch as pushed for
//! the rest of its life, even after the branch is deleted on the remote and the commits
//! live in that one directory. The reassurance was a record of an old push, not a
//! reading of anything.
//!
//! So the proof is built here, on every run, from two kinds of evidence and no network:
//!
//! | evidence | what it proves | how it can fail |
//! |---|---|---|
//! | another clone's object store holds the commit | the commit survives this folder | nothing: it is a second copy |
//! | a remote-tracking ref holds the commit | the commit reached the remote once | the branch may have been deleted or rewritten since |
//!
//! The second is kept, because without it every single clone of a third-party project
//! on the machine would be called unique work. It is kept under one condition: the
//! **freshest witness** does not contradict it. The freshest witness is the clone of the
//! same remote that heard from that remote most recently, out of those that heard from
//! it after this clone did and that fetch every branch. When that clone does not have
//! the branch, the branch is gone and the ref is not proof. When there is no clone
//! fresher than this one, nothing on this machine can say either way and the ref stands.
//!
//! Only the freshest witness is asked, and this is not a detail. Thirty-four clones of
//! one remote were made over several weeks, and each one holds the refs the remote had
//! on the day it was made. Asking whether *any* fresher clone still has the branch lets
//! one clone made a day later corroborate another clone's stale ref, and a chain of them
//! corroborates all the way forward. Only the most recent reading of the remote is a
//! reading of the remote as it is now.
//!
//! A clone that could not be read is never called clean. It is reported as not checked,
//! and the closing line counts how many were not checked, because an unchecked clone
//! reported as safe is the fault this module exists to remove.
//!
//! Nothing here writes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::git::{Git, Oid};

/// One remote-tracking ref: which branch of which remote, and the commit it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTip {
    /// The branch name, without the remote, as the remote spells it.
    pub branch: String,
    /// The commit the ref holds.
    pub oid: Oid,
}

/// What one clone can say, read once while the clone is inspected.
#[derive(Debug, Clone, Default)]
pub struct Evidence {
    /// The commit HEAD names, `None` when the clone has no commit.
    pub head: Option<Oid>,
    /// Every ref tip. Each one is a commit this object store holds.
    pub tips: Vec<Oid>,
    /// The remote-tracking refs, which say what this clone last saw of a remote.
    pub remotes: Vec<RemoteTip>,
    /// When this clone last heard from a remote, `None` when that cannot be read.
    pub heard: Option<SystemTime>,
    /// Whether this clone fetches every branch, and so can say that one is gone.
    pub complete: bool,
    /// Whether the object store is shallow, which makes it no proof of anything.
    pub shallow: bool,
    /// Why this clone could not be read, `None` when it was.
    pub unreadable: Option<String>,
}

/// What this run proved about one clone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proof {
    /// Commits of HEAD that no remote-tracking ref this run trusts already holds.
    pub off_remote: Option<usize>,
    /// Commits of HEAD that no other copy on this machine holds.
    pub only_copy: Option<usize>,
    /// Why the proof was not made, `None` when it was.
    pub unchecked: Option<String>,
}

impl Proof {
    /// A clone the run could not examine.
    fn unchecked(why: impl Into<String>) -> Self {
        Self { off_remote: None, only_copy: None, unchecked: Some(why.into()) }
    }

    /// A clone with nothing in it to lose.
    fn empty() -> Self {
        Self { off_remote: Some(0), only_copy: Some(0), unchecked: None }
    }
}

/// One clone and what it can say.
#[derive(Debug, Clone)]
pub struct Subject {
    /// The working tree.
    pub path: PathBuf,
    /// What was read from it.
    pub evidence: Evidence,
}

/// Prove, for each clone of one group, what it is the only copy of.
///
/// The clones are of one remote, so any of them can be a witness for any other.
#[must_use]
pub fn prove(subjects: &[Subject]) -> Vec<Proof> {
    let elsewhere = held_by_others(subjects);
    subjects
        .iter()
        .enumerate()
        .map(|(index, subject)| one(subject, &trusted(subjects, index), &elsewhere[index]))
        .collect()
}

/// The proof for one clone: two `rev-list` runs at worst, one when it is contained.
fn one(subject: &Subject, trusted: &[Oid], elsewhere: &[Oid]) -> Proof {
    if let Some(why) = &subject.evidence.unreadable {
        return Proof::unchecked(why.clone());
    }
    let Some(head) = &subject.evidence.head else {
        return Proof::empty();
    };
    let git = Git::at(&subject.path);
    let off_remote = match git.count_outside(head.as_str(), trusted) {
        Ok(count) => count,
        Err(error) => return Proof::unchecked(error.to_string()),
    };
    if off_remote == 0 {
        return Proof { off_remote: Some(0), only_copy: Some(0), unchecked: None };
    }
    let mut anywhere: Vec<Oid> = trusted.to_vec();
    anywhere.extend_from_slice(elsewhere);
    match git.count_outside(head.as_str(), &anywhere) {
        Ok(only_copy) => {
            Proof { off_remote: Some(off_remote), only_copy: Some(only_copy), unchecked: None }
        }
        Err(error) => Proof::unchecked(error.to_string()),
    }
}

/// What this run is willing to believe the remote holds, for one clone.
///
/// With no clone fresher than this one, nothing on this machine can correct its refs and
/// they stand as they are. With a fresher clone, that clone's reading of each branch
/// replaces this one's: the branch it does not have is gone, and the branch it has is at
/// its tip, not at whatever tip this clone last saw. A branch that was rewritten keeps
/// its name and drops its old commits, and reading the old tip as proof of a push is the
/// same mistake as reading a deleted branch that way.
fn trusted(subjects: &[Subject], index: usize) -> Vec<Oid> {
    let subject = &subjects[index];
    let witnesses = freshest(subjects, index);
    if witnesses.is_empty() {
        return subject.evidence.remotes.iter().map(|tip| tip.oid.clone()).collect();
    }
    let mut tips: Vec<Oid> = subject
        .evidence
        .remotes
        .iter()
        .filter_map(|tip| witnessed(&witnesses, &tip.branch))
        .collect();
    tips.sort_unstable();
    tips.dedup();
    tips
}

/// Where the freshest witness has this branch, `None` when it does not have it at all.
fn witnessed(witnesses: &[&Evidence], branch: &str) -> Option<Oid> {
    witnesses
        .iter()
        .flat_map(|witness| witness.remotes.iter())
        .find(|tip| tip.branch == branch)
        .map(|tip| tip.oid.clone())
}

/// The clones that heard from the remote most recently, out of those that heard from it
/// after `index` did. More than one only where two clones share a reading to the second.
fn freshest(subjects: &[Subject], index: usize) -> Vec<&Evidence> {
    let subject = &subjects[index];
    let candidates: Vec<&Evidence> = subjects
        .iter()
        .enumerate()
        .filter(|(other, candidate)| *other != index && fresher(&candidate.evidence, subject))
        .map(|(_, candidate)| &candidate.evidence)
        .collect();
    let Some(latest) = candidates.iter().filter_map(|candidate| candidate.heard).max() else {
        return Vec::new();
    };
    candidates.into_iter().filter(|candidate| candidate.heard == Some(latest)).collect()
}

/// Whether `candidate` heard from the remote after `subject` did, and fetches every
/// branch, which is what it takes to say that a branch is not there any more.
fn fresher(candidate: &Evidence, subject: &Subject) -> bool {
    if !candidate.complete || candidate.unreadable.is_some() {
        return false;
    }
    match (candidate.heard, subject.evidence.heard) {
        (Some(later), Some(earlier)) => later > earlier,
        _ => false,
    }
}

/// For each clone, the tips every other clone of the group holds in its object store.
///
/// A tip shared with another clone stays in the list. It is the shared tips that carry
/// the whole answer: a commit two clones both hold survives the deletion of either one.
/// What comes out is only the clone's own sole hold on a tip, which proves nothing about
/// a folder that is about to go.
///
/// A shallow clone is not a witness: its object store stops at a depth, so a tip in it
/// does not mean the history behind that tip is in it.
fn held_by_others(subjects: &[Subject]) -> Vec<Vec<Oid>> {
    let mut holders: BTreeMap<&Oid, usize> = BTreeMap::new();
    let mine: Vec<BTreeSet<&Oid>> = subjects
        .iter()
        .map(|subject| {
            if witnessable(&subject.evidence) {
                subject.evidence.tips.iter().collect()
            } else {
                BTreeSet::new()
            }
        })
        .collect();
    for tips in &mine {
        for oid in tips {
            *holders.entry(oid).or_default() += 1;
        }
    }
    mine.iter()
        .map(|tips| {
            holders
                .iter()
                .filter(|(oid, held)| **held > usize::from(tips.contains(*oid)))
                .map(|(oid, _)| (*oid).clone())
                .collect()
        })
        .collect()
}

/// Whether this clone's object store may stand as a copy for another clone.
fn witnessable(evidence: &Evidence) -> bool {
    !evidence.shallow && evidence.unreadable.is_none()
}

/// What Git writes when a clone hears from a remote, newest of the three is the reading.
const HEARD: [&str; 3] = ["FETCH_HEAD", "packed-refs", "refs/remotes"];

/// When this clone last heard from a remote.
///
/// A clone that has never fetched still has `packed-refs` from the day it was made, and
/// that day is exactly when it last heard. `None` where `.git` is not a directory of
/// this clone's own, because then nothing here is this clone's reading.
#[must_use]
pub fn heard(path: &Path) -> Option<SystemTime> {
    let git_dir = path.join(".git");
    if !git_dir.is_dir() {
        return None;
    }
    HEARD
        .iter()
        .filter_map(|name| std::fs::metadata(git_dir.join(name)).ok()?.modified().ok())
        .max()
}

/// Whether these refspecs bring in every branch of the remote.
#[must_use]
pub fn complete(refspecs: &[String]) -> bool {
    refspecs.iter().any(|spec| spec.contains("refs/heads/*"))
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{Evidence, RemoteTip, Subject, complete, fresher, held_by_others, trusted};
    use crate::git::Oid;

    fn oid(seed: u8) -> Oid {
        Oid::parse(&format!("{seed:02x}").repeat(20)).expect("a well formed id")
    }

    fn subject(name: &str, evidence: Evidence) -> Subject {
        Subject { path: name.into(), evidence }
    }

    fn clone_of(branch: &str, seed: u8, seconds: u64) -> Evidence {
        Evidence {
            head: Some(oid(seed)),
            tips: vec![oid(seed)],
            remotes: vec![RemoteTip { branch: branch.to_owned(), oid: oid(seed) }],
            heard: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
            complete: true,
            shallow: false,
            unreadable: None,
        }
    }

    /// The whole point: an older clone's ref stops being proof once a clone that heard
    /// from the same remote later does not have the branch.
    #[test]
    fn a_fresher_witness_without_the_branch_takes_the_ref_away() {
        let old = subject("old", clone_of("gone", 1, 100));
        let new = subject("new", clone_of("main", 2, 200));
        assert!(trusted(&[old, new], 0).is_empty(), "a deleted branch is not proof of a push");
    }

    /// A branch keeps its name through a rewrite and drops its old commits. The tip the
    /// freshest clone has is the reading of it, not the tip this clone last saw.
    #[test]
    fn a_rewritten_branch_is_read_at_the_freshest_tip() {
        let old = subject("old", clone_of("main", 1, 100));
        let new = subject("new", clone_of("main", 2, 200));
        assert_eq!(trusted(&[old, new], 0), vec![oid(2)], "the old tip is not the remote");
    }

    /// Nothing on this machine can say either way, so the ref stands.
    #[test]
    fn a_ref_no_fresher_clone_contradicts_stands() {
        let alone = subject("alone", clone_of("main", 1, 100));
        let older = subject("older", clone_of("other", 2, 50));
        assert_eq!(trusted(&[alone, older], 0), vec![oid(1)]);
    }

    /// The reason only the freshest is asked. A clone made a day after this one holds
    /// the refs of that day, deleted branches and all. It is not a reading of the remote
    /// as it is now, and it may not vouch for one.
    #[test]
    fn a_slightly_fresher_clone_does_not_outvote_the_freshest_one() {
        let stale = subject("stale", clone_of("gone", 1, 100));
        let mut nearly = clone_of("main", 2, 110);
        nearly.remotes.push(RemoteTip { branch: String::from("gone"), oid: oid(1) });
        let today = subject("today", clone_of("main", 3, 900));
        let subjects = [stale, subject("nearly", nearly), today];
        assert!(trusted(&subjects, 0).is_empty(), "the freshest clone has no such branch");
    }

    /// An older clone cannot testify that a branch is gone.
    #[test]
    fn an_older_clone_is_not_a_witness() {
        let subject_ = subject("subject", clone_of("main", 1, 200));
        let older = clone_of("other", 2, 100);
        assert!(!fresher(&older, &subject_));
    }

    /// A clone that fetches one branch has nothing to say about another.
    #[test]
    fn a_single_branch_clone_is_not_a_witness() {
        let subject_ = subject("subject", clone_of("main", 1, 100));
        let mut narrow = clone_of("other", 2, 200);
        narrow.complete = false;
        assert!(!fresher(&narrow, &subject_));
        assert!(!complete(&[String::from("+refs/heads/main:refs/remotes/origin/main")]));
        assert!(complete(&[String::from("+refs/heads/*:refs/remotes/origin/*")]));
    }

    /// A commit two clones both hold survives the deletion of either one, so the shared
    /// tip has to stay in each one's list of what is held elsewhere.
    #[test]
    fn a_tip_two_clones_share_is_held_elsewhere_for_both_of_them() {
        let shared = oid(7);
        let left = subject("left", Evidence { tips: vec![shared.clone()], ..Evidence::default() });
        let right =
            subject("right", Evidence { tips: vec![shared.clone()], ..Evidence::default() });
        let held = held_by_others(&[left, right]);
        assert_eq!(held[0], vec![shared.clone()]);
        assert_eq!(held[1], vec![shared]);
    }

    /// A tip only this clone holds is not proof of anything about this clone.
    #[test]
    fn a_tip_only_one_clone_holds_is_held_nowhere_else() {
        let lone = subject("lone", Evidence { tips: vec![oid(7)], ..Evidence::default() });
        let other = subject("other", Evidence { tips: vec![oid(8)], ..Evidence::default() });
        assert_eq!(held_by_others(&[lone, other])[0], vec![oid(8)]);
    }

    /// A shallow clone's object store stops at a depth, so a tip in it is no promise
    /// that the history behind the tip is there too.
    #[test]
    fn a_shallow_clone_stands_for_nothing() {
        let deep = subject("deep", Evidence { tips: vec![oid(7)], ..Evidence::default() });
        let shallow = subject(
            "shallow",
            Evidence { tips: vec![oid(7)], shallow: true, ..Evidence::default() },
        );
        assert!(held_by_others(&[deep, shallow])[0].is_empty());
    }
}
