#!/usr/bin/env sh
# Acceptance test: the committed JSON schemas are what the model and the tool surface
# generate. Regenerates schemas/ and fails if anything in it changed or appeared.
set -eu

cargo run --quiet --locked -p nodal-core --example export-schemas -- schemas

# The tool surface's own document: what `tools/list` answers, written by the binary that
# answers it. A tool whose arguments changed shows up here as a diff.
mkdir -p schemas/mcp
cargo run --quiet --locked -p nodal-cli --bin nodal -- mcp --tools > schemas/mcp/tools.json

if [ -n "$(git status --porcelain -- schemas)" ]; then
	echo "schema diff: schemas/ is out of date; run ci/schema-diff.sh and commit the result" >&2
	git --no-pager diff -- schemas >&2
	git status --porcelain -- schemas >&2
	exit 1
fi
echo "schema diff: schemas/ matches the model and the tool listing matches the tools"
