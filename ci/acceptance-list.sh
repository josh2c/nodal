#!/usr/bin/env sh
# Acceptance test for the list.
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
# The suite is then run a second time with the temporary directory reached through a
# symbolic link. This is the shape macOS gives every test for free, because `/var` there
# is a link to `/private/var` and a running process is given the resolved name: a project
# recorded under one name was invisible to a command run under the other. Linux has no
# such link, so the condition is made rather than waited for, and both hosts check it.
#
# `ci/compare-worktrunk.sh` is the check against a second implementation and runs
# separately, because it needs a reference tool that a CI runner does not have.
set -eu

cargo test --locked -p nodal-core --test list
cargo test --locked -p nodal-core --test output
cargo test --locked -p nodal-cli --test ls -- --nocapture

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-cli --test ls
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test list

echo "acceptance (list): every shape gets its verdict, under a linked path too, and the ten-unit list is timed"
