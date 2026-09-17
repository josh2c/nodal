//! A stale remote-tracking ref never makes a destructive operation safe.
//!
//! `tests/reclaim_refusal.rs` is about work a home visibly holds: a changed file, an
//! untracked file, a commit nothing else has. This file is about the one case where the
//! home looks clean and is not.
//!
//! A home writes `refs/remotes/origin/*` when `nodal done` pushes its branch and never
//! corrects them afterwards, because nothing in Nodal ever fetches a home's `origin`.
//! Delete that branch on the remote — which is what a host does the moment a pull
//! request merges — and the home is left naming a commit the remote has not got. A check
//! that read the name would move the only copy of that commit to the trash and print
//! "nothing that is only here". That is a false safe, and it is the one answer a
//! destructive operation must never give.
//!
//! So the home's own refs are believed only where a **witness** confirms them, and the
//! witness is the person's own checkout, which is the repository on this machine that
//! actually fetches `origin` ([`nodal_core::lifecycle::witness`]). With no witness
//! nothing about the remote is proved, and the operation refuses and says so.
//!
//! Every property here runs on a machine whose checkout has a bare `origin` beside it
//! ([`Machine::with_remote`]), and every `git` Nodal starts on that machine may use the
//! local file transport and nothing else. A property that reached a network would fail
//! rather than succeed on whichever host happened to be online.
//!
//! | property | test |
//! |---|---|
//! | a stale ref is not proof | `a_stale_remote_tracking_ref_does_not_make_a_reclaim_safe` |
//! | the same for the sweep | `the_sweep_refuses_the_unit_a_reclaim_refuses_and_says_why` |
//! | a current reading is proof | `a_checkout_that_read_the_remote_proves_the_commit_is_reconstructable` |
//! | a second copy here is proof | `a_second_copy_on_this_machine_settles_it_without_the_remote` |
//! | a rewritten branch is not | `a_rewritten_branch_does_not_vouch_for_the_tip_it_dropped` |
//! | another remote does not answer | `a_ref_of_another_remote_does_not_answer_for_origin` |
//! | a partial checkout witnesses nothing | `a_checkout_that_fetches_one_branch_witnesses_nothing` |
//! | a name is not an object store | `a_ref_naming_a_commit_its_own_store_lost_vouches_for_nothing` |
//! | a witness has to be the later reading | `a_checkout_that_has_not_read_the_remote_since_the_push_is_not_a_witness` |

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::model::{Timestamp, UnitStatus};
use nodal_core::store::units;
use nodal_safety::InState as _;
use nodal_safety::{Machine, git, platform, stderr, stdout};

/// The unit every property here reclaims. It is one of the fixture's own handles.
const SLUG: &str = "worker-import";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const ONLY: &str = "only-here.txt";

/// The branch a home pushes its work to, as a review branch on the remote.
const TOPIC: &str = "topic";

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// No proxy either, for the same reason.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// A machine with a remote, whose every Nodal command is held to the filesystem.
fn machine() -> Machine {
    Machine::with_remote().with_env(ONLY_LOCAL).with_env(NO_PROXY)
}

/// A unit whose one commit exists only in its home, pushed to `origin` as `TOPIC`.
///
/// The push is what writes the home's own `refs/remotes/origin/topic`. After it, the
/// home holds a ref naming a commit, and what the remote does with that branch next is
/// what each property here decides.
fn pushed(machine: &Machine, slug: &str) -> (PathBuf, String) {
    let home = machine.unit(slug);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    git(&home, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{TOPIC}")]);
    assert_eq!(
        git(&home, &["rev-parse", &format!("refs/remotes/origin/{TOPIC}")]),
        tip,
        "the push wrote the home's own remote-tracking ref, which is what is stale later"
    );
    (home, tip)
}

/// The same, with the branch then deleted on the remote, as a host does after a merge.
fn stranded(machine: &Machine, slug: &str) -> (PathBuf, String) {
    let (home, tip) = pushed(machine, slug);
    git(machine.origin(), &["update-ref", "-d", &format!("refs/heads/{TOPIC}")]);
    assert!(!on_remote(machine, &tip), "the remote still reaches the commit");
    (home, tip)
}

/// Whether any ref of the remote reaches this commit.
fn on_remote(machine: &Machine, tip: &str) -> bool {
    git(machine.origin(), &["rev-list", "--all"]).lines().any(|line| line == tip)
}

/// Insist that the home is where it was and plain Git still reaches the commit.
///
/// A refusal that had moved the home first would be no refusal, and one that left a
/// directory the person's own `git` could not read the work out of would be worse.
fn intact(machine: &Machine, home: &Path, tip: &str) {
    assert!(home.is_dir(), "the refusal moved the home");
    assert_eq!(machine.homes(), vec![home.to_path_buf()], "the home is still the project's");
    assert!(machine.trashed().is_empty(), "the refusal put something in the trash");
    assert_eq!(git(home, &["cat-file", "-t", tip]), "commit", "the commit is not readable");
    assert_eq!(git(home, &["rev-parse", "HEAD"]), tip, "the branch no longer names the commit");
    assert_eq!(std::fs::read_to_string(home.join(ONLY)).unwrap(), "the only copy\n");
}

/// The invariant. The home's own ref says the commit is on the remote, the remote does
/// not have it, and nothing on this machine read the remote to tell the two apart.
#[test]
fn a_stale_remote_tracking_ref_does_not_make_a_reclaim_safe() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "a stale ref reclaimed the only copy: {}", stdout(&refused));
    let told = stderr(&refused);
    assert!(told.contains("commits no current reading proves a remote has (1)"), "{told}");
    assert!(told.contains(&tip[..8]), "the refusal does not name the commit: {told}");
    assert!(told.contains("nothing here read the remote to check them"), "{told}");
    intact(&machine, &home, &tip);
}

/// The collection path says the same thing. `nodal gc` reclaims a merged unit whose
/// retention has run out, and it does it through the ordinary reclaim, so a unit a
/// reclaim refuses is a line of the sweep's report rather than a home the sweep took.
#[test]
fn the_sweep_refuses_the_unit_a_reclaim_refuses_and_says_why() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    merged_and_due(&machine, SLUG);

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains(SLUG), "the sweep does not name the unit it kept: {report}");
    assert!(report.contains("commits no current reading proves a remote has (1)"), "{report}");
    intact(&machine, &home, &tip);
}

/// A control, and the reason this is a proof rather than a refusal to answer. The person
/// fetches, their checkout reads the remote, and the reclaim goes ahead.
#[test]
fn a_checkout_that_read_the_remote_proves_the_commit_is_reconstructable() {
    let machine = machine();
    let (home, _) = pushed(&machine, SLUG);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);

    let allowed = platform::reclaim(|args| machine.nodal(args), &["reclaim", SLUG]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert!(stdout(&allowed).contains("nothing that is only here"), "{}", stdout(&allowed));
    assert!(!home.exists(), "the home is not where it was");
    assert_eq!(machine.trashed().len(), 1, "the trash holds it");
}

/// The second control: the question that needs no ref at all. Another tree on this
/// machine holds every commit, so removing this one loses nothing whatever the remote
/// has, and the remote is never asked.
#[test]
fn a_second_copy_on_this_machine_settles_it_without_the_remote() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    git(&machine.source, &["fetch", "--quiet", home.to_str().unwrap(), "HEAD"]);
    assert_eq!(git(&machine.source, &["cat-file", "-t", &tip]), "commit");

    let allowed = platform::reclaim(|args| machine.nodal(args), &["reclaim", SLUG]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert_eq!(machine.trashed().len(), 1, "the trash holds it");
}

/// A branch keeps its name through a rewrite and drops its old commits. The checkout has
/// read the remote, so it is a witness, and what it vouches for is the tip the branch
/// has now — not the tip this home pushed and the rewrite threw away.
#[test]
fn a_rewritten_branch_does_not_vouch_for_the_tip_it_dropped() {
    let machine = machine();
    let (home, tip) = pushed(&machine, SLUG);
    git(&machine.source, &["push", "--quiet", "--force", "origin", &format!("main:{TOPIC}")]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    assert!(!on_remote(&machine, &tip), "the rewrite left the old tip on the remote");

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "a rewritten branch vouched for a dropped tip");
    let told = stderr(&refused);
    assert!(told.contains("commits no current reading proves a remote has (1)"), "{told}");
    intact(&machine, &home, &tip);
}

/// A project may have a `backup` it mirrors to. Pushing there says nothing about whether
/// `origin` has the commit, and a ref under `refs/remotes/backup/` may not answer for one.
#[test]
fn a_ref_of_another_remote_does_not_answer_for_origin() {
    let machine = machine();
    let elsewhere = machine.source.parent().unwrap().join("backup.git");
    let named = elsewhere.to_str().unwrap();
    git(&machine.source, &["init", "--quiet", "--bare", "--initial-branch", "main", named]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);

    let home = machine.unit(SLUG);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    git(&home, &["remote", "add", "backup", named]);
    git(&home, &["push", "--quiet", "backup", &format!("HEAD:refs/heads/{TOPIC}")]);
    assert_eq!(git(&home, &["rev-parse", &format!("refs/remotes/backup/{TOPIC}")]), tip);

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "a ref of another remote answered for origin");
    let told = stderr(&refused);
    assert!(told.contains("commits no current reading proves a remote has (1)"), "{told}");
    intact(&machine, &home, &tip);
}

/// A checkout that fetches one branch cannot say another is gone, so it witnesses
/// nothing at all. Nothing here then reads the remote, and the refusal says that rather
/// than pretending to a reading it does not have.
#[test]
fn a_checkout_that_fetches_one_branch_witnesses_nothing() {
    let machine = machine();
    let narrow = "+refs/heads/main:refs/remotes/origin/main";
    git(&machine.source, &["config", "remote.origin.fetch", narrow]);
    let (home, tip) = stranded(&machine, SLUG);

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "a partial checkout answered for the remote");
    let told = stderr(&refused);
    assert!(told.contains("commits no current reading proves a remote has (1)"), "{told}");
    assert!(told.contains("nothing here read the remote to check them"), "{told}");
    assert!(told.contains("Fetch in the project checkout"), "{told}");
    intact(&machine, &home, &tip);
}

/// A witness vouches with its objects and never with a name.
///
/// The checkout names the home's commit on `origin/topic`, exactly as it would after a
/// fetch, and its object store does not hold it — the shape a pruned store, or one
/// restored without its objects, leaves behind. The tip is read as `--not <commit>`
/// inside the home, which does hold the commit, so a proof drawn from the name alone
/// lands and the home is called clean over its own only copy.
///
/// Both destructive paths are asserted, because the sweep reaches the same check.
#[test]
fn a_ref_naming_a_commit_its_own_store_lost_vouches_for_nothing() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    name_without_objects(&machine, &tip);

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "a name with no objects behind it vouched");
    let told = stderr(&refused);
    assert!(told.contains("commits no current reading proves a remote has (1)"), "{told}");
    intact(&machine, &home, &tip);

    merged_and_due(&machine, SLUG);
    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains("commits no current reading proves a remote has (1)"), "{report}");
    intact(&machine, &home, &tip);
}

/// Being the repository a person usually fetches in is not a reading of the remote.
///
/// The checkout read `origin` before the home pushed, so it has never seen the branch
/// the home wrote. It may not be called the newest reading of that remote, and the
/// refusal has to say that nothing here read it rather than name a reading it does have.
#[test]
fn a_checkout_that_has_not_read_the_remote_since_the_push_is_not_a_witness() {
    let machine = machine();
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    let (home, tip) = stranded(&machine, SLUG);

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "an older reading answered for the remote");
    let told = stderr(&refused);
    assert!(told.contains("commits no current reading proves a remote has (1)"), "{told}");
    assert!(told.contains("nothing here read the remote to check them"), "{told}");
    assert!(!told.contains("the newest reading of the remote here"), "{told}");
    intact(&machine, &home, &tip);
}

/// Give the checkout a `refs/remotes/origin/topic` naming `tip`, with no object behind
/// it. Writing the ref by hand is the only way to hold the name and the objects apart:
/// `git update-ref` refuses a commit the repository does not have.
fn name_without_objects(machine: &Machine, tip: &str) {
    let reference = machine.source.join(".git/refs/remotes/origin").join(TOPIC);
    std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
    std::fs::write(&reference, format!("{tip}\n")).unwrap();
    assert!(
        !nodal_safety::try_git(&machine.source, &["cat-file", "-e", tip]).status.success(),
        "the checkout holds the object, so this asserts nothing about a name"
    );
}

/// Mark the unit merged and let its home's retention run out, which is the one state
/// `nodal gc` reclaims a live home in.
fn merged_and_due(machine: &Machine, slug: &str) {
    let recipe = machine.source.join("nodal.toml");
    let written = std::fs::read_to_string(&recipe).unwrap();
    std::fs::write(&recipe, format!("{written}\n[reclaim]\ntrash_retention = 0\n")).unwrap();
    let store = machine.store();
    let project = machine.project(&store);
    let unit = units::list(store.conn(), project.id)
        .unwrap()
        .into_iter()
        .find(|unit| unit.slug.as_str() == slug)
        .expect("the unit is registered");
    let merged = units::update_status(store.conn(), unit.id, UnitStatus::Merged, Timestamp::now())
        .expect("the unit is moved to merged");
    assert!(merged, "no row was moved to merged");
}
