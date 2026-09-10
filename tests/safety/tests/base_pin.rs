//! A package-manager pin the host cannot satisfy is refused before the clone.
//!
//! A project pins its package manager — `packageManager` in a `package.json`, an
//! `engines` table, a `mise.toml` row — and it pins it because the version matters. A
//! package manager one major version away does not refuse the lockfile. It installs,
//! exits zero, and writes a tree that is subtly not the tree the lockfile describes.
//! Every unit cloned from that base inherits the difference, and the person who trips
//! over it is debugging their own code.
//!
//! So Nodal refuses, and it refuses at the only moment where refusing is cheap: before
//! the clone. A refusal that arrives after twenty thousand files have been written has
//! taken minutes from the person and left them a directory to think about.
//!
//! The machine here is sealed: its path holds the stub package manager and `git` and
//! nothing else. That is what makes the assertion true of a laptop where `corepack` and
//! `mise` happen to be installed, either of which would satisfy the pin instead.
//! `crates/nodal-core/src/substrate/pin.rs` is where those two paths are asserted.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use nodal_safety::InState as _;
use nodal_safety::{Machine, answer, stderr};

#[test]
fn a_pin_this_host_cannot_run_is_refused_by_tool_and_by_version() {
    let machine = Machine::pinning("11.7.0", "10.4.1");
    let refused = machine.nodal(&["base", "build"]);

    assert!(!refused.status.success(), "an install ran at the wrong major version");
    let told = format!("{}{}", answer(&refused), stderr(&refused));
    assert!(told.contains("pnpm"), "the refusal does not name the tool: {told}");
    assert!(told.contains("11.7.0"), "it does not name the version wanted: {told}");
    assert!(told.contains("10.x"), "it does not name the version the host has: {told}");
    assert!(told.contains("corepack enable"), "it does not say how to fix it: {told}");
}

#[test]
fn the_refusal_happens_before_anything_is_cloned() {
    let machine = Machine::pinning("11.7.0", "10.4.1");
    assert!(!machine.nodal(&["base", "build"]).status.success());

    assert!(machine.bases().is_empty(), "a refused build left a base: {:?}", machine.bases());
    assert!(
        machine.partials().is_empty(),
        "a refused build left a clone behind: {:?}",
        machine.partials()
    );
    assert!(machine.homes().is_empty(), "a refused build left a home: {:?}", machine.homes());
}

#[test]
fn a_host_of_the_pinned_major_series_builds() {
    let machine = Machine::pinning("10.7.0", "10.4.1");
    let built = machine.nodal(&["base", "build"]);

    assert!(
        built.status.success(),
        "a host that satisfies the pin was refused: {}",
        stderr(&built)
    );
    assert_eq!(machine.bases().len(), 1, "the build made no base");
}
