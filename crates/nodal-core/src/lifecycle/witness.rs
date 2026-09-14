//! What a destructive operation may believe already exists outside a unit home.
//!
//! Removing a home is safe exactly where every commit in it exists somewhere else. Nodal
//! makes no network call, so "somewhere else" is answered from this disk, in two parts:
//!
//! | evidence | what it proves | how it can fail |
//! |---|---|---|
//! | the project's checkout holds the commit | the commit survives this home | nothing: it is a second copy |
//! | a remote-tracking ref holds the commit | the commit reached the remote once | the branch may be deleted or rewritten since |
//!
//! The first is a fact about an object store and needs no interpretation. The second is
//! a record of a past push, and turning a record into proof is what
//! [`crate::doctor::unique::believed`] does. That rule is one function, and the machine
//! survey and every destructive path call it, so there is one answer to "is this ref
//! still true" rather than two that can disagree.
//!
//! # A home never reads its own origin
//!
//! This is the fact the module rests on, and it is a fact about how Nodal builds a home
//! rather than a guess about a person's habits.
//!
//! A home is a copy of a base, and the base is the one thing that ever runs `git fetch
//! origin` ([`crate::substrate::build`]). After the copy, a home writes
//! `refs/remotes/origin/*` in exactly one situation: `nodal done` pushes its branch.
//! Nothing else in Nodal fetches from a home's `origin`.
//!
//! So a home's `refs/remotes/origin/*` is its record of what it sent, and of what the
//! base held on the day it was built. It is never a reading of the remote as it is now.
//! A branch somebody deleted on the remote after that push is still named there for as
//! long as the home exists, and a check that believed the name would call the only copy
//! of a commit safe to remove. That is the false safe this module exists to remove.
//!
//! # The checkout is the one witness a home has
//!
//! The person's own checkout is the repository on this machine that fetches and pulls.
//! It stands in one of two relations to a home's `origin`, and it witnesses in both:
//!
//! - **it shares the origin**: both were cloned from one remote, so the checkout's
//!   `refs/remotes/origin/*` may be a later reading of the same refs, branch by branch;
//! - **it is the origin**: a project with no remote of its own is cloned from the
//!   checkout itself, so the checkout's `refs/heads/*` is not a reading of the remote —
//!   it is the remote.
//!
//! *May be* is the word in the first of those, and it is checked rather than assumed.
//! Being the repository a person usually fetches in is not a reading of the remote; a
//! checkout nobody has fetched in for a month is as stale as anything else. So a
//! same-remote checkout vouches only where it read `origin` **after** the home last
//! wrote its own reading of it ([`later`]), which is the relation the survey requires of
//! the clone it picks. An unknown date, and an equal one, witness nothing.
//!
//! In every other case it witnesses nothing, and each of those is a way it would
//! otherwise answer a question it cannot answer:
//!
//! | case | why it may not answer |
//! |---|---|
//! | Git will not read it | an unreadable path says nothing |
//! | it is shallow | its object store stops at a depth, so a tip promises no history |
//! | its `origin` is another remote | `backup/main` is not a reading of `origin/main` |
//! | it fetches one branch | a checkout that fetches one branch cannot say another is gone |
//! | it has not read `origin` since the home wrote its refs | its reading is the older one |
//!
//! With no witness, nothing about the remote is proved. That is not "the remote has
//! nothing"; it is "nothing here knows", and [`Elsewhere::witnesses`] is empty to say so.
//!
//! # A witness proves with its objects, never with a name
//!
//! Reading a witness's ref says which commit it calls the branch. It does not say that
//! the witness has that commit, and the two come apart: a repository whose objects were
//! pruned, or that was restored without them, keeps the name over an empty store.
//!
//! That difference is not cosmetic here, because a tip is used as `--not <commit>` inside
//! the **home**, and the home holds the commit. The exclusion lands on the strength of a
//! name read somewhere that has nothing behind it, and the home is called clean over a
//! commit nothing but the home has. So every tip a witness offers is looked for in the
//! witness before it counts ([`holds`]).
//!
//! What that leaves is one kind of proof and not two: a commit in a second object store.
//! A ref only says which commits to go and look for.
//!
//! # The mirror vouches for nothing
//!
//! A home also carries `refs/nodal/origin/*`, the copy Nodal took of the checkout's
//! reading of `origin` ([`crate::git::refs::ORIGIN`]). It is evidence that a home's own
//! refs are stale — it is refreshed from the checkout and it drops what the checkout
//! dropped — and it is not evidence that the remote holds anything.
//!
//! Two reasons, and either is enough. It is a copy of a reading, so this run can read
//! neither when that reading was taken nor whether it covered every branch. And it lives
//! in the object store that is about to be moved to the trash, so it is no second copy
//! of anything. What it implies is a refusal, and a refusal is what a home with no
//! witness gets anyway.
//!
//! Nothing here writes, and nothing here reaches a network.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::doctor::unique::{Evidence, RemoteTip, Subject, believed};
use crate::doctor::{inspect, origin};
use crate::git::{Git, Oid};

/// The remote a home's uniqueness question is about.
///
/// Named, and that is the point. A project may have an `upstream` it was forked from or
/// a `backup` it mirrors to, and neither says whether `origin` has a commit.
const ORIGIN: &str = "origin";

/// Where a repository keeps its own branches.
const HEADS: &str = "refs/heads/";

/// What this machine can prove already exists outside one home.
///
/// The two kinds of tip are kept apart, and that is the whole of what a report can say
/// beyond safe or unsafe. A commit a second object store on this disk holds survives the
/// removal of this home whatever any remote has; a commit only a witnessed reading of
/// the remote reaches survives it only as long as the remote keeps the branch. Both make
/// a reclaim safe and they are different answers to "where else is my work", so a check
/// that merged them could report neither ([`crate::lifecycle::assess`]).
#[derive(Debug, Clone, Default)]
pub struct Elsewhere {
    /// Commits the project's own checkout holds, by a ref of its own.
    ///
    /// A tip and not a history. Every commit behind a tip is held wherever the tip is,
    /// so one `rev-list` in the home answers for all of them at once.
    pub local: Vec<Oid>,
    /// Commits a witnessed reading of the remote proves the remote still holds.
    ///
    /// Empty where nothing here read the remote, which is not the same as the remote
    /// holding nothing: [`Elsewhere::witnesses`] is what tells the two apart.
    pub remote: Vec<Oid>,
    /// The repositories whose reading of the remote was used. Empty means nothing on
    /// this machine read the remote, so nothing about the remote is proved.
    pub witnesses: Vec<PathBuf>,
    /// Whether the witness is the remote itself rather than another clone of it.
    ///
    /// A project with no remote of its own is cloned from the person's checkout, so the
    /// checkout is what `origin` names. Reading it is reading the remote, and what it
    /// does not hold, the remote does not hold. Every other reading is a clone's, and a
    /// clone can only say what it last saw.
    pub direct: bool,
}

/// Everything this machine can say about where `home`'s commits also live.
///
/// `checkout` is the project's own, when this machine still has one. A checkout that is
/// not there, or is no longer a repository, is simply not asked: the answer is then the
/// stricter one, which is the safe direction to be wrong in.
#[must_use]
pub fn elsewhere(home: &Path, checkout: Option<&Path>) -> Elsewhere {
    let Some(path) = checkout else {
        return Elsewhere::default();
    };
    let evidence = inspect::evidence(path);
    if evidence.unreadable.is_some() || evidence.shallow {
        return Elsewhere::default();
    }
    // Every tip of the checkout, whatever it is a checkout of. A commit two trees both
    // hold survives the removal of either one, and that is true of a checkout that may
    // not answer for the remote at all.
    let mine = evidence.tips.clone();
    let relation = relation(home, path);
    let Some(witness) = witness(home, path, relation, evidence) else {
        return Elsewhere { local: holds(path, &mine), ..Elsewhere::default() };
    };
    let subject = Subject { path: home.to_path_buf(), evidence: inspect::evidence(home) };
    let trusted = believed(&subject, &[&witness]);
    // One reading of the object store for both kinds of tip, and the split is made
    // afterwards from the names. Asking twice would cost a second `rev-list` in the
    // checkout to answer a question the first one already answered.
    let (local, remote) = split(&holds(path, &joined(&mine, &trusted.tips)), &mine);
    Elsewhere { local, remote, witnesses: trusted.witnesses, direct: relation == Relation::IsTheRemote }
}

impl Elsewhere {
    /// Every tip, for the caller that only asks whether a commit is somewhere else at
    /// all and does not care which evidence says so.
    #[must_use]
    pub fn tips(&self) -> Vec<Oid> {
        joined(&self.local, &self.remote)
    }
}

/// Two sets of tips as one sorted set with no repeats.
fn joined(left: &[Oid], right: &[Oid]) -> Vec<Oid> {
    let mut all: Vec<Oid> = left.iter().chain(right).cloned().collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// Sort proved tips into the checkout's own and the remote's, by which set named them.
///
/// A commit both name is the checkout's: a second object store on this disk is the
/// stronger of the two proofs, and reporting it as the weaker one would tell a person
/// their work depends on a branch staying on a server when it does not.
fn split(proved: &[Oid], mine: &[Oid]) -> (Vec<Oid>, Vec<Oid>) {
    let mine: std::collections::BTreeSet<&Oid> = mine.iter().collect();
    proved.iter().cloned().partition(|oid| mine.contains(oid))
}

/// The checkout as a witness for this home's `origin`, in whichever relation it has to it.
fn witness(
    home: &Path,
    checkout: &Path,
    relation: Relation,
    mut evidence: Evidence,
) -> Option<Subject> {
    match relation {
        // Two clones of one remote. The checkout's reading replaces the home's only
        // where it is the later one, it covers every branch, and it can therefore say
        // that a branch is gone.
        Relation::SameRemote if evidence.complete && later(&evidence, home) => {}
        // The checkout is what the base was cloned from, so its branches are not a
        // reading of the remote. They are the remote, and reading the authority needs
        // no comparison with anybody's copy of it.
        Relation::IsTheRemote => evidence.remotes = heads(checkout),
        _ => return None,
    }
    Some(Subject { path: checkout.to_path_buf(), evidence })
}

/// The commits of `tips` that the repository they were read out of actually holds.
///
/// A ref is a name and an object store is a fact, and this is where the one is turned
/// into the other. Every tip in this proof arrives as a name: `git for-each-ref` prints
/// the object a ref points at whether or not the object is there, and [`believed`]
/// answers with a witness's own tip for a branch, read from the ref rather than from the
/// objects behind it. A repository whose objects were pruned, or that was restored
/// without them, keeps the name over an empty store.
///
/// That difference decides a reclaim, because a tip is read as `--not <commit>` inside
/// the **home**, and the home holds the commit. The exclusion lands on the strength of a
/// name, and the home is called clean over its own only copy.
///
/// So the whole set is looked for where it was read, in one process, before any of it
/// counts. What survives is a commit in a second object store, which is the only thing
/// that makes removing a directory safe. A repository that will not answer holds nothing.
fn holds(repo: &Path, tips: &[Oid]) -> Vec<Oid> {
    Git::at(repo).held(tips).unwrap_or_default()
}

/// Whether the checkout read `origin` after the home last wrote its own reading of it.
///
/// [`believed`] assumes its caller has already picked a witness that read the remote
/// later than the subject did; the survey picks one by comparing every clone's
/// [`crate::doctor::unique::heard`]. This is that comparison for a home, and the two
/// halves of it are read differently on purpose.
///
/// The checkout's half is `heard`, which is the newest of `FETCH_HEAD`, `packed-refs`
/// and `refs/remotes`. `FETCH_HEAD` is the one file that moves for a fetch which changed
/// nothing, so a reading without it would call a checkout that fetched a minute ago
/// older than one that has not fetched for a month.
///
/// The home's half may not use `FETCH_HEAD`, and this is the whole reason the two are
/// not one function. A home never fetches its `origin`, but Nodal fetches the mirror
/// into it from the checkout, over the filesystem, on any command where the checkout's
/// refs have moved ([`crate::context::refresh`]) — which is exactly the command before a
/// reclaim. That writes the home's `FETCH_HEAD`, and reading it would date the home by
/// Nodal's own local copy and leave every checkout looking older than every home.
///
/// So the home is dated by the only refs it writes about `origin`: its own
/// `refs/remotes/origin/*`, which the base copy and `nodal done`'s push are the only
/// writers of.
///
/// An unknown date on either side, and an equal one, witness nothing. A comparison that
/// cannot be made is not a comparison that passed.
fn later(checkout: &Evidence, home: &Path) -> bool {
    read_since(home, checkout.heard)
}

/// Whether a repository that last heard from the remote at `heard` heard from it after
/// `home` last wrote its own record of that remote.
///
/// The half of the witness rule that costs no process at all: both sides are read with
/// `stat`. It is not the whole rule — [`witness`] also requires that the checkout fetch
/// every branch, which is a reading of its configuration — so a caller that uses this on
/// its own gets the cheap necessary condition and not the proof. `nodal ls` is that
/// caller: it flags a unit whose remote evidence *cannot* be current, for a per-row cost
/// of two `stat` calls, and leaves the proof to `nodal reclaim --check`.
///
/// An unknown date on either side, and an equal one, witness nothing. A comparison that
/// cannot be made is not a comparison that passed.
#[must_use]
pub fn read_since(home: &Path, heard: Option<SystemTime>) -> bool {
    match (heard, wrote_origin(home)) {
        (Some(read), Some(written)) => read > written,
        _ => false,
    }
}

/// When this repository's own `refs/remotes/origin/*` last moved.
///
/// `packed-refs` counts, because a clone's first reading of a remote is packed into it
/// and never written loose. `None` where `.git` is not a directory of this repository's
/// own, or where nothing there says.
fn wrote_origin(repo: &Path) -> Option<SystemTime> {
    let git_dir = repo.join(".git");
    if !git_dir.is_dir() {
        return None;
    }
    let mut newest = modified(&git_dir.join("packed-refs"));
    let mut pending = vec![git_dir.join("refs/remotes").join(ORIGIN)];
    while let Some(path) = pending.pop() {
        newest = newest.max(modified(&path));
        for entry in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            pending.push(entry.path());
        }
    }
    newest
}

/// When a path was last written, `None` when it is not there.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// How a checkout stands to a home's `origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    /// Both were cloned from one remote.
    SameRemote,
    /// The home's `origin` is this checkout, which is how a project with no remote of
    /// its own is built.
    IsTheRemote,
    /// Neither, so the checkout answers for nothing out there.
    Unrelated,
}

/// Which of the three this pair is.
fn relation(home: &Path, checkout: &Path) -> Relation {
    let Some(origin) = named(home) else {
        return Relation::Unrelated;
    };
    if named(checkout).is_some_and(|theirs| theirs == origin) {
        return Relation::SameRemote;
    }
    if resolved(checkout).is_some_and(|path| origin::normalize(&path) == origin) {
        return Relation::IsTheRemote;
    }
    Relation::Unrelated
}

/// The grouping name of a repository's `origin`, `None` when it has none to read.
fn named(repo: &Path) -> Option<String> {
    let url = Git::at(repo).remote_url(ORIGIN).ok()??;
    let name = origin::normalize(&url);
    (!name.is_empty()).then_some(name)
}

/// A path as the filesystem spells it, so two names for one directory compare equal.
fn resolved(path: &Path) -> Option<String> {
    Some(std::fs::canonicalize(path).ok()?.to_str()?.to_owned())
}

/// A repository's own branches, read as the tips a clone of it would fetch.
fn heads(repo: &Path) -> Vec<RemoteTip> {
    Git::at(repo)
        .list_refs(HEADS)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|reference| {
            let branch = reference.name.strip_prefix(HEADS)?.to_owned();
            Some(RemoteTip { branch, oid: reference.oid })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{Relation, elsewhere, named, relation};

    /// A path that is not a repository names no remote, stands in no relation to one,
    /// and witnesses nothing. This is the safe direction: nothing is believed.
    #[test]
    fn a_path_that_is_not_a_repository_witnesses_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let absent = directory.path().join("gone");
        assert_eq!(named(&absent), None);
        assert_eq!(relation(&absent, directory.path()), Relation::Unrelated);
        assert!(elsewhere(&absent, Some(&absent)).witnesses.is_empty());
    }

    /// With no checkout to ask, nothing is believed and nobody vouched.
    #[test]
    fn a_home_with_no_checkout_to_ask_believes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let read = elsewhere(directory.path(), None);
        assert!(read.tips().is_empty(), "{read:?}");
        assert!(read.witnesses.is_empty(), "{read:?}");
    }
}
