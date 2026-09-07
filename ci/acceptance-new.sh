#!/usr/bin/env sh
# Acceptance test for T1.3 and T1.11a: creating a unit from a golden base.
#
# The suite makes units in a real repository and checks the things a person can check
# for themselves: `git status` in a unit says nothing, `git worktree list` names one
# checkout, two units cannot see each other's files, a second unit on one branch is
# refused by name, what the checkout had uncommitted is in no unit, and the second unit
# of a workspace finds its base warm and clones nothing. It then kills a create with
# SIGKILL twice — once while it makes a home, once while it builds its base — and runs
# the next `nodal`, which has to take the first back to nothing and finish the second.
#
# It also covers T1.4, which added a step to the same operation: a unit made from a base
# that holds build caches carries the ones that move and none of the ones that record
# the path they were made at, and the removal is one line of the unit's own log.
#
# It runs under `cargo test --workspace` too. It has its own job because a kill part-way
# through a clone is the sort of thing that behaves differently on one platform, and a
# named job says which.
set -eu

cargo test --locked -p nodal-core --test create
cargo test --locked -p nodal-core --lib workspace::relocate
cargo test --locked -p nodal-cli --test new
echo "acceptance (new): a unit is clean, alone and independent, made from a base, carries no cache keyed to it, and a killed create resolves"
