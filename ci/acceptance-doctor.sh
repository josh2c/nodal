#!/usr/bin/env sh
# Acceptance test for T1.0: nodal doctor.
#
# The machine is planted rather than found: a checkout with two nested worktrees, one of
# them locked, a build cache nothing has written to for a month, an orphan database
# directory under Nodal's state, and a Docker that answers with one exited container and
# one unreferenced volume. Six claims against that one machine:
#   - every kind of leftover is found and sized;
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
set -eu

cargo test --locked -p nodal-core --test doctor
cargo test --locked -p nodal-core --lib doctor::
cargo test --locked -p nodal-core --lib output::view::doctor
cargo test --locked -p nodal-core --lib services::docker
cargo test --locked -p nodal-cli --test doctor
echo "acceptance (doctor): the machine is reported, sized, and left exactly as it was"
