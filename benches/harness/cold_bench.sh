#!/usr/bin/env bash
# What a cold start costs, and what a reflinked copy of a warm base costs.
#
# The first half clones the checkout and runs the test command in it, which is the
# whole of the older way: no shared objects, no warm build, every crate compiled again.
#
# The second half copies a warm base with `cp -a --reflink` and runs the same command.
# That is the measurement that separates what Nodal does from what the filesystem does:
# a base a unit is cloned from shares its blocks, so the cost of the copy is not the
# cost of the bytes. Where the filesystem does not do reflinks the copy is a plain one,
# and the results file says which was made.

BENCH_NAME=cold
BENCH_ABOUT="the cost of a cold clone, against a reflinked copy of a warm base"
# shellcheck source=benches/harness/common.sh
. "$(dirname "$0")/common.sh"

bench_main "$@"
bench_require binary git cargo
bench_start

project=$(bench_project)

echo "== COLD: git clone, then the test command, ${BENCH_RUNS} runs =="
for i in $(seq 1 "${BENCH_RUNS}"); do
  bench_cold_clone "${project}" "${i}"
done

echo "== ISOLATION: a copy of the same warm base, then the test command =="
registry=$(bench_workdir registry)
( cd "${project}" && env NODAL_HOME="${registry}" "${BENCH_BINARY}" base build --warm ) \
  > "$(bench_log base-build)" 2>&1
base=$(bench_base_path "${registry}" "${project}")
if [ -z "${base}" ] || [ ! -d "${base}" ]; then
  echo "${BENCH_NAME}: no warm base was built, so the isolation half was not measured" >&2
  exit 1
fi
echo "warm base: ${base}"
for i in $(seq 1 3); do
  copy="${BENCH_RUN}/copy${i}"
  start=$(bench_ms)
  if cp -a --reflink=always "${base}" "${copy}" 2>/dev/null; then
    kind=reflink
  else
    cp -a "${base}" "${copy}"
    kind=plain
  fi
  bench_record copy_base_ms "$(( $(bench_ms) - start ))" "run=${i} copy=${kind}"
  bench_green "${copy}" "copy-${i}" copybase_time_to_green_ms "run=${i}"
  rm -rf -- "${copy}"
done
