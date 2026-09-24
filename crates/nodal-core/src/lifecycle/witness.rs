//! What a destructive operation may believe already exists outside a unit home.
//!
//! Removing a home is safe exactly where every commit in it exists somewhere else. Nodal
//! makes no network call, so "somewhere else" is answered from this disk, in two parts:
//!
//! | reading | what it proves | how it can fail |
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
//! reading of `origin` ([`crate::git::refs::ORIGIN`]). It is reading that a home's own
//! refs are stale — it is refreshed from the checkout and it drops what the checkout
//! dropped — and it is not reading that the remote holds anything.
//!
//! Two reasons, and either is enough. It is a copy of a reading, so this run can read
//! neither when that reading was taken nor whether it covered every branch. And it lives
//! in the object store that is about to be moved to the trash, so it is no second copy
//! of anything. What it implies is a refusal, and a refusal is what a home with no
//! witness gets anyway.
//!
//! # The checkout is read once, and every home is read against that reading
//!
//! Which of the five cases above a home is in depends on the home. What the checkout
//! *is* — its git directory, its refs, its `origin`, and which of its tips its object
//! store really holds — does not. A survey that asked those of each home in turn paid
//! six `git` invocations per home to learn one answer over and over.
//!
//! So [`Checkout::read`] takes that reading once and [`elsewhere`] is given it
//! ([`Checkout`]). A survey reads one; a command about one home reads one and uses it
//! once. The reading is the same reading either way, so no verdict moves.
//!
//! Nothing here writes, and nothing here reaches a network.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Result;
use crate::doctor::unique::{self, CloneReading, RemoteTip, Subject, Trusted, believed};
use crate::doctor::{inspect, origin};
use crate::git::{Git, Oid, fetched, refs, union};
use crate::model::Timestamp;

/// The remote a home's uniqueness question is about.
///
/// Named, and that is the point. A project may have an `upstream` it was forked from or
/// a `backup` it mirrors to, and neither says whether `origin` has a commit.
const ORIGIN: &str = "origin";

/// Where a repository keeps its own branches.
const HEADS: &str = "refs/heads/";

/// What this machine can prove already exists outside one home.
///
/// Three lists, because "somewhere else" is three different answers and a person deciding
/// whether to remove a home wants to know which one they have.
///
/// [`Elsewhere::own`] is the denominator of every question below it. The commits a home
/// has that the checkout's own branches already reach are the project's history rather
/// than this unit's work, and a report that grouped ninety thousand of them would bury
/// the three that matter.
///
/// The other two both make a removal safe and they are not the same promise.
/// [`Elsewhere::remote`] survives losing this laptop and stops surviving if somebody
/// deletes the branch; [`Elsewhere::copies`] is the other way about. Neither is worth
/// anything as a name alone, so every tip in all three is looked for in the object store
/// it was read out of before it counts ([`holds`]).
#[derive(Debug, Clone, Default)]
pub struct Elsewhere {
    /// Commits the checkout reaches from a ref of its own: a branch, a tag, a stash.
    ///
    /// A tip and not a history. Every commit behind a tip is held wherever the tip is,
    /// so one `rev-list` in the home answers for all of them at once.
    pub own: Vec<Oid>,
    /// Commits the checkout holds under a reading of a remote that nothing vouched for,
    /// or with no ref on them at all.
    ///
    /// Objects on this disk, and no statement about any server. A commit fetched into a
    /// checkout by identifier is one of these, and a person rescuing work out of a home
    /// makes exactly one.
    pub copies: Vec<Oid>,
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
    /// The branches the witness's last fetch is older than, and so says nothing about.
    pub unobserved: Vec<String>,
}

impl Elsewhere {
    /// Every commit another object store on this machine holds, however it names it.
    #[must_use]
    pub fn local(&self) -> Vec<Oid> {
        union(&self.own, &self.copies)
    }

    /// Every tip a removal may rest on: what the checkout keeps of its own accord, and
    /// what a dated observation of the remote proves.
    ///
    /// [`Elsewhere::copies`] is not in it, and that is the whole of the difference between
    /// this and a list of what the checkout happens to hold. Those commits sit under a
    /// remote-tracking ref nothing vouched for, and such a ref is the checkout's record of
    /// a fetch or a push that one `git fetch --prune` deletes — exactly as `git gc`
    /// deletes an object under no ref. A verdict that rested a fourteen-day trash timer on
    /// one rested it on the weakest ref there is, and the commits behind it are the
    /// commits the observation rule exists to judge.
    ///
    /// They stay on [`Elsewhere::copies`] because a report says what it found, and a
    /// person deciding what to do next is helped by being told the commit is in their
    /// checkout under `origin/<branch>`.
    #[must_use]
    pub fn tips(&self) -> Vec<Oid> {
        union(&self.own, &self.remote)
    }
}

/// The project's own checkout, read once.
///
/// Everything in it is a fact about the checkout rather than about any home read against
/// it, so a survey of many homes takes one of these and hands it to [`elsewhere`] for
/// each of them.
///
/// A checkout that Git will not read, and a shallow one, witness nothing: [`elsewhere`]
/// stops at the reading and asks them nothing further. So nothing further is read of
/// them here either, and the fields below are empty rather than unknown.
///
/// # One call stack, so a cell needs no lock
///
/// [`Checkout::heads`] is read at most once and kept, which needs interior mutability.
/// It is [`OnceCell`], a cell and not a lock: a `Checkout` is made inside one command,
/// is handed out by reference for the length of that call, and is dropped before the
/// command returns. Nothing shares one between threads, and `ci/measure.sh` holds this
/// workspace to zero asynchronous execution, so there is no executor to move one
/// across. Locking here would buy nothing and would make the type `Sync` by accident,
/// which reads as a promise this module does not make.
///
/// # A reading that failed is not a reading that found nothing
///
/// One reading is taken here and many homes are judged against it, so a failure that
/// was once one home's is now every home's. An empty `held` would say "the checkout
/// holds none of the commits it names", which is a claim, and a `rev-list` that did not
/// run has not earned it. So a failure is written into
/// [`CloneReading::unreadable`], which is the field that already means "this repository
/// could not be read", and [`elsewhere`] treats it exactly as it treats a checkout Git
/// would not open: nothing is believed, and every commit of the home is reported as not
/// checked. That is a refusal, which is what the failure earned before.
#[derive(Debug)]
pub struct Checkout {
    /// Where it is.
    path: PathBuf,
    /// What it can say about where its commits also live, or why it could not be read.
    reading: CloneReading,
    /// The grouping name of its `origin`, `None` when it has none to read.
    origin: Option<String>,
    /// The commits of its own tips that its object store really holds ([`stored`]).
    ///
    /// Empty is a fact about the store and never a reading that failed: a failure is in
    /// `reading.unreadable` instead.
    held: Vec<Oid>,
    /// Its own branches, read the first time a home needs them ([`Checkout::heads`]).
    ///
    /// Lazy and not read with the rest, because only one of the three relations asks for
    /// them ([`Relation::IsTheRemote`]). A command about one home in either of the other
    /// two must pay exactly what it paid before this value existed.
    heads: OnceCell<Vec<RemoteTip>>,
}

impl Checkout {
    /// Read the checkout at `path`.
    ///
    /// Four `git` invocations for the reading, one for the name of `origin`, and one
    /// that looks for every tip it names in its own object store. A caller that reads
    /// one home pays what it always paid; a caller that reads many pays it once.
    ///
    /// A checkout that could not be read is read no further, and says why.
    #[must_use]
    pub fn read(path: &Path) -> Self {
        let mut reading = inspect::reading(path);
        let path = path.to_path_buf();
        let mut origin = None;
        let mut held = Vec::new();
        if reading.unreadable.is_none() && !reading.shallow {
            origin = named(&path);
            match stored(&path, &reading.tips) {
                Ok(found) => held = found,
                Err(why) => reading.unreadable = Some(why.to_string()),
            }
        }
        Self { path, reading, origin, held, heads: OnceCell::new() }
    }

    /// Where it is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its own branches, as the tips a clone of it would fetch.
    ///
    /// A fact about the checkout, like everything else here, and read once however many
    /// homes ask. Only the relation in which the checkout *is* the remote asks at all,
    /// so the reading is taken on the first home that needs it and not before.
    fn heads(&self) -> &[RemoteTip] {
        self.heads.get_or_init(|| heads(&self.path))
    }
}

/// Everything this machine can say about where `home`'s commits also live.
///
/// `checkout` is the project's own, read once, when this machine still has one. A
/// checkout that is not there, or is no longer a repository, is simply not asked: the
/// answer is then the stricter one, which is the safe direction to be wrong in.
///
/// One reading of the checkout's object store covers all three lists, and one reading of
/// its refs covers the split between them. The split is made from the names, and the
/// names arrive with the reading ([`CloneReading::own`]) rather than being read again: the
/// `for-each-ref` that listed the tips already knew which of them were a reading of
/// somewhere else, and asking the same repository twice would cost a process to learn
/// what the first answer held.
///
/// The split is by **ref** and never by commit, and that matters where it looks like it
/// would not: a checkout whose `main` and whose `origin/main` stand at one commit is the
/// ordinary case, and reading that commit as the remote's would take the whole of the
/// project's history out of the denominator and report it back as this unit's work.
#[must_use]
pub fn elsewhere(home: &Path, checkout: Option<&Checkout>, refs: &[refs::Ref]) -> Elsewhere {
    let Some(checkout) = checkout else {
        return Elsewhere::default();
    };
    let reading = &checkout.reading;
    if reading.unreadable.is_some() || reading.shallow {
        return Elsewhere::default();
    }
    let origin = named(home);
    let relation = relation(origin.as_deref(), checkout);
    // No reading of the home at all: the refs its caller already listed are all of it.
    // What is asked of the home here is which branches of `origin` it names — [`believed`]
    // reads that field and no other, and [`observed`] dates each of those branches from
    // files Git wrote. A full reading cost four invocations per home for three fields
    // nothing below looks at.
    let subject = Subject {
        path: home.to_path_buf(),
        reading: CloneReading {
            remotes: unique::named_of(ORIGIN, refs),
            ..CloneReading::default()
        },
    };
    let asked = Asked { checkout, origin, subject: &subject };
    let mut unobserved = Vec::new();
    let trusted = vouched(&asked, relation, reading.clone(), &mut unobserved);
    let Ok(held) = holdings(checkout, &trusted.tips) else {
        return Elsewhere::default();
    };
    let carried: BTreeSet<&Oid> = held.iter().collect();
    let own: Vec<Oid> = reading.own.iter().filter(|oid| carried.contains(oid)).cloned().collect();
    let remote: Vec<Oid> =
        trusted.tips.iter().filter(|oid| carried.contains(oid)).cloned().collect();
    let named: BTreeSet<&Oid> = own.iter().chain(&remote).collect();
    let copies: Vec<Oid> = held.iter().filter(|oid| !named.contains(oid)).cloned().collect();
    Elsewhere {
        own,
        copies,
        remote,
        witnesses: trusted.witnesses,
        direct: relation == Relation::IsTheRemote,
        unobserved,
    }
}

/// The tips the checkout really holds, out of its own and whatever a witness added.
///
/// [`Checkout::read`] already looked for every tip the checkout names, which is the
/// whole of the set wherever nobody witnesses, and is borrowed rather than copied. A
/// witness adds the commits it vouches for, and the ones of those the checkout does not
/// already name are looked for here — so the ordinary home costs no invocation at all,
/// and a witnessed one costs the same single invocation it always did.
///
/// A reading that failed is an error and never an empty list, for the reason
/// [`Checkout`] states: the caller turns it into a refusal.
fn holdings<'a>(checkout: &'a Checkout, trusted: &[Oid]) -> Result<Cow<'a, [Oid]>> {
    let named: BTreeSet<&Oid> = checkout.reading.tips.iter().collect();
    let extra: Vec<Oid> = trusted.iter().filter(|oid| !named.contains(oid)).cloned().collect();
    if extra.is_empty() {
        return Ok(Cow::Borrowed(&checkout.held));
    }
    Ok(Cow::Owned(union(&checkout.held, &vouched_for(&checkout.path, &extra)?)))
}

/// What a witness will vouch for, and nothing at all when there is no witness.
fn vouched(
    asked: &Asked<'_>,
    relation: Relation,
    reading: CloneReading,
    unobserved: &mut Vec<String>,
) -> Trusted {
    let Some(witness) = witness(asked, relation, reading, unobserved) else {
        return Trusted::default();
    };
    believed(asked.subject, &[&witness])
}

/// The pair a witness question is about: the repository that may answer, the remote the
/// question is about, and the home it is asked for.
struct Asked<'a> {
    /// The repository that may answer.
    checkout: &'a Checkout,
    /// The grouping name of the home's `origin`, read once by [`relation`].
    origin: Option<String>,
    /// The home, and what was read of it.
    subject: &'a Subject,
}

/// The checkout as a witness for this home's `origin`, in whichever relation it has to it.
fn witness(
    asked: &Asked<'_>,
    relation: Relation,
    mut reading: CloneReading,
    unobserved: &mut Vec<String>,
) -> Option<Subject> {
    match relation {
        // Two clones of one remote. What the checkout may say is what its last fetch saw
        // and wrote down, branch by branch, and the refspec has to cover every branch or
        // it cannot say that one is gone.
        Relation::SameRemote if reading.complete => {
            reading.remotes = observed(asked, unobserved)?;
        }
        // The checkout is what the base was cloned from, so its branches are not a
        // reading of the remote. They are the remote, and reading the authority needs
        // no comparison with anybody's copy of it.
        Relation::IsTheRemote => reading.remotes = asked.checkout.heads().to_vec(),
        _ => return None,
    }
    Some(Subject { path: asked.checkout.path.clone(), reading })
}

/// The branches of the remote the checkout's last fetch observed, dated against this
/// home's own record of each, and the branches it could not answer for.
///
/// This is the whole of the freshness rule, and the reason it is per branch rather than
/// per repository. A directory time says a repository fetched something; it does not say
/// which branch, and a fetch that dropped a branch from its record is the shape a merged
/// pull request leaves. So each branch the home names is asked of the record Git wrote for
/// the last fetch ([`crate::git::fetched`]).
///
/// The record is a listing and not a lookup, and that is what makes it answer in both
/// directions. A fetch of a remote writes one line per ref it saw, so a branch in the
/// listing is a branch the remote had at that instant, at the commit on the line; and a
/// branch **not** in the listing is a branch the remote did not have. The second is a
/// reading and not a gap, and it is the reading that closes the ordinary shape after a
/// pull request merges: the host drops the branch, the person pulls, the tracking ref
/// stays over a branch the remote has not got, and the record of the fetch says so
/// whether or not that fetch pruned.
///
/// Either reading is worth something only where it is not older than the work. A fetch
/// made before this home pushed says nothing about what the remote has since: a branch
/// absent from it is a branch that did not exist yet, and one present in it stands at a
/// commit from before the push. So a branch whose reading predates the home's own record
/// of it is unobserved, and the row says so.
///
/// The commit vouched for is the one the fetch saw and wrote down, and never the one the
/// tracking ref names. They are the same in the ordinary case and they come apart in
/// exactly the cases this rule is about. It is also why there is no reading of how the
/// checkout's own tracking ref came to move: a push moves such a ref as surely as a fetch
/// does, and under the old rule that mattered, because the ref was the evidence. The
/// evidence is the record of the fetch now, so what moved the ref afterwards cannot reach
/// the proof.
///
/// `None` is a checkout that has made no observation of this remote at all: it has never
/// fetched, or its last fetch was of another remote and rewrote the record with that
/// remote's refs. Neither is a repository that found the remote empty.
fn observed(asked: &Asked<'_>, unobserved: &mut Vec<String>) -> Option<Vec<RemoteTip>> {
    let home = asked.subject.path.as_path();
    let url = asked.origin.as_deref()?;
    let observation =
        fetched::last(&asked.checkout.path, url).filter(|read| !read.seen.is_empty())?;
    let mut seen = Vec::new();
    for tip in &asked.subject.reading.remotes {
        let branch = tip.branch.clone();
        let tracking = format!("{TRACKING}{ORIGIN}/{branch}");
        let wrote = refs::last_moved(home, &tracking);
        match observation.branch(&branch) {
            Some(oid) if not_older(observation.at, wrote) => {
                seen.push(RemoteTip { branch, oid: oid.clone() });
            }
            // A branch the listing does not carry is a branch the remote did not have,
            // which is a reading and not a gap: the fetch asked for the remote's branches
            // and this was not one of them. Nothing is vouched for and nothing is
            // reported, because the question was asked and the answer was no.
            None if strictly_later(observation.at, wrote) => {}
            // Either way the reading predates this home's own record of the branch, so
            // it answers for a state of that branch from before the work.
            _ => unobserved.push(branch),
        }
    }
    Some(seen)
}

/// Whether a reading taken at `read` is not older than the home's own record at `wrote`.
///
/// The test a **positive** observation has to pass, and it is the lenient one of the two
/// on purpose. What proves a commit is the commit the fetch saw and wrote down, so a
/// reading is never believed to reach work it could not have seen; the date only keeps a
/// reading of an older state of the branch from standing as a reading of this one.
///
/// A record nothing dates passes. A home carries every branch its base was copied with,
/// and those refs were packed on the day the base was built and have never moved, so Git
/// keeps no log of them. Refusing there would refuse nearly every branch of nearly every
/// home, and nothing is given away: the sha carries the proof.
///
/// One second counts as not older. Both records are whole seconds, so two things in one
/// second cannot be put in an order, and reading that as "older" refused every home whose
/// push and whose fetch fell in the same second.
fn not_older(read: Timestamp, wrote: Option<Timestamp>) -> bool {
    wrote.is_none_or(|written| read >= written)
}

/// Whether a reading taken at `read` is later than the home's own record at `wrote`.
///
/// The test a **negative** observation has to pass, and it is the strict one, because
/// nothing else guards this direction. Concluding that the remote has not got a branch
/// rests on the reading alone: there is no sha to check it against. A reading taken in
/// the same second as the home's own record may have been taken first, and a branch is
/// absent from such a reading because it did not exist yet rather than because the remote
/// dropped it. So the two have to be in a definite order, and where they are not the
/// branch is unobserved and the row says so.
///
/// A record nothing dates passes, for the reason [`not_older`] gives: a branch of the
/// base that the remote no longer carries is a branch the remote no longer carries.
fn strictly_later(read: Timestamp, wrote: Option<Timestamp>) -> bool {
    wrote.is_none_or(|written| read > written)
}

/// Where a repository keeps its reading of a remote.
const TRACKING: &str = "refs/remotes/";

/// The commits of `tips` that the repository they were read out of actually holds.
///
/// `tips` are that repository's own refs, and this is the one place where having the
/// object is the whole question: a ref names each of these commits, so the object being
/// there is the same fact as a ref reaching it. The reading costs one process for that
/// reason ([`crate::git::Git::stores`]), and [`vouched_for`] asks the dearer question
/// where the identifiers come from somewhere else.
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
/// that makes removing a directory safe.
///
/// A repository that will not answer is an error and not an empty answer. The two look
/// the same in a list of commits and they are not the same fact: one says the store
/// holds none of these, the other says nobody asked it. Only the first may make a home
/// look safe, so the failure is raised and every caller turns it into a refusal.
///
/// # Errors
/// [`crate::Error::Git`] when `rev-list` failed.
fn stored(repo: &Path, tips: &[Oid]) -> Result<Vec<Oid>> {
    Git::at(repo).stores(tips)
}

/// The commits of `wanted` this repository holds, where `wanted` is somebody else's list.
///
/// A witness vouches for a commit this repository never named, so no ref of this one is
/// known to reach it and the object being there proves nothing on its own: `git gc`
/// removes an object under no ref. This is the reachability question
/// ([`crate::git::Git::held`]).
///
/// # Errors
/// [`crate::Error::Git`] when `rev-list` failed.
fn vouched_for(repo: &Path, wanted: &[Oid]) -> Result<Vec<Oid>> {
    Git::at(repo).held(wanted)
}

/// Whether a repository that last heard from the remote at `heard` heard from it after
/// `home` last wrote its own record of that remote.
///
/// The half of the witness rule that costs no process at all: both sides are read with
/// `stat`. It is not the whole rule — [`witness`] also requires that the checkout fetch
/// every branch, which is a reading of its configuration — so a caller that uses this on
/// its own gets the cheap necessary condition and not the proof. `nodal ls` is that
/// caller: it flags a unit whose remote reading *cannot* be current, for a per-row cost
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
fn relation(origin: Option<&str>, checkout: &Checkout) -> Relation {
    let Some(origin) = origin else {
        return Relation::Unrelated;
    };
    if checkout.origin.as_deref().is_some_and(|theirs| theirs == origin) {
        return Relation::SameRemote;
    }
    if resolved(&checkout.path).is_some_and(|path| origin::normalize(&path) == origin) {
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
///
/// One `git for-each-ref`. [`Checkout::heads`] is what calls it, and keeps the answer.
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
    use super::{Checkout, Relation, elsewhere, named, relation};

    /// A path that is not a repository names no remote, stands in no relation to one,
    /// and witnesses nothing. This is the safe direction: nothing is believed.
    #[test]
    fn a_path_that_is_not_a_repository_witnesses_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let absent = directory.path().join("gone");
        assert_eq!(named(&absent), None);
        let none = relation(named(&absent).as_deref(), &Checkout::read(directory.path()));
        assert_eq!(none, Relation::Unrelated);
        assert!(elsewhere(&absent, Some(&Checkout::read(&absent)), &[]).witnesses.is_empty());
    }

    /// With no checkout to ask, nothing is believed and nobody vouched.
    #[test]
    fn a_home_with_no_checkout_to_ask_believes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let read = elsewhere(directory.path(), None, &[]);
        assert!(read.tips().is_empty(), "{read:?}");
        assert!(read.witnesses.is_empty(), "{read:?}");
    }
}
