//! Git isolation: a unit's repository holds a unit's Git state and nobody else's.
//!
//! A home is a clone and not a linked worktree, which is the decision this file
//! defends. A linked worktree shares one object store, one ref namespace and one stash
//! with every other worktree of the same repository, so a branch made in one is a
//! branch in all of them, and two agents committing at once are two agents in one
//! repository. A clone gives each unit refs, a stash and an index of its own.
//!
//! The second claim is what a person checks by hand: `git worktree list` in a unit
//! names that unit and nothing else. A unit that could see another unit's checkout is a
//! unit an agent can `cd` into.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;

use nodal_safety::{Machine, git, try_git};

/// A tracked file to make a commit and a stash out of.
const TRACKED: &str = "apps/web/app/page.tsx";

/// The ref one unit writes, in the namespace Nodal's own snapshots use.
const REF: &str = "refs/nodal/probe";

#[test]
fn a_branch_a_commit_a_stash_and_a_ref_made_in_one_unit_reach_nothing_else() {
    let machine = Machine::new();
    let one = machine.unit("worker-import");
    let two = machine.unit("payroll-export");

    git(&one, &["switch", "--quiet", "--create", "side-branch"]);
    std::fs::write(one.join(TRACKED), "committed in one unit\n").unwrap();
    git(&one, &["add", "--all"]);
    git(&one, &["commit", "--quiet", "--message", "work only this unit has"]);
    let commit = git(&one, &["rev-parse", "HEAD"]);
    git(&one, &["update-ref", REF, "HEAD"]);
    std::fs::write(one.join(TRACKED), "stashed in one unit\n").unwrap();
    git(&one, &["stash", "push", "--quiet", "--message", "one unit's stash"]);

    // The unit that did the work has all four.
    assert!(git(&one, &["branch", "--list", "side-branch"]).contains("side-branch"));
    assert!(git(&one, &["stash", "list"]).contains("one unit's stash"));
    assert_eq!(git(&one, &["rev-parse", REF]), commit);

    for (name, elsewhere) in [("the other unit", &two), ("the source", &machine.source)] {
        assert_eq!(
            git(elsewhere, &["branch", "--list", "side-branch"]),
            "",
            "a branch made in one unit is in {name}"
        );
        assert_eq!(
            git(elsewhere, &["stash", "list"]),
            "",
            "a stash taken in one unit is in {name}"
        );
        assert_eq!(
            git(elsewhere, &["for-each-ref", "--format=%(refname)", REF]),
            "",
            "a ref written in one unit is in {name}"
        );
        assert!(
            !try_git(elsewhere, &["cat-file", "-e", &commit]).status.success(),
            "a commit made in one unit is in {name}'s object store"
        );
    }
}

#[test]
fn a_unit_is_the_only_worktree_its_repository_has() {
    let machine = Machine::new();
    let one = machine.unit("worker-import");
    let two = machine.unit("payroll-export");
    let base = machine.base();

    for tree in [&one, &two, &base, &machine.source] {
        let listed = git(tree, &["worktree", "list"]);
        assert_eq!(
            listed.lines().count(),
            1,
            "{} lists more than itself:\n{listed}",
            tree.display()
        );
        assert!(names(&listed, tree), "{} does not name itself:\n{listed}", tree.display());
    }

    // And the one line a unit prints is its own home, not another unit's and not the
    // base every home was cloned from.
    let listed = git(&one, &["worktree", "list"]);
    assert!(!names(&listed, &two), "one unit lists another:\n{listed}");
    assert!(!names(&listed, &base), "a unit lists the base it came from:\n{listed}");
}

/// Whether a `git worktree list` names this directory.
///
/// Git prints the path it resolved, and a temporary directory is reached through a link
/// on macOS, so the two names are compared in the one form both agree on.
fn names(listed: &str, tree: &Path) -> bool {
    let resolved = tree.canonicalize().unwrap_or_else(|_| tree.to_path_buf());
    listed.contains(resolved.to_str().expect("a temporary path is UTF-8"))
}
