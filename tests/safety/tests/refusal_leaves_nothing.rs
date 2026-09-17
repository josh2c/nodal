//! What a refused `nodal new` and a refused `nodal adopt` leave behind: nothing.
//!
//! A refusal is a sentence a person acts on. "The `post_new` hook is not approved" says
//! that the project declares a command this machine has not accepted, and a person who
//! reads it believes no unit was made. On 0.1.0-rc.2 one was: the approval was asked for
//! after the clone, the install and the registry write, so the refused create left a row
//! in `nodal ls`, a branch, a port lease and a home of several megabytes that nobody
//! would ever go looking for.
//!
//! So the rule this file holds, which `nodal reclaim` already held for an unread process
//! table: **every refusal an operation can give is given before its first step writes**.
//! A refusal that can only be found part-way through a run is allowed, because the run
//! rolls back through its journal and the outcome is the same — nothing.
//!
//! The readings are the four things a create makes, and all four are taken from outside
//! the product: the rows in the registry, the homes on the disk, the port leases, and the
//! checkout's own refs. A create that was refused and left one of them is a create whose
//! refusal was a lie.
//!
//! Nothing here approves the hooks it declares. Every other hook suite does
//! (`tests/hook_processes.rs`), because every property in those is about a hook that ran.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use nodal_core::model::{PortAllocation, Unit};
use nodal_core::store::{port_allocations, projects, units};
use nodal_safety::InState as _;
use nodal_safety::{Machine, git, git_ok, stderr, stdout};

/// The unit a refused command would have made.
const UNIT: &str = "worker-import";

/// A hook line no machine in this file has approved.
const UNAPPROVED: &str = "post_new = 'echo the hook ran'";

/// A branch of the project that nothing has checked out, for the materialised adoption.
const ORPHAN: &str = "feature/orphan";

/// Every unit row this machine holds, of every project.
///
/// Read through the projects rather than through one project, because a refused create
/// has registered the project and a test that assumed otherwise would panic instead of
/// reporting the row it found.
fn rows(machine: &Machine) -> Vec<Unit> {
    let store = machine.store();
    projects::list(store.conn())
        .unwrap()
        .into_iter()
        .flat_map(|project| units::list(store.conn(), project.id).unwrap())
        .collect()
}

/// Every port lease this machine holds.
fn leases(machine: &Machine) -> Vec<PortAllocation> {
    port_allocations::list_in_range(machine.store().conn(), 0, u16::MAX).unwrap()
}

/// Insist that the refused command made no unit, in every place a unit is recorded.
fn no_unit(machine: &Machine, told: &str) {
    assert_eq!(rows(machine), Vec::new(), "the refusal left a unit row: {told}");
    assert_eq!(machine.homes(), Vec::<std::path::PathBuf>::new(), "it left a home: {told}");
    assert_eq!(leases(machine), Vec::new(), "it left a port lease: {told}");
    let listed = stdout(&machine.nodal(&["ls"]));
    assert!(listed.contains("no units yet"), "a refused command is listed as a unit: {listed}");
}

/// **The central property.** A create refused for an unapproved hook makes nothing.
///
/// The hook itself is correct to not run. What was wrong is when the question was asked:
/// after the home, the branch, the ports and the rows existed.
#[test]
fn a_new_under_an_unapproved_hook_leaves_no_unit() {
    let machine = Machine::declaring(UNAPPROVED);
    let before = nodal_safety::git::untouched(&machine.source);

    let refused = machine.nodal(&["new", "--name", UNIT, "worker import"]);

    let told = stderr(&refused);
    assert!(!refused.status.success(), "an unapproved hook did not refuse the create: {told}");
    assert!(told.contains("post_new"), "the refusal does not name the hook: {told}");
    assert!(told.contains("not approved"), "{told}");
    no_unit(&machine, &told);
    before.assert_unchanged(
        &nodal_safety::git::untouched(&machine.source),
        "a refused create took the checkout's branch",
    );
    assert_eq!(
        machine.bases(),
        Vec::<std::path::PathBuf>::new(),
        "the refusal was asked after the base was built, which is minutes of work for nothing"
    );
}

/// The same for an adoption that makes a home. A branch nothing has checked out is
/// materialised the way a create is, and it runs the same hook.
#[test]
fn an_adopt_under_an_unapproved_hook_leaves_no_unit() {
    let machine = Machine::declaring(UNAPPROVED);
    git_ok(&machine.source, &["branch", ORPHAN]);
    let before = nodal_safety::git::untouched(&machine.source);

    let refused = machine.nodal(&["adopt", ORPHAN]);

    let told = stderr(&refused);
    assert!(!refused.status.success(), "an unapproved hook did not refuse the adoption: {told}");
    assert!(told.contains("post_new"), "the refusal does not name the hook: {told}");
    no_unit(&machine, &told);
    before.assert_unchanged(
        &nodal_safety::git::untouched(&machine.source),
        "a refused adoption wrote in the checkout it was refusing to adopt",
    );
    assert_eq!(
        git(&machine.source, &["branch", "--list", ORPHAN]).trim(),
        ORPHAN,
        "the branch the adoption refused is still the project's"
    );
}

/// An adoption in place runs no `post_new` ([`nodal_core::lifecycle::ops::adopt`]), so a
/// hook nobody approved does not refuse it. A guard that refused every form would stop a
/// person registering a checkout they are working in, over a command that would never run.
#[test]
fn an_adopt_in_place_is_not_refused_by_a_hook_it_never_runs() {
    let machine = Machine::declaring(UNAPPROVED);
    let worktree = machine.source.parent().unwrap().join("orphan");
    nodal_safety::git::worktree(&machine.source, &worktree, ORPHAN);

    let adopted = machine.nodal_in(&worktree, &["adopt", ".", "--in-place"]);

    assert!(adopted.status.success(), "{}", stderr(&adopted));
    assert_eq!(rows(&machine).len(), 1, "the checkout was not registered");
}

/// A refusal that can only be found part-way through a run rolls the run back to nothing.
///
/// The one used here is real and not injected: `--carry` reads the checkout's untracked
/// work, and a home that already holds one of those paths with other content is refused
/// by the last step of the plan rather than by any check before it
/// ([`nodal_core::git::carry`]). The activation file every home gets is such a path.
#[test]
fn a_refusal_in_the_middle_of_a_run_rolls_back_to_nothing() {
    let machine = Machine::new();
    std::fs::write(machine.source.join(".envrc"), "use flake\n").unwrap();

    let refused = machine.nodal(&["new", "--name", UNIT, "worker import", "--carry"]);

    let told = stderr(&refused);
    assert!(!refused.status.success(), "the collision was not refused: {told}");
    assert!(told.contains(".envrc"), "the refusal does not name the path it collided on: {told}");
    assert!(told.contains("home.carry"), "it does not say which step refused: {told}");
    no_unit(&machine, &told);
}

/// The name a person asked for is the name they get once they approve the hook.
///
/// This is what a leftover row costs even after somebody notices it. A refused create
/// that held the slug would send the second attempt to `worker-import-2`, and the unit,
/// its branch and its home would carry the suffix for as long as the work lasts.
#[test]
fn the_unit_takes_its_plain_name_after_the_hook_is_approved() {
    let machine = Machine::declaring(UNAPPROVED);
    let refused = machine.nodal(&["new", "--name", UNIT, "worker import"]);
    assert!(!refused.status.success(), "{}", stderr(&refused));

    let approved = machine.nodal(&["approve", "--yes"]);
    assert!(approved.status.success(), "the hooks were not approved: {}", stderr(&approved));
    let made = machine.nodal(&["new", "--name", UNIT, "worker import"]);

    assert!(made.status.success(), "{}", stderr(&made));
    let slugs: Vec<String> = rows(&machine).iter().map(|unit| unit.slug.to_string()).collect();
    assert_eq!(slugs, [UNIT], "the refused create held the name it did not keep");
}
