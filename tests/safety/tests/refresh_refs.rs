//! A unit starts where the person's checkout is, and BEHIND is measured against it.
//!
//! A home is a clone of a base, and a base is a clone taken at whatever moment it was
//! built. Everything the home knows about the project is that old: `origin/main` in a
//! home is the commit `origin/main` was at when the base was made, and nothing in Nodal
//! ever moved it. Two consequences, and both were seen on real machines before this
//! suite existed.
//!
//! A unit made on Friday out of a base built on Monday started on Monday. The person
//! typed `nodal new`, was given a home, and wrote a commit on top of a main that four
//! merges had already gone past. Nothing said so.
//!
//! And the BEHIND column was arithmetic about the wrong commits. A checkout last touched
//! three weeks earlier printed `-0 (origin/main)` for every one of its worktrees: the
//! subtraction was right and the data was three weeks old. A number that is confidently
//! wrong is worse than no column, because a person acts on it.
//!
//! The fix is a fetch out of the checkout, by path. So the third property here is that
//! it is still a fetch by path: the whole suite runs with every Git protocol but the
//! filesystem refused, and with the proxy Git would use for the one remaining network
//! transport set to a command that fails. A refresh that reached a network would fail
//! these tests rather than quietly work on the machine of whoever had a remote up.
//!
//! The fourth is about the fork point. Where a unit started is now written down at
//! create, because it was worked out on every read as a merge base, and a merge base
//! moves when somebody rebases. The recorded value does not move, because a unit forks
//! once.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;

use nodal_core::model::CommitId;
use nodal_safety::{InState, Machine, git, git_ok, stdout};

/// A file the fixture project tracks, to make commits on the checkout's main out of.
const TRACKED: &str = "apps/web/app/page.tsx";

/// A second one, for a commit on a unit's own branch that does not collide with the
/// commits the checkout's main is gaining.
const OTHER: &str = "apps/web/app/layout.tsx";

/// Git's own allow-list of transports. Naming the filesystem alone means every network
/// protocol Git has is refused before a connection is attempted, so a fetch that tried
/// one fails loudly instead of succeeding on a machine that happens to be online.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// The command Git runs to reach a proxy. `false` exits non-zero without doing
/// anything, which is what the brief for this work named.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// A machine whose every `git` may use the filesystem and nothing else.
fn offline() -> Machine {
    Machine::new().with_env(ONLY_LOCAL).with_env(NO_PROXY)
}

/// Commit one more line on the checkout's own `main`.
fn advance(source: &Path, line: &str) -> String {
    std::fs::write(source.join(TRACKED), format!("{line}\n")).unwrap();
    git_ok(source, &["add", "--all"]);
    git_ok(source, &["commit", "--quiet", "--message", line]);
    git(source, &["rev-parse", "HEAD"])
}

/// The commit a home's HEAD is on.
fn head(home: &Path) -> String {
    git(home, &["rev-parse", "HEAD"])
}

/// A unit made after a commit to the checkout's main starts at that commit.
///
/// The first unit is what builds the base, so the second is the one that would have
/// started where the base was built. Before this was fixed it started at the first
/// commit and the assertion below read the wrong commit.
#[test]
fn a_unit_starts_at_the_commit_the_checkout_is_at() {
    let machine = offline();
    let first = machine.unit("worker-import");
    let started = head(&first);

    let moved = advance(&machine.source, "a commit the base has never seen");
    assert_ne!(moved, started, "the fixture did not move the checkout");

    let second = machine.unit("payroll-export");
    assert_eq!(
        head(&second),
        moved,
        "a unit made after a commit to main must start at that commit, not at the \
         commit the base was built from"
    );
}

/// The registry records where the unit forked from, and it is that same commit.
#[test]
fn the_registry_records_the_commit_the_unit_forked_from() {
    let machine = offline();
    drop(machine.unit("worker-import"));
    let moved = advance(&machine.source, "a commit the base has never seen");

    let home = machine.unit("payroll-export");
    let store = machine.store();
    let project = machine.project(&store);
    let units = nodal_core::store::units::list(store.conn(), project.id).unwrap();
    let unit = units.iter().find(|unit| unit.slug.as_str() == "payroll-export").unwrap();

    assert_eq!(
        unit.base_commit.as_ref().map(CommitId::as_str),
        Some(moved.as_str()),
        "the fork point the registry holds is the commit the home is actually on"
    );
    assert_eq!(head(&home), moved);
}

/// The list says a unit is behind after a push nobody fetched inside the home.
///
/// The home is never told. The commits reach the checkout and stop there, exactly as a
/// person's own `git pull` in their own directory leaves every home untouched, and the
/// list has to notice anyway.
#[test]
fn the_list_counts_behind_after_a_commit_no_home_ever_fetched() {
    let machine = offline();
    let home = machine.unit("worker-import");
    let before = stdout(&machine.nodal(&["ls"]));
    assert!(before.contains("worker-import"), "the list names the unit: {before}");
    let stood = head(&home);

    advance(&machine.source, "one the home has not got");
    advance(&machine.source, "and a second");

    let listed = stdout(&machine.nodal(&["ls"]));
    assert!(
        listed.contains("-2"),
        "two commits on main that no home fetched must read as two behind:\n{listed}"
    );
    assert_eq!(head(&home), stood, "reading a list moved no branch");
}

/// A unit whose branch carries no work of its own is finished, and the list says so
/// once the base has moved past it.
///
/// This is the founder's lane board: fifteen units whose branches were already merged
/// read `open +N`, because the ref they were compared against was frozen at the base
/// build. With the base refreshed, a branch that is in main's history reads as done.
#[test]
fn a_branch_already_in_main_reads_as_done_and_not_as_open() {
    let machine = offline();
    let home = machine.unit("worker-import");
    std::fs::write(home.join(TRACKED), "work this unit did\n").unwrap();
    git_ok(&home, &["add", "--all"]);
    git_ok(&home, &["commit", "--quiet", "--message", "the unit's own work"]);
    let work = git(&home, &["rev-parse", "HEAD"]);

    // The person merges it in their own checkout, and tells no home about it. The
    // commit lives in the home, so the checkout fetches it by path exactly as a person
    // would, and nothing about that reaches the home.
    let home_path = home.to_str().unwrap();
    git_ok(&machine.source, &["fetch", "--quiet", "--no-tags", "--", home_path, &work]);
    git_ok(&machine.source, &["merge", "--quiet", "--no-ff", "-m", "merged", &work]);

    let listed = stdout(&machine.nodal(&["ls"]));
    assert!(
        listed.contains("done"),
        "a branch main already carries must not read as open:\n{listed}"
    );
}

/// A rebase moves the branch and leaves the recorded fork point where it was.
///
/// The two are different facts. Where the branch stands now is a merge base and it
/// moves; where the unit started is a property of the unit and it does not. Before the
/// registry recorded one, every reader was given the first and told it was the second.
#[test]
fn a_rebase_leaves_the_recorded_fork_point_alone() {
    let machine = offline();
    let home = machine.unit("worker-import");

    let forked = {
        let store = machine.store();
        let project = machine.project(&store);
        let units = nodal_core::store::units::list(store.conn(), project.id).unwrap();
        units.first().unwrap().base_commit.clone().expect("the create recorded a fork point")
    };
    assert_eq!(forked.as_str(), head(&home));

    std::fs::write(home.join(OTHER), "the unit's own work\n").unwrap();
    git_ok(&home, &["add", "--all"]);
    git_ok(&home, &["commit", "--quiet", "--message", "the unit's own work"]);

    let moved = advance(&machine.source, "main moves under the unit");
    // A list is what brings the new commit into the home, which is what makes the
    // rebase below possible without anybody fetching by hand.
    drop(stdout(&machine.nodal(&["ls"])));
    git_ok(&home, &["rebase", "--quiet", &moved]);
    assert_ne!(head(&home), forked.as_str(), "the rebase moved the branch");

    let store = machine.store();
    let project = machine.project(&store);
    let units = nodal_core::store::units::list(store.conn(), project.id).unwrap();
    assert_eq!(
        units.first().unwrap().base_commit.as_ref().map(CommitId::as_str),
        Some(forked.as_str()),
        "a rebase must not rewrite where the unit forked from"
    );
}

/// `--from` names a branch that exists only in the person's own checkout.
///
/// The home is a clone of a base, which is a clone of the remote, so it has never heard
/// of a branch the person made locally. The create copies the checkout's branches in,
/// which is what makes this resolvable at all.
#[test]
fn from_resolves_a_branch_only_the_checkout_has() {
    let machine = offline();
    drop(machine.unit("worker-import"));

    git_ok(&machine.source, &["switch", "--quiet", "--create", "spike"]);
    let tip = advance(&machine.source, "a spike nobody has pushed");
    git_ok(&machine.source, &["switch", "--quiet", "main"]);

    let made = machine.nodal(&["new", "--name", "on-the-spike", "--from", "spike"]);
    assert!(made.status.success(), "nodal new --from spike: {}", nodal_safety::stderr(&made));
    assert_eq!(head(&machine.home_of("on-the-spike")), tip, "--from starts at the spike's tip");
}

/// A `--from` nobody has is refused, and the refusal says which branch and where it was
/// looked for. Never an error without its reason.
#[test]
fn from_a_branch_nothing_has_is_refused_with_its_reason() {
    let machine = offline();
    let made = machine.nodal(&["new", "--name", "nowhere", "--from", "no-such-branch"]);
    assert!(!made.status.success(), "a create from a branch nothing has must fail");
    let said = nodal_safety::stderr(&made);
    assert!(said.contains("no-such-branch"), "the refusal names the branch: {said}");
}
