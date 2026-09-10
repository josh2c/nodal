#!/usr/bin/env sh
# Acceptance test for the context compiler.
#
# `WORKUNIT.md` is a unit's memory, and the whole claim about it is that it is computed
# rather than accumulated. The suite drives real commands against real repositories: a
# unit is made with `nodal new`, its tests are run with `nodal run`, and the memory is
# then read as a person would read it after a session ended with nothing written down.
#
# Six claims, in `crates/nodal-cli/tests/context.rs`: a session that ends with no
# handoff still leaves the branch, the diff, the last commands and the failing test run;
# the ledger names what a sibling touched and what the base gained; `ls`, `show`, `new`,
# `merge` and `reclaim` each write the file again; a dozen busy siblings still compile
# inside the size budget with every sibling capped and the cap saying what it dropped;
# the pointer is one line in each vendor file and the home's `git status` stays empty;
# and a `CLAUDE.md` the project tracks is not written in at all.
#
# The pure parts are checked where they are: the cap and the flattening of an event body
# in `context::render`, the pointer's idempotence in `context::pointer`, the
# beside-and-rename write in `context::atomic`, and the two Git parsers in
# `git::history`.
#
# The size budget prints the number it measured on every run, pass or fail, so the
# record a later recalibration needs is in the log of any green run.
set -eu

cargo test --locked -p nodal-cli --test context -- --nocapture
cargo test --locked -p nodal-core --lib context
cargo test --locked -p nodal-core --lib git::history

echo "acceptance (context): a crash with no handoff loses nothing, the ledger names every sibling, and a dozen of them still fit"
