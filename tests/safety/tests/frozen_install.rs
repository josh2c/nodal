//! An install never writes a file the project tracks, and a unit is never born dirty.
//!
//! `nodal new` on a fresh clone of the founder's own Node project ran `npm install` in the
//! base. The lockfile's `name` disagreed with `package.json`, so npm rewrote
//! `package-lock.json`, and every home cloned from that base held a dirty tracked file
//! nobody in it had touched: `nodal ls` read `unique loss`, `nodal reclaim --check`
//! refused over it, and `--force` committed npm's rewrite into the pre-reclaim record.
//! It happened on two release candidates.
//!
//! Three things hold now, and each one is a test here.
//!
//! * **Installs are frozen.** A project that carries a lockfile is installed in the form
//!   that installs from it and refuses to change it. The base's provenance records the
//!   form that ran, and the home reads clean.
//! * **A lockfile that disagrees is a refusal with the reason.** A frozen install that
//!   fails in the tool's own words for that case is reported with those words, the two
//!   files named, and where the fix goes. The create leaves no unit.
//! * **The guard does not depend on the tool.** Whatever an install does to a tracked
//!   file, in the base or in the home, the file is put back in that copy, the step is
//!   refused, and the refusal names the file and the tool. The stub here is a package
//!   manager that exits zero after rewriting `package.json`, which is the case no exit
//!   code catches.
//!
//! The last test runs the `npm` this host has on the founder's shape. A host with none
//! says so on standard output; CI's runners carry `git` and `sh` and nothing else.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;

use nodal_core::store::bases;
use nodal_safety::InState as _;
use nodal_safety::{Machine, git, stderr, stdout};

/// The unit each machine here makes, or is refused.
const UNIT: &str = "frozen";

/// What the stub package manager says when it refuses the lockfile, in pnpm's words.
const REASON: &str = "ERR_PNPM_OUTDATED_LOCKFILE";

/// `git status` over tracked paths in `tree`, which is empty for a clean copy.
fn dirty(tree: &Path) -> String {
    git(tree, &["status", "--porcelain", "--untracked-files=no"])
}

/// Insist that a refused create made no unit.
fn no_unit(machine: &Machine, told: &str) {
    assert!(machine.homes().is_empty(), "the refusal left a home: {told}");
    let listed = stdout(&machine.nodal(&["ls"]));
    assert!(listed.contains("no units yet"), "a refused create is listed as a unit: {listed}");
}

/// The install the base's provenance records, as one line per argument list.
fn recorded_installs(machine: &Machine) -> Vec<String> {
    let store = machine.store();
    let project = machine.project(&store);
    bases::list_for_project(store.conn(), project.id)
        .unwrap()
        .into_iter()
        .filter_map(|base| base.provenance)
        .flat_map(|provenance| provenance.install)
        .map(|argv| argv.join(" "))
        .collect()
}

/// **Installs are frozen.** The fixture carries `pnpm-lock.yaml`, so the base installs
/// with `--frozen-lockfile`, and the home it hands over reads clean.
#[test]
fn a_project_with_a_lockfile_is_installed_frozen_and_the_home_is_clean() {
    let machine = Machine::new();
    let home = machine.unit(UNIT);

    let installs = recorded_installs(&machine);
    assert!(
        installs.iter().any(|argv| argv.ends_with("install --frozen-lockfile")),
        "the base did not install frozen: {installs:?}"
    );
    assert_eq!(dirty(&home), "", "the home is dirty the moment it was made");
}

/// **The guard.** A package manager that rewrites `package.json` and exits zero is
/// refused by name, the file is put back in the base it ran in, and the checkout the
/// base was cloned from is not touched.
#[test]
fn an_install_that_rewrites_a_tracked_file_is_refused_and_the_file_put_back() {
    let machine = Machine::rewriting_a_tracked_file();
    let before = nodal_safety::git::untouched(&machine.source);
    let manifest = std::fs::read_to_string(machine.source.join("package.json")).unwrap();

    let refused = machine.nodal(&["new", "--name", UNIT]);

    let told = stderr(&refused);
    assert!(!refused.status.success(), "an install that rewrote a tracked file was accepted");
    assert!(told.contains("package.json"), "the refusal does not name the file: {told}");
    assert!(told.contains("pnpm"), "the refusal does not name the tool: {told}");
    assert!(told.contains("put back"), "it does not say the change was reverted: {told}");
    no_unit(&machine, &told);

    let kept = machine.partials();
    assert_eq!(kept.len(), 1, "the clone the install ran in is not where a retry finds it");
    assert_eq!(
        std::fs::read_to_string(kept[0].join("package.json")).unwrap(),
        manifest,
        "the tracked file was not put back in the copy the install ran in"
    );
    assert_eq!(dirty(&kept[0]), "", "the copy is still dirty after the refusal");
    before.assert_unchanged(
        &nodal_safety::git::untouched(&machine.source),
        "the guard reached into the checkout",
    );
    assert_eq!(
        std::fs::read_to_string(machine.source.join("package.json")).unwrap(),
        manifest,
        "the checkout's manifest changed"
    );
}

/// **A lockfile that disagrees is a refusal with the reason.** The tool's sentence, the
/// two files, and where the fix goes; and no unit.
#[test]
fn a_lockfile_that_disagrees_refuses_the_create_with_both_files_named() {
    let machine = Machine::refusing_the_lockfile();
    let refused = machine.nodal(&["new", "--name", UNIT]);

    let told = stderr(&refused);
    assert!(!refused.status.success(), "a lockfile the tool refused was installed anyway");
    for expected in [REASON, "pnpm-lock.yaml", "package.json", "in the project"] {
        assert!(told.contains(expected), "the refusal does not say {expected:?}: {told}");
    }
    no_unit(&machine, &told);
}

/// An npm project is installed with `npm ci`, whatever the lockfile records as its name.
#[test]
fn an_npm_project_is_installed_with_npm_ci() {
    let machine = Machine::npm_project("demo-before-the-rename");
    let home = machine.unit(UNIT);

    let ran = std::fs::read_to_string(home.join(Machine::npm_argv())).unwrap();
    assert_eq!(ran.trim(), "ci", "npm was not run in its frozen form");
    assert_eq!(dirty(&home), "", "the home is dirty the moment it was made");
}

/// **The founder's shape, with the real tool.** A lockfile whose name disagrees with
/// `package.json`, installed by the `npm` this host has: the home reads clean and the
/// lockfile is byte for byte the project's.
#[test]
fn the_founders_shape_leaves_the_lockfile_as_the_project_committed_it() {
    let Some(machine) = Machine::npm_project_on_this_host("demo-before-the-rename") else {
        println!("not checked: this host has no npm");
        return;
    };
    let committed = std::fs::read(machine.source.join("package-lock.json")).unwrap();

    let made = machine.nodal(&["new", "--name", UNIT]);
    assert!(made.status.success(), "{}", stderr(&made));
    let home = machine.home_of(UNIT);

    assert_eq!(dirty(&home), "", "the home is dirty the moment it was made");
    assert_eq!(
        std::fs::read(home.join("package-lock.json")).unwrap(),
        committed,
        "npm rewrote the lockfile on the way into the home"
    );
}
