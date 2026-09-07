#!/usr/bin/env sh
# Acceptance test for T1.8: env activation.
#
# Four claims, over the fixture project (tests/fixture):
#   - an activated home carries every generated variable, and a real shell gets them
#     by both routes: `nodal env --export` and the .envrc that direnv reads;
#   - a credential no source holds is a report line, and the home is still written;
#   - the per-machine secrets file is created 0600 and refused when it is looser;
#   - no secret value reaches a manifest, a report, a log line or an error message.
#
# The direnv route reports itself as skipped when direnv is not installed, so this
# script runs anywhere; the CI job installs direnv, which is what makes that check real.
set -eu

cargo test --locked -p nodal-core --test env
cargo test --locked -p nodal-cli --test env
echo "acceptance (env): an activated home carries its variables and leaks no value"
