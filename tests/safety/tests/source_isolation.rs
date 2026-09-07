//! Source isolation: what is written in one unit is in that unit and nowhere else.
//!
//! This is the first thing a person is promised and the last thing they check. Two
//! agents work on one project at once because each has a working copy of its own, and
//! the whole arrangement is worth nothing if a file one of them writes turns up in the
//! other's tree or in the checkout the person is reading.
//!
//! Four directions are asserted, because a break in any one of them is the same loss:
//! a file made, a tracked file changed, a file removed, and a change made in the
//! person's own checkout after the units already existed.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use nodal_safety::{Machine, git};

/// A tracked file of the fixture that every copy starts out holding.
const TRACKED: &str = "apps/web/app/page.tsx";

/// Another one, removed in one unit to show a removal is as local as a write.
const REMOVED: &str = "seed.sql";

#[test]
fn a_write_in_one_unit_is_unseen_in_the_other_and_in_the_source() {
    let machine = Machine::new();
    let one = machine.unit("worker-import");
    let two = machine.unit("payroll-export");
    assert_ne!(one, two, "two units, two homes");

    let original = std::fs::read_to_string(machine.source.join(TRACKED)).unwrap();

    // A file that never existed anywhere.
    std::fs::write(one.join("apps/web/only-here.txt"), "made in one unit\n").unwrap();
    // A tracked file, changed.
    std::fs::write(one.join(TRACKED), "export default function Changed() {}\n").unwrap();
    // A tracked file, removed.
    std::fs::remove_file(one.join(REMOVED)).unwrap();

    assert!(one.join("apps/web/only-here.txt").is_file(), "the unit holds what it wrote");
    for (name, elsewhere) in [("the other unit", &two), ("the source", &machine.source)] {
        assert!(
            !elsewhere.join("apps/web/only-here.txt").exists(),
            "a file written in one unit reached {name}"
        );
        assert_eq!(
            std::fs::read_to_string(elsewhere.join(TRACKED)).unwrap(),
            original,
            "a tracked file changed in one unit changed in {name}"
        );
        assert!(
            elsewhere.join(REMOVED).is_file(),
            "a file removed in one unit was removed from {name}"
        );
    }
}

#[test]
fn a_write_in_the_source_after_a_unit_exists_is_unseen_in_the_unit() {
    let machine = Machine::new();
    let one = machine.unit("worker-import");
    let two = machine.unit("payroll-export");

    std::fs::write(machine.source.join("late.txt"), "written after the units\n").unwrap();
    std::fs::write(machine.source.join(TRACKED), "the person kept working\n").unwrap();

    for home in [&one, &two] {
        assert!(!home.join("late.txt").exists(), "a file made in the source reached a unit");
        assert_ne!(
            std::fs::read_to_string(home.join(TRACKED)).unwrap(),
            "the person kept working\n",
            "an edit in the source reached a unit"
        );
    }
}

#[test]
fn a_unit_stays_clean_while_the_other_is_dirty() {
    let machine = Machine::new();
    let one = machine.unit("worker-import");
    let two = machine.unit("payroll-export");

    std::fs::write(one.join(TRACKED), "changed\n").unwrap();
    std::fs::write(one.join("notes.txt"), "not added\n").unwrap();

    let dirty = git(&one, &["status", "--porcelain", "--untracked-files=all"]);
    assert_eq!(dirty.lines().count(), 2, "the unit that was written in is dirty: {dirty}");
    for (name, clean) in [("the other unit", &two), ("the source", &machine.source)] {
        assert_eq!(
            git(clean, &["status", "--porcelain", "--untracked-files=all"]),
            "",
            "{name} is dirty after a write in another tree"
        );
    }
}
