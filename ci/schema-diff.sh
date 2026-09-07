#!/usr/bin/env sh
# Acceptance test for T0.2: the committed JSON schemas are what the model generates.
# Regenerates schemas/ and fails if anything in it changed or appeared.
set -eu

cargo run --quiet --locked -p nodal-core --example export-schemas -- schemas

if [ -n "$(git status --porcelain -- schemas)" ]; then
	echo "schema diff: schemas/ is out of date; run ci/schema-diff.sh and commit the result" >&2
	git --no-pager diff -- schemas >&2
	git status --porcelain -- schemas >&2
	exit 1
fi
echo "schema diff: schemas/ matches the model"
