#!/usr/bin/env sh
# Acceptance test for T1.11b: the integration verdict, against a second implementation.
#
# Worktrunk (`wt`) answers the same question on the same repository, so its answer is a
# check on ours that no test of ours can be. This script builds the shape fixture, asks
# `wt list` for its verdict on every branch, maps it to the word Nodal prints, and fails
# on a disagreement. The expected verdicts come from the fixture itself
# (`nodal_fixture::shapes::BRANCHES`), so a shape and its expectation cannot drift.
#
# The two tools use different words for one verdict, and this is the map:
#
#   wt integrated · ancestor            -> nodal done (ancestor)
#   wt integrated · merge-adds-nothing  -> nodal done (absorbed)
#   wt integrated · no-added-changes    -> nodal done (absorbed)
#   wt would_conflict                   -> nodal conflict
#   wt ahead                            -> nodal open
#   wt diverged                         -> nodal open
#
# One verdict is deliberately different and is not in the map. A branch whose tip is the
# base's tip is `empty` to worktrunk and `done (ancestor)` to Nodal. Worktrunk lists
# branches, where "this branch has no commits of its own" is worth saying. Nodal lists
# units, where a unit with nothing on its branch has nothing left to integrate, which is
# the same answer as a merged one. `docs/contracts.md` states the four verdicts Nodal has.
#
# `wt` is a reference tool and not a dependency: a machine without it skips the
# comparison and says so.
#
# Usage: ci/compare-worktrunk.sh
set -eu

if ! command -v wt >/dev/null 2>&1; then
  echo "compare (worktrunk): wt is not on this machine; the comparison did not run"
  exit 0
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "compare (worktrunk): python3 is not on this machine; the comparison did not run"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo run --locked -q -p nodal-fixture -- --shapes "$work/shapes" > "$work/expected"
repository=$(head -n 1 "$work/expected")
tail -n +2 "$work/expected" > "$work/branches"

wt list --branches --format json -C "$repository" > "$work/wt.json" 2>/dev/null

python3 - "$work/branches" "$work/wt.json" <<'PY'
import json, sys

MAP = {
    ("integrated", "ancestor"): "done (ancestor)",
    ("integrated", "merge-adds-nothing"): "done (absorbed)",
    ("integrated", "no-added-changes"): "done (absorbed)",
    ("would_conflict", None): "conflict",
    ("ahead", None): "open",
    ("diverged", None): "open",
}

expected = {}
with open(sys.argv[1]) as handle:
    for line in handle:
        name, verdict = line.rstrip("\n").split("\t")
        expected[name] = verdict

with open(sys.argv[2]) as handle:
    reference = {row["branch"]: row for row in json.load(handle)}

failures = []
for name, ours in sorted(expected.items()):
    row = reference.get(name)
    if row is None:
        failures.append(f"{name}: worktrunk did not list it")
        continue
    key = (row.get("main_state"), row.get("integration_reason"))
    theirs = MAP.get(key)
    if theirs is None:
        failures.append(f"{name}: worktrunk said {key}, which the map does not cover")
    elif theirs != ours:
        failures.append(f"{name}: nodal says {ours!r}, worktrunk says {theirs!r}")
    else:
        print(f"compare (worktrunk): {name:<14} {ours}")

if failures:
    for failure in failures:
        print(f"compare (worktrunk): {failure}", file=sys.stderr)
    raise SystemExit(1)
print(f"compare (worktrunk): {len(expected)} shapes, every verdict agrees")
PY
