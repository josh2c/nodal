//! The commands `docs/contracts.md` names are commands the binary has.
//!
//! One claim: every entry in the contract's CLI list is a subcommand `nodal --help`
//! prints. The contract once named `prune`, `status` and three more that no release
//! carried, and nothing read the list against the binary. A flag after the name
//! (`reclaim --check`) is the subcommand's, so the name is what is checked.

#![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

mod state;

use std::collections::BTreeSet;
use std::path::Path;

/// The subcommands the binary prints, by name.
fn subcommands() -> BTreeSet<String> {
    let machine = state::Machine::new();
    let output = machine.nodal().arg("--help").output().unwrap();
    let help = String::from_utf8(output.stdout).unwrap();
    let listed = help.split_once("Commands:\n").unwrap().1;
    listed
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

/// The names the contract's CLI section lists, first word of each entry.
fn contracted() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let section = text.split_once("## CLI\n").unwrap().1;
    let list = section.split_once('`').unwrap().1.split_once('`').unwrap().0;
    list.split(',').filter_map(|entry| entry.split_whitespace().next()).map(str::to_owned).collect()
}

#[test]
fn every_command_the_contract_names_is_one_the_binary_has() {
    let have = subcommands();
    let named = contracted();
    assert!(named.len() > 10, "the list was not read: {named:?}");
    let missing: Vec<&String> = named.iter().filter(|name| !have.contains(*name)).collect();
    assert!(missing.is_empty(), "the contract names commands the binary has not got: {missing:?}");
}
