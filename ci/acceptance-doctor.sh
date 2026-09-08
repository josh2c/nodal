#!/usr/bin/env sh
# Acceptance test for T1.0: nodal doctor.
#
# The machine is planted rather than found: a checkout with two nested worktrees, one of
# them locked, a build cache nothing has written to for a month, an orphan database
# directory under Nodal's state, and a Docker that answers with one exited container and
# one unreferenced volume. Six claims against that one machine:
#   - every kind of leftover is found and sized;
#   - a worktree's Git state is the fact that was read and not a claim on top of it: a
#     worktree with no commits of its own is `pushed`, never `merged`;
#   - a locked worktree is reported as locked and read no further: no size, no Git state
#     and no intent, because a lock says another tool is working there;
#   - another project's leftovers are a section of their own, with names and sizes only;
#   - a project over the unit threshold is a row of its own;
#   - Docker that is not installed is a note, and the rest of the report is unaffected;
#   - the report writes nothing anywhere: every path of the machine has the same name,
#     size and modification time after the report as before it.
#
# The command itself is driven through the binary as well, on a machine that has never
# run Nodal, in both renderings, and against the words a read-only command may not use.
#
# The suites are then run a second time with the temporary directory reached through a
# symbolic link, as `ci/acceptance-list.sh` does and for the same reason. Doctor compares
# paths from four sources, and Git and Docker resolve every link before they answer while
# a shell and a registry row do not. macOS gives that condition to every test for free,
# because `/var` there is a link to `/private/var`; Linux has no such link, so the
# condition is made rather than waited for, and both hosts check it.
set -eu

cargo test --locked -p nodal-core --test doctor
cargo test --locked -p nodal-core --lib doctor::
cargo test --locked -p nodal-core --lib output::view::doctor
cargo test --locked -p nodal-core --lib services::docker
cargo test --locked -p nodal-cli --test doctor

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test doctor
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-cli --test doctor

echo "acceptance (doctor): the machine is reported, sized, left exactly as it was, and named one way"
