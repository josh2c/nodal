#!/usr/bin/env sh
# Acceptance test for T1.11b: the list.
#
# The list is what `nodal ls`, and a bare `nodal`, print. Four things are checked:
#
#   1. Every shape a real branch is in gets the right verdict, read out of real
#      repositories the fixture builds: a branch that never left the base, a branch with
#      work on it, a branch the base moved under, a branch merged, a branch squashed onto
#      the base, and a branch that would conflict. The squash merge is the one that
#      matters: no reading of the commit history finds it.
#   2. Both renderings match their committed snapshots, `--json` included.
#   3. The binary prints one row per unit, puts the units the base has moved under first,
#      and falls back to the help where there is no project.
#   4. The ten-unit list is timed. The number is printed and nothing is gated on it: it
#      depends on the machine and on how many homes Git has to answer for.
#
# `ci/compare-worktrunk.sh` is the fifth check and runs separately, because it needs a
# reference tool that a CI runner does not have.
set -eu

cargo test --locked -p nodal-core --test list
cargo test --locked -p nodal-core --test output
cargo test --locked -p nodal-cli --test ls -- --nocapture
echo "acceptance (list): every shape gets its verdict, and the ten-unit list is timed"
