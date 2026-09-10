#!/usr/bin/env sh
# Acceptance test for the recipe engine.
#
# The fixture project (tests/fixture) states every key a recipe needs, so inferring it
# must leave ZERO gaps. Anything unanswered is a file the engine failed to read. The
# suite also holds the merge rules, the round trip through the written nodal.toml, and
# `nodal init` end to end. Building the fixture is ci/acceptance-fixture.sh.
#
# NODAL_RECIPE_ROOT / NODAL_RECIPE_REFERENCE are deliberately not set here: the
# reference comparison is a developer's local check against a project CI does not have.
set -eu

cargo test --locked -p nodal-core --test recipe
cargo test --locked -p nodal-cli --test init
echo "acceptance (recipe): the fixture infers with zero gaps"
