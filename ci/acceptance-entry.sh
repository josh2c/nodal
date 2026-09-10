#!/usr/bin/env sh
# Acceptance test for activation and entry.
#
# Four claims, against real shells:
#   - a process started in an activated directory carries NODAL_ID, by either route:
#     the .envrc direnv reads, or the prompt hook `nodal shell-init` installs;
#   - no subshell anywhere: `nodal shell` becomes the shell, and the integration only
#     changes the directory of the shell that is already running;
#   - `nodal cd` moves the shell it was run from, in bash and in zsh;
#   - `nodal run` runs in the unit's environment and records one command event, with
#     the credential replaced by the name that holds it.
#
# A shell this machine has not got reports itself as skipped, so the script runs
# anywhere; the CI job installs bash, zsh, fish and direnv, which is what makes each
# check real.
set -eu

cargo test --locked -p nodal-core --test runtime
cargo test --locked -p nodal-cli --test entry
echo "acceptance (entry): a process in an activated home carries NODAL_ID, and no shell is started"
