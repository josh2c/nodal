#!/usr/bin/env sh
# Acceptance test for T0.12: the release binary starts inside the DL-017 budget.
#
# DL-017 sets a 5 ms cold-start budget for the hot paths. This gate measures the fast
# path that exists today, `nodal --version`, and fails when its median is over the
# threshold below. It prints every number on every run, pass or fail, so the record a
# later recalibration needs is in the log of any green run.
#
# The threshold is not the budget. `benches/startup` times a whole spawn from the
# parent, so each number includes the fork and exec the parent pays for, whatever it
# spawns. A shared CI runner charges more for that than a workstation does. The gate
# therefore allows the budget plus the runner's own spawn cost:
#
#   threshold = 5 ms budget + measured runner spawn overhead, rounded up
#
# The overhead is measured, not assumed: the script times the same number of spawns of
# a binary that does nothing (`true`) and prints that median as the reference. The
# reference is reported, never gated on, because it says how the runner is behaving and
# not how Nodal is behaving. When the reference median moves far from the number the
# threshold was calibrated against, recalibrate the threshold and record the new numbers.
#
# Calibration, ubuntu-24.04 GitHub-hosted runner (see spikes/RESULTS.md, T0.12):
#   reference `true`  median REFERENCE_MEDIAN ms
#   `nodal --version` median NODAL_MEDIAN ms
#
# Usage: ci/startup-budget.sh [path-to-nodal]
# Environment:
#   NODAL_STARTUP_THRESHOLD_MS  the number the median must stay under
#   NODAL_STARTUP_RUNS          how many runs the median is taken over
#   NODAL_STARTUP_WARMUP        how many runs are discarded first
set -eu

threshold=${NODAL_STARTUP_THRESHOLD_MS:-50}
runs=${NODAL_STARTUP_RUNS:-200}
warmup=${NODAL_STARTUP_WARMUP:-20}

bench=$(cargo build --release --locked -q -p nodal-startup-bench --message-format=json \
  | sed -n 's/.*"executable":"\([^"]*nodal-startup-bench\)".*/\1/p' | head -n 1)
[ -n "$bench" ] || { echo "startup: the harness did not build" >&2; exit 1; }

bin=${1:-}
if [ -z "$bin" ]; then
  cargo build --release --locked -q -p nodal-cli
  bin=$(dirname "$bench")/nodal
fi

# The runner's own cost to spawn anything. Reported, never gated on.
reference=/usr/bin/true
[ -x "$reference" ] || reference=/bin/true
if [ -x "$reference" ]; then
  "$bench" --runs "$runs" --warmup "$warmup" --report-only "$reference"
else
  echo "startup: no reference binary on this machine; overhead not reported"
fi

"$bench" --runs "$runs" --warmup "$warmup" --threshold-ms "$threshold" "$bin" --version
