#!/usr/bin/env bash
# How long it takes to get a unit, three ways.
#
# A: `nodal new` against a cold registry, which builds the base first. B: `nodal new`
# against a registry whose base is already warm. C: `git clone`, which is what the
# thirty-four clones on the founder's workstation each cost.
#
# A and B are the two halves of the claim the README makes. C is the number they are
# claimed against.

BENCH_NAME=create
BENCH_ABOUT="how long a unit takes to make: cold registry, warm base, and git clone"
# shellcheck source=benches/harness/common.sh
. "$(dirname "$0")/common.sh"

bench_main "$@"
bench_require binary git
bench_start

project=$(bench_project)

echo "== A: nodal new against a cold registry, the base build included =="
for i in $(seq 1 "${BENCH_RUNS}"); do
  registry=$(bench_workdir "cold${i}")
  start=$(bench_ms)
  bench_new "${registry}" "${project}" "c${i}" "cold-create-${i}" > /dev/null
  bench_record cold_create_ms "$(( $(bench_ms) - start ))" "run=${i}"
  rm -rf -- "${registry}"
done

echo "== B: nodal new against one registry whose base is warm =="
registry=$(bench_workdir warm)
bench_new "${registry}" "${project}" seed warm-seed > /dev/null
for i in $(seq 1 "${BENCH_RUNS}"); do
  start=$(bench_ms)
  bench_new "${registry}" "${project}" "w${i}" "warm-create-${i}" > /dev/null
  bench_record warm_create_ms "$(( $(bench_ms) - start ))" "run=${i}"
done

echo "== C: git clone, which is what the older way cost =="
for i in $(seq 1 "${BENCH_RUNS}"); do
  clone="${BENCH_RUN}/clone${i}"
  start=$(bench_ms)
  git clone --quiet "${project}" "${clone}"
  bench_record git_clone_local_ms "$(( $(bench_ms) - start ))" "bytes=$(bench_bytes "${clone}")"
  rm -rf -- "${clone}"
done
