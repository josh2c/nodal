#!/usr/bin/env sh
# Acceptance test for the Claude Code integration.
#
# The suite feeds the payloads Claude Code was measured sending — a `WorktreeCreate`
# with an empty transcript path and a prompt-derived slug, and a `SessionStart` whose
# session identifier is not the one the create carried — and asserts the contract each
# event has:
#
#   - `WorktreeCreate` is a provider. Claude reads a directory from its standard output
#     and ends the session without one, so both answers are tested: the home of a unit
#     that was really made, and the refusal a project with no recipe gets. The refusal is
#     a path Claude rejects rather than an empty line, and the case where `nodal` is not
#     installed is run through the command text that is really in `.claude/settings.json`
#     with an empty `PATH`;
#   - `SessionStart` prints a memory inside a unit home and nothing anywhere else, and
#     answers the same for two different session identifiers over one path;
#   - `Stop` records a handoff where there is a message and is silent where the desktop
#     application sent none;
#   - `WorktreeRemove` removes nothing. It fired in none of four measured session
#     lifecycles, and a unit outliving its session is the product working.
#
# The last claims are about the file: it names no directory of this machine, because a
# person may commit it, and an install followed by an uninstall leaves it byte for byte
# the file it was, with the hooks somebody else put in it still in it.
#
# The suite is then run again with the temporary directory reached through a symbolic
# link, as `ci/acceptance-uninstall.sh` does and for the same reason: the provider hook
# prints a path that Claude compares with directories, and macOS gives that condition
# for free while Linux does not.
set -eu

cargo test --locked -p nodal-core --lib adapters::
cargo test --locked -p nodal-cli --test claude_code
cargo test --locked -p nodal-cli --test init
cargo test --locked -p nodal-cli --test uninstall

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-cli --test claude_code

echo "acceptance (claude code): the provider answers with a real home, refuses loudly, and the settings file comes back byte for byte"
