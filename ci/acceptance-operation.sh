#!/usr/bin/env sh
# Acceptance test for T0.11: the operation framework.
#
# The suite runs a real operation in a second process, kills it with SIGKILL while it
# is parked between two steps, and then does what the next `nodal` would do: report the
# interrupted run and resolve it. What it asserts afterwards is that nothing is left —
# no home directory, nothing under the services directory, and no registry row.
#
# It runs under `cargo test --workspace` too. It has its own job because a kill between
# two steps is the sort of thing that fails on one platform and not another, and a
# named job says which.
set -eu

cargo test --locked -p nodal-core --test lifecycle
echo "acceptance (operation): a killed run is reported, rolled back, and leaves nothing"
