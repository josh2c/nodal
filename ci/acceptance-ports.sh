#!/usr/bin/env sh
# Acceptance test for T1.7: the port allocator.
#
# Two units of one project take different ports from the project's block; sixteen
# callers in sixteen threads, each with its own registry connection, come away with
# sixteen distinct ports and no double grant; a port the project pins is held by one
# unit at a time and a second unit is told which unit holds it; reclaim gives every
# port and lease back; and the listener scan finds a socket the test itself binds.
#
# It runs under `cargo test --workspace` too. It has its own job because the listener
# scan reads /proc, and a named job says which host answered.
set -eu

cargo test --locked -p nodal-core --test ports
echo "acceptance (ports): distinct ports under concurrency, fixed-port holder named, listener found"
