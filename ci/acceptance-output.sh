#!/usr/bin/env sh
# Acceptance test for T0.7: the output layer.
#
# Every read type is rendered twice — aligned text for a person, JSON for a tool — from
# fixed values, and both renderings are compared against committed snapshots. The status
# stream is driven by a fixed list of answers with a zero interval, so its NDJSON is a
# snapshot too. A column, a label or a JSON key cannot move without the diff showing it.
#
# Rerun the suite with UPDATE_SNAPSHOTS=1 to rewrite the files after a deliberate change.
set -eu

cargo test --locked -p nodal-core --test output
cargo test --locked -p nodal-core --lib output::
cargo test --locked -p nodal-cli --test init
echo "acceptance (output): both renderers match their snapshots"
