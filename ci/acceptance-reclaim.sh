#!/usr/bin/env sh
# Acceptance test for T1.12: reclaim, trash and gc.
#
# The suite reclaims units in a real repository and checks what a person can check for
# themselves: a unit whose home holds uncommitted work, an untracked file or a commit no
# other tree has is refused with the paths named; `--force` goes on and the work is on a
# snapshot ref inside the trashed home; a clean unit's home is in the trash under the
# name it had, its ports are free, and the verification finds nothing left by id; a
# process planted in the home is stopped; the four recipe hooks run in order and a
# command that changed since `nodal init` approved it refuses to run; a checkout adopted
# in place is unregistered and never moved; and `nodal gc` removes a trashed home once
# its retention has run out and leaves the one whose has not.
#
# It also kills a reclaim with SIGKILL while it is stopping a process that ignores being
# asked, and runs the next `nodal`, which has to take the run back and leave the unit
# there to be reclaimed properly.
#
# Stopping needs a process table, which macOS does not publish. The tests that depend on
# one assert the degraded behaviour there rather than standing aside quietly: the reclaim
# finishes, the ports still come back, the plant is still running because nothing could
# see it, and the report carries a note saying the process table went unread.
#
# It runs under `cargo test --workspace` too. It has its own job because stopping a
# process and killing one part-way through are the sort of thing that behaves
# differently on one platform, and a named job says which host answered.
set -eu

cargo test --locked -p nodal-cli --test reclaim
cargo test --locked -p nodal-core --lib lifecycle::uniqueness
cargo test --locked -p nodal-core --lib lifecycle::hooks
cargo test --locked -p nodal-core --lib runtime::stop
cargo test --locked -p nodal-core --lib lifecycle::ops::gc
echo "acceptance (reclaim): dirty is refused, a clean unit leaves only a trash entry, hooks are approved by text, and gc removes it"
