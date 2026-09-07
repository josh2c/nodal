#!/usr/bin/env sh
# Acceptance test for T1.10: attribution.
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
set -eu

cargo test --locked -p nodal-core --test attribution
cargo test --locked -p nodal-core --lib runtime::
cargo test --locked -p nodal-core --lib services::docker
cargo test --locked -p nodal-cli --test ps
echo "acceptance (attribution): what is running is attributed to its unit, with a confidence"
