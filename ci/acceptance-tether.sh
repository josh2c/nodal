#!/usr/bin/env sh
# Acceptance test for P6: `nodal run --tether`, and the process group that dies with the
# unit.
#
# The suite starts real tethered commands against a real repository and checks what a
# person can check with `ps`. A tethered command that starts a child which starts another
# child is dead whole after the unit is reclaimed, not just its leader. A tethered command
# that stops when it is interrupted is never sent `SIGTERM`, and the file it writes from
# its own signal handler says so. A tether whose `nodal run` was killed with `SIGKILL` is
# still found, because the registry row is the record of the group and not the parent. An
# untethered process in the same home is stopped by attribution, as one process rather
# than as part of anybody's group. A tether left behind by a materialisation that has been
# reclaimed is stopped by the next sweep.
#
# A process group is addressed with `kill`, which every host answers, so nothing here
# stands aside on a host with no process table. Only the untethered half of the mixed test
# needs one, and it asserts the degraded behaviour where there is none.
#
# It runs under `cargo test --workspace` too. It has its own job because starting process
# groups, orphaning them and signalling them is the sort of thing that behaves differently
# on one platform, and a named job says which host answered.
set -eu

cargo test --locked -p nodal-cli --test tether
cargo test --locked -p nodal-core --lib runtime::stop
echo "acceptance (tether): a group dies whole, the ladder stops where it works, and an orphaned tether is still found"
