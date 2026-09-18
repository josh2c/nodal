#!/usr/bin/env bash
# What a unit costs when the base it comes from is not stale.
#
# A base is stale when the project moved on after it was built: the unit gets the base's
# build output, and then rebuilds the crates that changed. This measures the other case.
# The run builds the base, makes one generation of units from it so the base is one that
# has been used, and then measures a second generation against the same, unchanged base.
# Those second-generation numbers are the best the design can do.
#
# The first version of this script read a registry an earlier run of `best_bench.sh`
# left behind, and named five unit identifiers to skip. It could not run twice, and the
# registry it needed was eighty-five gigabytes nothing removed. This run makes both
# generations itself, and keeps the home Nodal gave it for each one, so the second
# generation is the set it measures rather than the set it has to subtract.

BENCH_NAME=fresh
BENCH_ABOUT="create and time to green against a base that is warm and not stale"
# shellcheck source=benches/harness/common.sh
. "$(dirname "$0")/common.sh"

bench_main "$@"
bench_require binary git cargo
bench_start

project=$(bench_project)
registry=$(bench_workdir registry)
( cd "${project}" && env NODAL_HOME="${registry}" "${BENCH_BINARY}" base build --warm ) \
  > "$(bench_log base-build)" 2>&1

echo "== first generation, which makes the base one that has been used =="
for i in $(seq 1 "${BENCH_RUNS}"); do
  bench_new "${registry}" "${project}" "g${i}" "first-new-${i}" > /dev/null
done

echo "== second generation, measured =="
# Only the homes of this generation, which is why they are kept rather than listed back
# out of the registry and filtered.
set --
for i in $(seq 1 "${BENCH_RUNS}"); do
  start=$(bench_ms)
  home=$(bench_new "${registry}" "${project}" "f${i}" "fresh-new-${i}")
  bench_record fresh_create_ms "$(( $(bench_ms) - start ))" "run=${i}"
  set -- "$@" "${home}"
done

echo "== time to green from a base that was not stale =="
index=0
for home in "$@"; do
  index=$((index + 1))
  bench_green "${home}" "fresh-${index}" fresh_time_to_green_ms "run=${index}"
done

echo "== disk =="
base=$(bench_base_path "${registry}" "${project}")
if [ -n "${base}" ]; then
  bench_record fresh_base_bytes "$(bench_bytes "${base}")" "path=${base}"
fi
