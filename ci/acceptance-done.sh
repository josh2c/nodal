#!/usr/bin/env sh
# Acceptance test for the states a unit passes through after the work is done.
#
# The suite pushes to a real repository — a bare one in the same temporary directory —
# and checks what a person can check for themselves. `nodal done` puts the branch on
# that remote and nothing else, keeps the work-in-progress snapshot here unless `--wip`
# asks for it, prints the compare page for the host the remote names, and leaves the
# unit in `review`. A squash merge in the project's own checkout, and a fetch by the
# person's own git, is what makes the next `nodal ls` say
# `merged`; the state is read back out of the registry, because the point of recording
# the flip is that it is a fact and not a rendering. `nodal gc` waits out the retention
# before it reclaims that unit's home, and refuses one somebody has since put work in.
# `nodal gc --idle` reports a quiet live unit and leaves its home and its state alone.
#
# Nothing here reaches a network. The one test that needs a *named* host uses Git's own
# `url.<local>.pushInsteadOf`, so `origin` really is `https://github.com/...` to
# everything that reads it and the push still lands next door.
#
# The job also asserts the promise the whole command is shaped around: not that no pull
# request was opened, but that no code path in either crate could open one. Both crates'
# sources are scanned for a host API name and for the command-line clients that speak
# one. That test is here rather than only in the workspace run because it is the one a
# later change is most likely to break without noticing.
set -eu

cargo test --locked -p nodal-cli --test done
cargo test --locked -p nodal-core --lib git::host
cargo test --locked -p nodal-core --lib git::push
cargo test --locked -p nodal-core --lib lifecycle::states
cargo test --locked -p nodal-core --lib lifecycle::idle
cargo test --locked -p nodal-core --lib lifecycle::ops::done
cargo test --locked -p nodal-core --lib output::view::done

echo "acceptance (done): the branch is pushed with one git push and the snapshot stays here, no pull request is opened, a squash-merged unit is recorded as merged, and gc waits out its retention"
