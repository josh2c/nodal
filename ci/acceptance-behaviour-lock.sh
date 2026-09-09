#!/usr/bin/env sh
# Acceptance test for the behaviour the operation model must keep while it is
# refactored.
#
# Two things are checked.
#
#   1. Every operation's plan has the step keys the lock states, in order, and a plan
#      rebuilt from the journal is the same plan. A create, an adopt and a merge write
#      the same registry rows and events whether they run to completion or are killed
#      and taken over by a later invocation.
#   2. The two divergences this file was written to reproduce are gone. A resumed create
#      writes the relocation event a first run writes, and a resumed reclaim leaves open
#      the session row of a process group its teardown could not stop. Both came from
#      one hole — a step could not hand a value to the registry write, so four
#      operations passed one by hand in a lock the journal knew nothing about, and a
#      rebuilt plan got an empty lock. The journal carries the value now, so both are
#      plain assertions and the `NODAL_ENFORCE_STEP_OUTPUTS` gate they sat behind is
#      gone.
#
# The suite is then run again with the temporary directory reached through a symbolic
# link. This is the shape macOS gives every test for free, because `/var` there is a
# link to `/private/var` and a running process is given the resolved name: a home
# recorded under one name was invisible to a command run under the other. Linux has no
# such link, so the condition is made rather than waited for, and both hosts check it.
set -eu

# Every fixture here states its own state directory, and none of these tests reads the
# registry of whoever is running them. The variable is set anyway, so that a test added
# later cannot reach the real one by leaving it out.
NODAL_HOME=$(mktemp -d)
export NODAL_HOME
trap 'rm -rf "$NODAL_HOME"' EXIT INT TERM

echo "acceptance (behaviour lock): every plan is locked and both resume paths agree"
cargo test --locked -p nodal-core --test behaviour_lock -- --nocapture

echo
echo "acceptance (behaviour lock): the same, under a linked path"
work=$(mktemp -d)
trap 'rm -rf "$work" "$NODAL_HOME"' EXIT INT TERM
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test behaviour_lock

echo
echo "acceptance (behaviour lock): every plan is locked, and neither resume path diverges"
