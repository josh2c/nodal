#!/usr/bin/env sh
# Acceptance test for adoption and inspection: `nodal adopt`, `nodal show`,
# `nodal explain`.
#
# The suite adopts a nested worktree in a real repository and checks the promise the
# in-place form makes: `git status` in that checkout says exactly what it said before,
# byte for byte, both after the adoption and after the `nodal show` that writes the
# unit's memory into it. It then reclaims the unit and checks the other end of the same
# promise: the registration is given up, Nodal's own files are taken back out, and the
# directory is where it always was with the same `git status` again.
#
# It also checks what a worktree an agent tool made is worth on its own: the unit comes
# out of the adoption carrying the intent recovered from that tool's session record,
# marked as recovered rather than stated, in the registry and in every rendering.
#
# The other form is here too. A branch nothing has checked out gets a home of its own
# from a base, carrying the commits the base has never had; a branch a checkout does
# hold is refused rather than materialised, because a second home for it would leave
# whatever is uncommitted in that checkout behind.
#
# The suite is then run a second time with the temporary directory reached through a
# symbolic link, as `ci/acceptance-reclaim.sh` does: an adopted checkout's home is
# recorded and compared against the name the filesystem itself uses (DL-037), and a host
# that hands a process another name for the same directory is the one that finds it.
set -eu

cargo test --locked -p nodal-cli --test adopt
cargo test --locked -p nodal-core --lib lifecycle::ops::adopt
cargo test --locked -p nodal-core --lib lifecycle::guard
cargo test --locked -p nodal-core --lib env::files
cargo test --locked -p nodal-core --lib doctor::intent
cargo test --locked -p nodal-core --test output

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-cli --test adopt

echo "acceptance (adopt): a checkout becomes a unit where it stands and git status does not move, its intent is recovered, and a reclaim gives the directory back untouched"
