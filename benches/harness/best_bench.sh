#!/usr/bin/env bash
# What the base is worth when the recipe asks it to run the test command.
#
# A base whose build step is `cargo build` hands a unit a warm `target` for the build
# profile only. A base whose build step is the test command hands it a warm `target`
# for the test profile as well, which is the one a person waits on. This measures the
# second: the base build, the creates it feeds, and the time to green in each unit.
#
# The recipe edit is made on the throwaway clone this run made, and is never committed
# to the checkout the person is reading.

BENCH_NAME=best
BENCH_ABOUT="the cost and the payback of a base whose build step runs the test command"
# shellcheck source=benches/harness/common.sh
. "$(dirname "$0")/common.sh"

bench_main "$@"
bench_require binary git cargo
bench_start

project=$(bench_project)

sed -i.bak "s#^build = .*#build = \"${BENCH_TEST_COMMAND}\"#" "${project}/nodal.toml"
rm -f "${project}/nodal.toml.bak"
# Only when the edit changed something. A project whose build step is already the test
# command needs no commit, and `git commit` with nothing staged exits non-zero, which
# under `set -e` would end a run that has nothing wrong with it.
if ! git -C "${project}" diff --quiet -- nodal.toml; then
  git -C "${project}" commit -qam "recipe: the base build runs the test command"
fi
echo "recipe build step: $(grep '^build' "${project}/nodal.toml")"

registry=$(bench_workdir registry)
start=$(bench_ms)
( cd "${project}" && env NODAL_HOME="${registry}" "${BENCH_BINARY}" base build --warm ) \
  > "$(bench_log base-build)" 2>&1
bench_record best_base_build_ms "$(( $(bench_ms) - start ))" "build=${BENCH_TEST_COMMAND}"

# The homes this run made, in the order it made them, as Nodal named them.
set --
for i in $(seq 1 "${BENCH_RUNS}"); do
  start=$(bench_ms)
  home=$(bench_new "${registry}" "${project}" "b${i}" "best-new-${i}")
  bench_record best_create_ms "$(( $(bench_ms) - start ))" "run=${i}"
  set -- "$@" "${home}"
done

echo "== time to green in each unit =="
index=0
for home in "$@"; do
  index=$((index + 1))
  bench_green "${home}" "best-${index}" best_time_to_green_ms "run=${index}"
done

echo "== disk =="
base=$(bench_base_path "${registry}" "${project}")
if [ -n "${base}" ]; then
  bench_record best_base_bytes "$(bench_bytes "${base}")" "path=${base}"
fi
index=0
for home in "$@"; do
  index=$((index + 1))
  bench_record best_home_bytes "$(bench_bytes "${home}")" "run=${index}"
done
