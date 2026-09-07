#!/usr/bin/env sh
# Acceptance test for T0.8: the fixture project.
#
# Two claims, both checked here so CI never needs a real project to check them against.
#
#   1. `nodal init` on the fixture yields ZERO gaps. Every key a recipe needs is stated
#      somewhere in the project, so anything unanswered is a file the engine failed to
#      read. `--json` is used because it reports the gaps without writing: the fixture
#      already carries the `nodal.toml` that states the one thing no project file can.
#   2. The fixture BUILDS, from nothing, inside a budget. A fixture that only ever gets
#      read drifts into a shape no real project has, so this installs and builds it and
#      fails when that takes longer than NODAL_FIXTURE_BUDGET_SECONDS (default 60).
#
# Its own suite, linter and type-checker run after the timed section: they prove the
# commands the recipe names are commands that run, but they are not the build. Every
# one of them is dependency-free or already installed, so none of them adds to it.
set -eu

# Neither tool phones home from CI, and neither does so inside the timed section.
export DO_NOT_TRACK=1
export NEXT_TELEMETRY_DISABLED=1
export TURBO_TELEMETRY_DISABLED=1

budget=${NODAL_FIXTURE_BUDGET_SECONDS:-60}
root=$(mktemp -d)
report=$(mktemp)
trap 'rm -rf "$root" "$report"' EXIT

cargo run --locked -q -p nodal-fixture -- "$root"
cargo run --locked -q -p nodal-cli -- init --json "$root" > "$report"

node -e '
  const plan = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
  if (!plan.existed) throw new Error("the fixture should carry its own nodal.toml");
  if (plan.gaps.length !== 0) {
    throw new Error("gaps: " + plan.gaps.map((gap) => gap.key).join(", "));
  }
  console.log("acceptance (fixture): nodal init leaves zero gaps");
' "$report"

start=$(date +%s)
(
  cd "$root"
  pnpm install --no-frozen-lockfile
  pnpm run build
)
elapsed=$(( $(date +%s) - start ))
echo "acceptance (fixture): install and build took ${elapsed}s (budget ${budget}s)"
if [ "$elapsed" -gt "$budget" ]; then
  echo "acceptance (fixture): over budget" >&2
  exit 1
fi

cd "$root"
pnpm run test
pnpm run lint
pnpm run typecheck
echo "acceptance (fixture): the project the recipe describes is one that runs"
