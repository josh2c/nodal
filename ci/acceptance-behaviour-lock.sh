#!/usr/bin/env sh
# Acceptance test for W7.0: the behaviour the operation model must keep while it is
# refactored.
#
# Three things are checked.
#
#   1. The suite passes. Every operation's plan has the step keys the lock states, in
#      order, and a plan rebuilt from the journal is the same plan. Two operations write
#      the same registry rows and events whether they run to completion or are killed
#      and taken over.
#   2. The two known divergences still reproduce, and are printed. A create that is
#      resumed writes no relocation event, and a reclaim that is resumed closes the
#      session row of a process group that is still running. Both come from the same
#      hole: a step cannot hand a value to the registry write, so four operations pass
#      one by hand in a lock the journal knows nothing about, and a rebuilt plan gets an
#      empty lock.
#   3. The gate the two sit behind works in both directions. With
#      `NODAL_ENFORCE_STEP_OUTPUTS=1` the two are enforced and must fail today, which is
#      what proves they are testing something. The day the step-output column lands,
#      that run turns green, the default run turns red with the message that says to
#      delete the gate, and this script is edited to keep only the enforced pass.
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

echo "acceptance (behaviour lock): the locks pass and the two reproductions still reproduce"
cargo test --locked -p nodal-core --test behaviour_lock -- --nocapture

echo
echo "acceptance (behaviour lock): the two reproductions fail when they are enforced"
if NODAL_ENFORCE_STEP_OUTPUTS=1 cargo test --locked -p nodal-core --test behaviour_lock \
    > /dev/null 2>&1; then
    echo "the enforced run passed, so the step-output column has landed." >&2
    echo "Delete the expected-failure gate: make the two locks plain assertions," >&2
    echo "and leave only the default run in this script." >&2
    exit 1
fi
echo "  the enforced run fails, as it must until the step-output column lands"

echo
echo "acceptance (behaviour lock): the same, under a linked path"
work=$(mktemp -d)
trap 'rm -rf "$work" "$NODAL_HOME"' EXIT INT TERM
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test behaviour_lock

echo
echo "acceptance (behaviour lock): every plan is locked, and the two reproductions are recorded"
