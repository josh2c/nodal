#!/usr/bin/env bash
# How long a unit takes to go green, warm against cold.
#
# A unit made from a warm base starts with the build output the base holds, so the test
# command compiles what changed and nothing else. A cold clone compiles everything.
# The two readings are the same command in two trees, and the difference between them
# is what the base is for.

BENCH_NAME=ready
BENCH_ABOUT="time to green in a warm unit, against time to green in a cold clone"
# shellcheck source=benches/harness/common.sh
. "$(dirname "$0")/common.sh"

bench_main "$@"
bench_require binary git cargo
bench_start

project=$(bench_project)
registry=$(bench_workdir registry)

echo "== WARM: units made from a warm base, then the test command =="
# The homes this run made, in the order it made them, as Nodal named them.
set --
for i in $(seq 1 "${BENCH_RUNS}"); do
  set -- "$@" "$(bench_new "${registry}" "${project}" "w${i}" "warm-new-${i}")"
done
index=0
for home in "$@"; do
  index=$((index + 1))
  bench_green "${home}" "warm-${index}" warm_time_to_green_ms "run=${index}"
done

echo "== COLD: git clone, then the same command =="
for i in $(seq 1 "${BENCH_RUNS}"); do
  bench_cold_clone "${project}" "${i}"
done
