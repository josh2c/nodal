#!/usr/bin/env sh
# Acceptance test for substrate bases.
#
# The suite builds real bases from a real remote. It asserts that two workspaces which
# key differently get two bases, that no base holds a file only the checkout has, that
# `gc` refuses a base a unit still holds, that a build from the nearest neighbour is
# faster than a cold one, and that a build killed between two steps is finished by the
# next invocation rather than started again.
#
# It runs under `cargo test --workspace` too. It has its own job because a kill between
# two steps and a measured comparison are the sort of thing that fails on one platform
# and not another, and a named job says which. `--nocapture` prints the two build times.
set -eu

cargo test --locked -p nodal-core --test substrate -- --nocapture --test-threads=1
cargo test --locked -p nodal-cli --test base
echo "acceptance (substrate): two bases coexist, a pinned base is refused, a neighbour build is faster"
