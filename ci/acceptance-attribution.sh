#!/usr/bin/env sh
# Acceptance test for attribution.
#
# Five claims, against this machine rather than against a table a test wrote:
#   - a process started with a home's environment is attributed to its unit as certain;
#   - a process started in a plain terminal that only stands in the home is attributed
#     to the same unit as probable, by its directory;
#   - a listener on a port the registry granted to a home is attributed to that unit;
#   - a Docker daemon that is not there is a note and not a failure, and where a daemon
#     is there a container Nodal labelled is attributed to its unit as certain;
#   - a host whose process table cannot be read says so for both signals that need one
#     and still answers with what the rest saw, which is the macOS condition.
#
# A check this machine cannot make reports itself as skipped and says why: no /proc, no
# Docker daemon, an account not in the docker group. The CI job runs on a runner that
# has all three, which is what makes every check real there.
#
# The suite is then run a second time with the temporary directory reached through a
# symbolic link, as `ci/acceptance-list.sh` does and for the same reason. Two of the four
# signals answer with a path, and both answer with the resolved one: `/proc/<pid>/cwd` is
# a link, and Docker resolves a mount before it prints one. A registry row holds the name
# the home was created under. macOS gives that condition to every test for free, because
# `/var` there is a link to `/private/var`; Linux has no such link, so the condition is
# made rather than waited for, and both hosts check it.
set -eu

cargo test --locked -p nodal-core --test attribution
cargo test --locked -p nodal-core --lib runtime::
cargo test --locked -p nodal-core --lib services::docker
cargo test --locked -p nodal-cli --test ps

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-cli --test ps
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test attribution

echo "acceptance (attribution): what is running is attributed to its unit, with a confidence, under a linked path too"
