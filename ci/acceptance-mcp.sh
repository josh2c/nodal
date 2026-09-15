#!/usr/bin/env sh
# Acceptance test for `nodal mcp`: the tool surface every agent reaches.
#
# The suite speaks JSON-RPC to the binary over a pipe, which is what an agent does, and
# asserts the four things the surface promises. The handshake names the server and its
# tools. A tool result is the bytes the matching `nodal <verb> --json` writes, compared
# text against text, so a second representation of a unit cannot appear without failing
# here. The verbs that remove or rewrite something a person has — reclaim, merge, gc,
# uninstall, base — are absent from the listing and refused by name with the reason and
# the command a person runs instead. A refusal is a JSON-RPC error carrying the sentence
# the command line prints for the same refusal.
#
# The registration is here too: what `nodal init --claude-hooks` writes into a project's
# `.mcp.json` comes out again byte for byte, and a file holding somebody else's server
# keeps it.
set -eu

cargo test --locked -p nodal-cli --test mcp
cargo test --locked -p nodal-core --lib adapters::mcp
cargo test --locked -p nodal-core --lib adapters::settings
cargo test --locked -p nodal-cli --test handoff

echo "acceptance (mcp): the tool surface answers the protocol, every tool answers with the command line's own json, the withheld verbs are refused by name, and the declaration is removable byte for byte"
