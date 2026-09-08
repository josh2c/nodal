#!/usr/bin/env sh
# Acceptance test for T1.0: nodal doctor.
#
# The machine is planted rather than found: a checkout with three worktrees inside it,
# one of them locked, two more worktrees of that same checkout that live outside it, a
# build cache nothing has written to for a month, an orphan database directory under
# Nodal's state, and a Docker that answers with one exited container and one unreferenced
# volume. Eight claims against that one machine:
#   - every kind of leftover is found and sized;
#   - a worktree outside the checkout is reported with the facts one inside it gets. The
#     machine builds one nested worktree and two outside ones as the same worktree, and
#     the three rows are compared fact for fact. This is the finding the fix came from: a
#     real machine held twenty-eight worktrees of one repository beside its checkout, and
#     a survey that walked for the ones underneath said it held none;
#   - a worktree registered outside the survey root is still this project's. Which
#     section a row goes in is answered by the repository that named it, not by the
#     directory it sits in, so this project's own unfinished work is never handed to a
#     person as somebody else's;
#   - a worktree's Git state is the fact that was read and not a claim on top of it: a
#     worktree with no commits of its own is `pushed`, never `merged`;
#   - a locked worktree is reported as locked and read no further: no size, no Git state
#     and no intent, because a lock says another tool is working there;
#   - another project's leftovers are a section of their own, with names and sizes only;
#   - a project over the unit threshold is a row of its own;
#   - Docker that is not installed is a note, and the rest of the report is unaffected;
#   - a registry a later Nodal wrote does not end the answer. Every other command refuses
#     that file; doctor reports what needs no registry and states the mismatch as a note
#     carrying both schema versions and the one command that upgrades this copy (DL-034);
#   - the report writes nothing anywhere: every path of the machine has the same name,
#     size and modification time after the report as before it, the worktrees outside the
#     checkout included, because they are directories doctor now opens.
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
cargo build --locked -p nodal-cli
cargo test --locked -p nodal-safety --test doctor_writes_nothing

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test doctor
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-cli --test doctor
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-safety --test doctor_writes_nothing

echo "acceptance (doctor): the machine is reported, sized, left exactly as it was, and named one way"
