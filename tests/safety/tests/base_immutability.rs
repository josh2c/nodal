//! Base immutability: the tree every unit is cloned from is read and never written.
//!
//! A base is one clone and one install, shared by every unit of a workspace. That is
//! where the seconds a create takes come from, and it is also the largest thing in the
//! system that two units have in common. If a unit could write into it, then one agent's
//! edit would be in the next unit anybody made, and the fingerprint the base is keyed by
//! would no longer describe the tree it names.
//!
//! Nothing here reads a status line. `git status` says nothing about a file no commit
//! tracks, and the base holds one: the install writes into `node_modules`, which is a
//! row the exclusion table keeps rather than drops. So the base is compared byte for
//! byte, `.git` included, by [`nodal_safety::Snapshot`].
//!
//! Both readings start from a base built by `nodal base build`, before any unit exists.
//! A reading taken after the first create would say nothing about the create itself: a
//! create that wrote the same file into its base every time would leave every later
//! reading equal to the first, and the property would hold while being false.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use nodal_safety::{Machine, Snapshot};

/// How many units the second property makes. Ten is the threshold `nodal doctor` calls
/// a lot of open units, so it is the most a project is expected to hold at once.
const MANY: usize = 10;

/// A tracked file of the fixture, changed in a unit.
const TRACKED: &str = "apps/web/app/page.tsx";

#[test]
fn a_write_in_a_unit_never_reaches_the_base_it_was_cloned_from() {
    let machine = Machine::new();
    let base = machine.build_base();
    let before = Snapshot::of(&base);
    assert!(!before.is_empty(), "the base holds nothing, so this proves nothing");

    // The create is inside the reading, so a create that writes into its base is caught
    // here and not only a unit that does.
    let home = machine.unit("worker-import");
    before.assert_unchanged(&Snapshot::of(&base), "making a unit wrote into its base");

    // Everything a working copy does to a tree: a tracked file changed, a tracked file
    // removed, a new file, and a change to what the install left behind — which is the
    // one thing in the home that no commit tracks and that came from the base.
    std::fs::write(home.join(TRACKED), "changed in the unit\n").unwrap();
    std::fs::remove_file(home.join("seed.sql")).unwrap();
    std::fs::write(home.join("only-here.txt"), "made in the unit\n").unwrap();
    std::fs::write(home.join(Machine::installed()), "rewritten in the unit\n").unwrap();
    std::fs::create_dir_all(home.join("node_modules/added")).unwrap();
    std::fs::write(home.join("node_modules/added/index.js"), "module.exports = {};\n").unwrap();

    before.assert_unchanged(&Snapshot::of(&base), "a unit wrote into its base");
    assert_eq!(
        std::fs::read_to_string(home.join(TRACKED)).unwrap(),
        "changed in the unit\n",
        "the unit kept the writes the base did not take"
    );
}

#[test]
fn a_base_is_unchanged_byte_for_byte_after_ten_units_are_made_from_it() {
    let machine = Machine::new();
    let base = machine.build_base();
    let before = Snapshot::of(&base);

    for index in 0..MANY {
        machine.unit(&format!("unit-{index}"));
    }

    assert_eq!(machine.homes().len(), MANY, "every create made a home of its own");
    assert_eq!(machine.bases().len(), 1, "one workspace, one base");
    before.assert_unchanged(&Snapshot::of(&base), "ten creates changed the base");
}

#[test]
fn every_unit_holds_its_own_copy_of_what_the_base_carried() {
    let machine = Machine::new();
    let base = machine.build_base();
    let one = machine.unit("worker-import");
    let two = machine.unit("payroll-export");

    let installed = Machine::installed();
    for home in [&one, &two] {
        assert!(home.join(installed).is_file(), "a home carries what the install left");
    }
    std::fs::write(one.join(installed), "one unit rewrote it\n").unwrap();

    assert_eq!(
        std::fs::read_to_string(two.join(installed)).unwrap(),
        std::fs::read_to_string(base.join(installed)).unwrap(),
        "one unit's copy of an installed file is shared with another's"
    );
}
