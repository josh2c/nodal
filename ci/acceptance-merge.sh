#!/usr/bin/env sh
# Acceptance test for `nodal merge`: one command from a dirty unit to a merged target.
#
# The suite merges units in real repositories and checks what a person can check for
# themselves: the target branch in the project's own checkout carries the work, the home
# is in the trash, and the commits the squash folded answer `git log` on the premerge
# ref inside that trashed home.
#
# It also holds the merge open inside the step that moves the target branch, kills it
# with SIGKILL, and runs the next `nodal`, which has to roll the run back and leave the
# work in the home exactly as it was. The door is held open by a `reference-transaction`
# hook the test installs in its own fixture; Nodal installs a Git hook nowhere.
#
# Each `--no-` flag is checked to drop exactly its stage, a conflict is checked to stop
# with both ways out working, and a target that moved is checked to be refused rather
# than forced.
set -eu

cargo test --locked -p nodal-cli --test merge
cargo test --locked -p nodal-core --lib lifecycle::template
cargo test --locked -p nodal-core --lib lifecycle::hooks
cargo test --locked -p nodal-core --lib services::ports
echo "acceptance (merge): a dirty unit reaches the target, the flags drop their stages, a conflict stops both ways, and a kill rolls back"
