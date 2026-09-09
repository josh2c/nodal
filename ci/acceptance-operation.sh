#!/usr/bin/env sh
# Acceptance test for T0.11: the operation framework.
#
# The suite runs a real operation in a second process, kills it with SIGKILL while it
# is parked between two steps, and then does what the next `nodal` would do: report the
# interrupted run and resolve it. What it asserts afterwards is that nothing is left —
# no home directory, nothing under the services directory, and no registry row.
#
# The other half of the same promise — that a run a second process finishes writes what
# the first process would have written — is `ci/acceptance-behaviour-lock.sh`, which
# compares the two runs of each operation. What this script adds is the ceiling: no
# operation carries a channel of its own for a value the journal should hold.
#
# It runs under `cargo test --workspace` too. It has its own job because a kill between
# two steps is the sort of thing that fails on one platform and not another, and a
# named job says which.
#
# The suite is then run again with the temporary directory reached through a symbolic
# link, as the other acceptance scripts do: a home is recorded by the name the filesystem
# uses, and every path a resolved run compares against goes through that.
set -eu

cargo test --locked -p nodal-core --test lifecycle
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test lifecycle

echo "acceptance (operation): a killed run is reported, rolled back, and leaves nothing"

# The ceiling this script adds: the framework carries what a step produced, so no
# operation carries a channel of its own. A sink is the shape the resume divergence came
# in, and it is cheaper to refuse the shape than to find the divergence again.
if grep -rn 'OnceLock' crates/nodal-core/src crates/nodal-cli/src; then
    echo "acceptance (operation): a step's output goes through the journal, not a sink" >&2
    exit 1
fi
echo "acceptance (operation): no step-output sink is left in product source"
