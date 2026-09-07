#!/usr/bin/env sh
# Acceptance test for T1.3: creating a unit.
#
# The suite makes units in a real repository and checks the four things a person can
# check for themselves: `git status` in a unit says nothing, `git worktree list` names
# one checkout, two units cannot see each other's files, and a second unit on one branch
# is refused by name. It then kills a create with SIGKILL and runs the next `nodal`,
# which has to leave nothing of the killed run behind.
#
# It runs under `cargo test --workspace` too. It has its own job because a kill part-way
# through a clone is the sort of thing that behaves differently on one platform, and a
# named job says which.
set -eu

cargo test --locked -p nodal-core --test create
cargo test --locked -p nodal-cli --test new
echo "acceptance (new): a unit is clean, alone and independent, and a killed create leaves nothing"
