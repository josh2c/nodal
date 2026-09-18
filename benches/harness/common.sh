# shellcheck shell=bash
# Shared rules for every benchmark harness run. Source this; do not run it.
#
# The harness measures how long Nodal takes to make a working unit, and how long the
# older ways take. Each measurement builds a Rust workspace, so each run writes
# gigabytes. This file holds the parts that keep those gigabytes from collecting.
#
# # Three rules
#
# **A run owns one work directory and removes it.** Every clone, registry and build
# tree a run makes goes under `$BENCH_RUN`. A trap removes that directory on every
# exit: success, failure, and the two signals a person sends. `--keep` holds it.
#
# **A run that is killed names itself.** `SIGKILL` runs no trap, so the work directory
# carries the script name and the process identifier that made it. The next run reads
# those names, and removes each directory whose process is gone.
#
# **Results are not work directories.** The numbers and the build logs go under
# `$BENCH_RESULTS`, which no run removes. `--gc` removes the results that are older
# than the project's trash retention, and every run prints that rule before it starts.
#
# # What a script must set before it sources this file
#
# `BENCH_NAME` is the one-word name of the measurement. `BENCH_ABOUT` is one line that
# says what it measures. Both go in the results file and in `--help`.

# A failed clone must not be followed by numbers recorded against a directory that is
# not there. `-e` stops the run, `pipefail` stops a failure hiding behind a pipe, and the
# trap is on `ERR` as well as the two signals, so the work directory goes either way.
# A command whose failure is a measurement, not a fault, captures it with `|| status=$?`.
set -eu
set -o pipefail

# ---------------------------------------------------------------------------
# Where things are. Every path comes from an argument or the environment.
# ---------------------------------------------------------------------------

# The checkout under measurement. Default: the repository the script is in.
BENCH_SOURCE="${NODAL_BENCH_SOURCE:-}"
# The `nodal` binary under measurement.
BENCH_BINARY="${NODAL_BENCH_BINARY:-}"
# Where work directories go.
BENCH_ROOT="${NODAL_BENCH_ROOT:-${XDG_CACHE_HOME:-${HOME}/.cache}/nodal-bench}"
# Where results go.
BENCH_RESULTS_ROOT="${NODAL_BENCH_RESULTS:-}"
# Whether the work directory survives the run.
BENCH_KEEP=0
# How many times each measurement is repeated.
BENCH_RUNS="${NODAL_BENCH_RUNS:-5}"

# The retention, in days, that a project which states none gets.
#
# It is the same number as Nodal's own default trash retention, because the harness
# holds results for as long as Nodal holds a reclaimed home, and one rule a person
# learns once is better than two. It is a second copy of that number and the help says
# so: the product's copy is `DEFAULT_TRASH_RETENTION_DAYS` in
# `crates/nodal-core/src/model/recipe.rs`, and nothing here can read it. A project that
# wants another number states it in `nodal.toml`, and then one value answers for both.
BENCH_DEFAULT_RETENTION_DAYS=14

# ---------------------------------------------------------------------------
# Arguments.
# ---------------------------------------------------------------------------

bench_help() {
  cat <<EOF
${BENCH_NAME} — ${BENCH_ABOUT}

Usage: ${BENCH_NAME}.sh [options]

Options:
  --source <path>     The checkout to measure. Default: the repository this script
                      is in.
  --binary <path>     The nodal binary to measure. Default: <source>/target/release/nodal.
  --root <path>       Where work directories go. Default: \$XDG_CACHE_HOME/nodal-bench.
  --results <path>    Where results go. Default: <source>/benches/results.
  --runs <n>          How many times to repeat each measurement. Default: ${BENCH_RUNS}.
  --keep              Do not remove the work directory. Print where it is.
  --gc                Remove results older than the retention, and do no measurement.
                      It removes only a directory a run of this harness wrote.
  --help              Print this and do nothing.

Environment: NODAL_BENCH_SOURCE, NODAL_BENCH_BINARY, NODAL_BENCH_ROOT,
NODAL_BENCH_RESULTS, NODAL_BENCH_RUNS, NODAL_BENCH_RETENTION_DAYS.

The run removes its work directory on every exit. Results stay for the retention
\`nodal.toml\` states under \`[reclaim] trash_retention\`. This project states none, so
runs use ${BENCH_DEFAULT_RETENTION_DAYS} days, the same number as Nodal's own default.
\`--gc\` removes the results that are older.
EOF
}

# Read the options every harness script takes. A script with options of its own reads
# them after this and passes the rest back.
bench_parse() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --source) BENCH_SOURCE="$2"; shift 2 ;;
      --binary) BENCH_BINARY="$2"; shift 2 ;;
      --root) BENCH_ROOT="$2"; shift 2 ;;
      --results) BENCH_RESULTS_ROOT="$2"; shift 2 ;;
      --runs) BENCH_RUNS="$2"; shift 2 ;;
      --keep) BENCH_KEEP=1; shift ;;
      --gc) BENCH_GC=1; shift ;;
      --help|-h) bench_help; exit 0 ;;
      *) echo "${BENCH_NAME}: unknown option $1" >&2; bench_help >&2; exit 2 ;;
    esac
  done
}

# ---------------------------------------------------------------------------
# The retention rule, which the run prints and `--gc` acts on.
# ---------------------------------------------------------------------------

# How many days results are held. The project's recipe states it; the default stands
# when the recipe leaves the line commented out, which is what `nodal init` writes.
bench_retention_days() {
  if [ -n "${NODAL_BENCH_RETENTION_DAYS:-}" ]; then
    echo "${NODAL_BENCH_RETENTION_DAYS}"
    return
  fi
  stated=$(sed -n 's/^[[:space:]]*trash_retention[[:space:]]*=[[:space:]]*\([0-9]\{1,\}\).*/\1/p' \
    "${BENCH_SOURCE}/nodal.toml" 2>/dev/null | head -1)
  echo "${stated:-${BENCH_DEFAULT_RETENTION_DAYS}}"
}

# Remove the result directories that are older than the retention.
# The command this project calls a test, from its recipe.
#
# One reading and one fallback. Four scripts measure time to green and each one used to
# read the recipe itself, so the fallback was written four times and a fifth script that
# forgot it would measure something else.
bench_test_command() {
  stated=$(sed -n 's/^test[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' "$1/nodal.toml" 2>/dev/null | head -1)
  echo "${stated:-cargo test --workspace}"
}

# Whether a directory is one a run of this harness wrote.
#
# Two proofs, and both are needed. The name has to be the one `bench_start` makes —
# `<script>.<stamp>.<pid>` — and the directory has to hold the results file every run
# writes before its first measurement.
#
# `--gc` removes directories, and `--results` is a path a person types. `--results ~`
# must not be a way to delete a home directory, so the sweep removes what it can prove
# a run of this harness made, and leaves everything else where it is.
bench_is_result_dir() {
  [ -f "$1/results.tsv" ] || return 1
  case "${1##*/}" in
    # <script>.<8 digits>T<6 digits>Z.<pid>
    *.????????T??????Z.*) ;;
    *) return 1 ;;
  esac
  name="${1##*/}"
  owner="${name##*.}"
  case "${owner}" in (*[!0-9]*|'') return 1 ;; esac
  return 0
}

# Remove the result directories that are older than the retention, and no others.
bench_gc() {
  days=$(bench_retention_days)
  if [ ! -d "${BENCH_RESULTS_ROOT}" ]; then
    echo "no results under ${BENCH_RESULTS_ROOT}"
    return 0
  fi
  removed=0
  kept=0
  while IFS= read -r old; do
    [ -n "${old}" ] || continue
    if ! bench_is_result_dir "${old}"; then
      echo "left ${old}; no run of this harness wrote it"
      kept=$((kept + 1))
      continue
    fi
    rm -rf -- "${old}"
    echo "removed ${old}"
    removed=$((removed + 1))
  done <<EOF
$(find "${BENCH_RESULTS_ROOT}" -mindepth 1 -maxdepth 1 -type d -mtime "+${days}" 2>/dev/null)
EOF
  echo "gc: ${removed} result directories older than ${days} days removed, ${kept} left alone"
}

# ---------------------------------------------------------------------------
# The work directory, and the trap that removes it.
# ---------------------------------------------------------------------------

# Remove the work directories of runs whose process is gone.
#
# A run killed with SIGKILL runs no trap. Its directory carries the name of the script
# and the process identifier, so this reads both and removes what no live run owns. A
# run of another script, and a second run of this one, are both left alone.
bench_sweep_stale() {
  [ -d "${BENCH_ROOT}" ] || return 0
  for stale in "${BENCH_ROOT}"/run.*; do
    [ -d "${stale}" ] || continue
    if [ "${stale}" = "${BENCH_RUN:-}" ]; then
      continue
    fi
    owner="${stale##*.}"
    case "${owner}" in (*[!0-9]*|'') continue ;; esac
    # Three answers, not two. `kill -0` succeeds while the process is alive, fails with
    # EPERM while it is alive and another account's, and fails with ESRCH once it is
    # gone. Only the third is a directory to remove: removing another person's work
    # directory on a shared host would be the fault this sweep exists to fix, made worse.
    if kill -0 "${owner}" 2>/dev/null; then
      continue
    fi
    if [ -O "${stale}" ]; then
      : # Ours, and the process is gone.
    else
      echo "left ${stale##*/}; it belongs to another account"
      continue
    fi
    rm -rf -- "${stale}"
    echo "removed the work directory of run ${stale##*/}, whose process is gone"
  done
}

# Remove this run's work directory, unless `--keep` said to hold it.
bench_cleanup() {
  status=$?
  trap - EXIT INT TERM ERR
  if [ "${BENCH_KEEP}" -eq 1 ]; then
    echo "kept ${BENCH_RUN}"
  elif [ -n "${BENCH_RUN:-}" ] && [ -d "${BENCH_RUN}" ]; then
    rm -rf -- "${BENCH_RUN}"
  fi
  echo "results: ${BENCH_RESULT_DIR}"
  exit "${status}"
}

# ---------------------------------------------------------------------------
# Starting a run.
# ---------------------------------------------------------------------------

# Fill in every path the script did not state, and refuse a run that cannot measure.
bench_resolve() {
  if [ -z "${BENCH_SOURCE}" ]; then
    BENCH_SOURCE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && git rev-parse --show-toplevel)
  fi
  BENCH_SOURCE=$(cd "${BENCH_SOURCE}" && pwd)
  # Written as `if` and not as `cond && assign`: under `set -e` the second form ends
  # the run whenever the condition is false, because a test that answers "no" is a
  # command that failed and this is the last one the function runs.
  if [ -z "${BENCH_BINARY}" ]; then
    BENCH_BINARY="${BENCH_SOURCE}/target/release/nodal"
  fi
  if [ -z "${BENCH_RESULTS_ROOT}" ]; then
    BENCH_RESULTS_ROOT="${BENCH_SOURCE}/benches/results"
  fi
}

# Refuse a measurement the machine cannot make, before anything is written.
bench_require() {
  for tool in "$@"; do
    case "${tool}" in
      binary)
        [ -x "${BENCH_BINARY}" ] || {
          echo "${BENCH_NAME}: ${BENCH_BINARY} is not there. Build it, or name one with --binary." >&2
          exit 1
        } ;;
      *)
        command -v "${tool}" >/dev/null 2>&1 || {
          echo "${BENCH_NAME}: ${tool} is not on the path." >&2
          exit 1
        } ;;
    esac
  done
}

# Make the work directory, set the trap, and print the rule.
bench_start() {
  stamp=$(date -u +%Y%m%dT%H%M%SZ)
  mkdir -p "${BENCH_ROOT}" "${BENCH_RESULTS_ROOT}"
  # The run's identity: the script, the time it started, and the process that made it.
  # The work directory and the results directory carry the same one, so a person reading
  # a directory a killed run left can find the results of that same run. The process
  # identifier is not decoration: two runs of one script in the same second would
  # otherwise write to one results directory and the second would overwrite the first.
  run_id="${BENCH_NAME}.${stamp}.$$"
  BENCH_RUN="${BENCH_ROOT}/run.${run_id}"
  BENCH_RESULT_DIR="${BENCH_RESULTS_ROOT}/${run_id}"
  bench_sweep_stale
  mkdir -p "${BENCH_RUN}" "${BENCH_RESULT_DIR}"
  BENCH_RESULT_FILE="${BENCH_RESULT_DIR}/results.tsv"
  BENCH_TEST_COMMAND=$(bench_test_command "${BENCH_SOURCE}")
  trap bench_cleanup EXIT INT TERM ERR
  commit=$(git -C "${BENCH_SOURCE}" rev-parse --short HEAD 2>/dev/null || echo unknown)
  {
    echo "# ${BENCH_NAME}	${BENCH_ABOUT}"
    echo "# started	${stamp}"
    echo "# run	${run_id}"
    echo "# source	${BENCH_SOURCE}	${commit}"
    echo "# binary	${BENCH_BINARY}"
    echo "# test	${BENCH_TEST_COMMAND}"
    echo "# host	$(uname -s)	$(uname -m)"
    printf 'metric\tvalue\tnote\n'
  } > "${BENCH_RESULT_FILE}"
  cat <<EOF
${BENCH_NAME}: ${BENCH_ABOUT}
  source     ${BENCH_SOURCE} (${commit})
  binary     ${BENCH_BINARY}
  test       ${BENCH_TEST_COMMAND}
  work       ${BENCH_RUN}
  results    ${BENCH_RESULT_DIR}
  retention  results are held $(bench_retention_days) days; \`${BENCH_NAME}.sh --gc\` removes the older ones
  cleanup    this run removes ${BENCH_RUN} when it exits, and when it fails
EOF
}

# ---------------------------------------------------------------------------
# Measuring.
# ---------------------------------------------------------------------------

# The clock, in milliseconds. `EPOCHREALTIME` is the shell's own and needs no process;
# `date +%s%3N` is the answer where the shell does not publish one.
bench_ms() {
  if [ -n "${EPOCHREALTIME:-}" ]; then
    echo $(( ${EPOCHREALTIME%%[.,]*} * 1000 + 10#${EPOCHREALTIME#*[.,]} / 1000 ))
  else
    date +%s%3N
  fi
}

# Write one measurement to the results file and to the terminal.
bench_record() {
  metric="$1"; value="$2"; note="${3:-}"
  printf '%s\t%s\t%s\n' "${metric}" "${value}" "${note}" >> "${BENCH_RESULT_FILE}"
  printf '%s %s %s\n' "${metric}" "${value}" "${note}"
}

# A work directory inside this run's, named for what it holds.
bench_workdir() {
  mktemp -d "${BENCH_RUN}/$1.XXXXXX"
}

# What a directory holds, in bytes. `du -sb` is the reading every host makes; `btrfs
# filesystem du` reports what shared extents really cost and is used where it answers.
bench_bytes() {
  measured=$(du -sb "$1" 2>/dev/null | cut -f1)
  echo "${measured:-0}"
}

# A log file inside this run's results, which the run does not remove.
bench_log() {
  echo "${BENCH_RESULT_DIR}/$1.log"
}

# A throwaway clone of the source checkout, inside this run's work directory.
#
# Every measurement that makes units works on this clone and never on the checkout the
# person is reading. A harness that edited the recipe of a live checkout, as the first
# version of `best_bench.sh` did, leaves a commit behind that no measurement wanted.
bench_project() {
  project="${BENCH_RUN}/project"
  if [ ! -d "${project}" ]; then
    git clone --quiet "${BENCH_SOURCE}" "${project}"
    git -C "${project}" config user.email "harness@nodal.invalid"
    git -C "${project}" config user.name "nodal benchmark harness"
  fi
  echo "${project}"
}

# The path of the one base a registry holds, read from Nodal rather than guessed.
bench_base_path() {
  registry="$1"; project="$2"
  ( cd "${project}" && env NODAL_HOME="${registry}" "${BENCH_BINARY}" base ls --json ) \
    | sed -n 's/.*"path": "\([^"]*\)".*/\1/p' | head -1
}

# Make a unit and answer with the home Nodal made for it.
#
# The home is read from `nodal new --json` and not from a glob of the registry. Three
# scripts used to walk `<registry>/project/e/*/` to find the homes they had just made,
# which is Nodal's own storage layout copied into five places that do not own it, and
# which cannot tell a home this run made from one it did not. A script that keeps what
# `bench_new` answered knows both.
bench_new() {
  registry="$1"; project="$2"; slug="$3"; log_name="$4"
  log=$(bench_log "${log_name}")
  ( cd "${project}" && env NODAL_HOME="${registry}" "${BENCH_BINARY}" \
      new "${slug}" --name "${slug}" --json ) > "${log}" 2>&1
  home=$(sed -n 's/.*"home": "\([^"]*\)".*/\1/p' "${log}" | head -1)
  if [ -z "${home}" ] || [ ! -d "${home}" ]; then
    echo "${BENCH_NAME}: nodal new ${slug} made no home; see ${log}" >&2
    return 1
  fi
  echo "${home}"
}

# Run the test command in `home` and record how long it took to go green.
#
# The exit status is a measurement and not a fault: a run that fails to build is a
# number this harness reports, so it is captured rather than left to stop the run.
bench_green() {
  home="$1"; log_name="$2"; metric="$3"; note="${4:-}"
  log=$(bench_log "${log_name}")
  start=$(bench_ms)
  status=0
  ( cd "${home}" && eval "${BENCH_TEST_COMMAND}" ) > "${log}" 2>&1 || status=$?
  # `grep -c` answers 1 when it counted none, which under `set -e` would end the run.
  compiled=$(grep -c Compiling "${log}" || true)
  bench_record "${metric}" "$(( $(bench_ms) - start ))" \
    "${note:+${note} }rc=${status} compiled=${compiled}"
}

# Clone the project, run the test command in the clone, and remove it.
#
# This is the older way, whole: no shared objects, no warm build, every crate compiled
# again. Two scripts measure it and both want the same four things said about it, so the
# loop is here rather than in each of them.
bench_cold_clone() {
  project="$1"; index="$2"
  clone="${BENCH_RUN}/cold${index}"
  git clone --quiet "${project}" "${clone}"
  bench_green "${clone}" "cold-${index}" cold_time_to_green_ms "run=${index}"
  # After the build and not before it: an empty clone holds no build output, and the
  # first version of this line measured exactly that.
  bench_record cold_target_bytes "$(bench_bytes "${clone}/target")" "run=${index}"
  rm -rf -- "${clone}"
}

# Everything a harness script does before its first measurement.
bench_main() {
  bench_parse "$@"
  bench_resolve
  if [ "${BENCH_GC:-0}" -eq 1 ]; then
    bench_gc
    exit 0
  fi
}
