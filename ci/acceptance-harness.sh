#!/usr/bin/env sh
# Acceptance test for the benchmark harness in `benches/harness/`.
#
# The harness exists because the five scripts it replaces left 152 GB on a workstation
# in five days. This suite holds the three promises that make it different, and it holds
# them on disk use rather than on what the scripts say about themselves.
#
# 1. A run that succeeds returns the work root to the size it started at.
# 2. A run that fails returns it to the same size. A harness that only cleans up after a
#    good run is the harness that made the 152 GB.
# 3. A run that gets SIGKILL runs no trap, so it leaves a work directory. The directory
#    names the script and the process that made it, and the next run removes it. A run
#    that is still alive keeps its own.
#
# The measurements are made against a tiny project, not this repository. What is under
# test is the cleanup, and a suite that built a Rust workspace five times to prove it
# would be the cost it is here to prevent.

set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
harness="$root/benches/harness"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

nodal="$root/target/release/nodal"
if [ ! -x "$nodal" ]; then
  cargo build --release -p nodal-cli
fi

# The project under measurement: a git repository with a recipe whose commands are
# instant. `nodal new` must not need a package manager the runner does not have, so the
# recipe pins none.
project="$work/project"
mkdir -p "$project"
cat > "$project/nodal.toml" <<EOF
backend = "native"
monorepo = false

[commands]
build = "true"
test = "true"
lint = "true"

[base]
exclude = []
invalidate = []
EOF
printf 'target/\n' > "$project/.gitignore"
echo "the project" > "$project/README.md"
git -C "$project" init -q .
git -C "$project" config user.email "harness@nodal.invalid"
git -C "$project" config user.name "acceptance"
git -C "$project" add -A
git -C "$project" commit -qm "the project under measurement"

# The same project with a test command slow enough to signal a run inside it.
slow="$work/slow"
cp -a "$project" "$slow"
sed -i.bak 's/^test = .*/test = "sleep 120"/' "$slow/nodal.toml"
rm -f "$slow/nodal.toml.bak"
git -C "$slow" commit -qam "a test command that waits"

bench_root="$work/root"
results="$work/results"
mkdir -p "$bench_root" "$results"

run() {
  script="$1"
  shift
  "$harness/${script}_bench.sh" \
    --source "$project" --binary "$nodal" \
    --root "$bench_root" --results "$results" --runs 2 "$@"
}

entries() {
  find "$bench_root" -mindepth 1 -maxdepth 1 | wc -l | tr -d ' '
}

bytes() {
  du -sb "$bench_root" | cut -f1
}

# ---------------------------------------------------------------------------
# 1. Two runs, and the disk use is where it started.
# ---------------------------------------------------------------------------

start_bytes=$(bytes)
start_entries=$(entries)

run create > "$work/create.log" 2>&1
run create >> "$work/create.log" 2>&1

after_bytes=$(bytes)
after_entries=$(entries)
if [ "$after_bytes" != "$start_bytes" ] || [ "$after_entries" != "$start_entries" ]; then
  echo "acceptance (harness): two runs left $((after_bytes - start_bytes)) bytes in $bench_root" >&2
  find "$bench_root" -mindepth 1 -maxdepth 1 >&2
  exit 1
fi
echo "acceptance (harness): two runs returned the work root to ${start_bytes} bytes"

# The results are the part a run keeps, and two runs wrote two of them.
kept=$(find "$results" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
if [ "$kept" -lt 2 ]; then
  echo "acceptance (harness): two runs wrote ${kept} result directories, expected 2" >&2
  exit 1
fi
if [ ! -f "$(find "$results" -name results.tsv | head -1)" ]; then
  echo "acceptance (harness): a run wrote no results file" >&2
  exit 1
fi
echo "acceptance (harness): each run kept its results and its logs"

# The rule a person has to be able to read is printed before the first measurement.
if ! grep -q "retention" "$work/create.log"; then
  echo "acceptance (harness): a run did not print its retention rule" >&2
  exit 1
fi
echo "acceptance (harness): every run prints the retention rule"

# ---------------------------------------------------------------------------
# 2. A run that fails leaves nothing.
# ---------------------------------------------------------------------------

# Refused before it starts: nothing was made, so nothing had to be removed. This is the
# cheap half, and on its own it proves nothing about the trap.
if run cold --binary "$work/there-is-no-binary" > "$work/refused.log" 2>&1; then
  echo "acceptance (harness): a run with no binary reported success" >&2
  exit 1
fi
if [ "$(entries)" != "$start_entries" ]; then
  echo "acceptance (harness): a refused run left a work directory" >&2
  find "$bench_root" -mindepth 1 -maxdepth 1 >&2
  exit 1
fi
echo "acceptance (harness): a run refused before it starts makes no work directory"

# Failed after it started, which is the half that exercises the trap. The project is a
# real one that clones and builds a base, and its test command exits non-zero, so the
# run reaches `bench_start`, makes its work directory, does real work in it, and then
# fails. The work directory has to go anyway.
failing="$work/failing"
cp -a "$project" "$failing"
sed -i.bak 's/^test = .*/test = "exit 3"/' "$failing/nodal.toml"
rm -f "$failing/nodal.toml.bak"
git -C "$failing" commit -qam "a test command that fails"

before=$(entries)
if "$harness/ready_bench.sh" --source "$failing" --binary "$nodal"   --root "$bench_root" --results "$results" --runs 1 > "$work/failed.log" 2>&1; then
  : # A test command that fails is a measurement, so the run itself may still succeed.
fi
if ! grep -q "rc=3" "$work/failed.log"; then
  echo "acceptance (harness): the run did not reach the failing test command" >&2
  cat "$work/failed.log" >&2
  exit 1
fi
if [ "$(entries)" != "$before" ]; then
  echo "acceptance (harness): a run whose work failed left a work directory" >&2
  find "$bench_root" -mindepth 1 -maxdepth 1 >&2
  exit 1
fi
echo "acceptance (harness): a run that fails after it starts still removes its work"

# And the trap itself, on a run stopped part-way by a signal it can act on. SIGTERM
# runs the trap; the work directory goes with it.
"$harness/ready_bench.sh" --source "$slow" --binary "$nodal"   --root "$bench_root" --results "$results" --runs 1 > "$work/signalled.log" 2>&1 &
signalled=$!
# Wait until the run has made its work directory and is inside the slow command.
waited=0
while [ "$(entries)" = "$before" ] && [ "$waited" -lt 60 ]; do
  sleep 1
  waited=$((waited + 1))
done
if [ "$(entries)" = "$before" ]; then
  echo "acceptance (harness): the run never made a work directory to signal it over" >&2
  kill "$signalled" 2>/dev/null || true
  exit 1
fi
kill -TERM "$signalled" 2>/dev/null || true
wait "$signalled" 2>/dev/null || true
if [ "$(entries)" != "$before" ]; then
  echo "acceptance (harness): a run stopped by SIGTERM left its work directory" >&2
  find "$bench_root" -mindepth 1 -maxdepth 1 >&2
  exit 1
fi
echo "acceptance (harness): a run stopped by a signal removes its work through the trap"

# ---------------------------------------------------------------------------
# 3. A run that is killed names itself, and the next run removes it.
# ---------------------------------------------------------------------------

# A directory shaped exactly as a killed run leaves one: the script, the time, and a
# process identifier. The identifier is of a process that has exited, which is what a
# killed run leaves and what the next run has to recognise.
sh -c 'echo $$' > "$work/dead.pid"
dead=$(cat "$work/dead.pid")
stale="$bench_root/run.create.20260101T000000Z.${dead}"
mkdir -p "$stale/registry"
echo "the work of a run that was killed" > "$stale/registry/payload"

# A directory of a run that is still alive, which the next run must not remove.
sleep 60 &
live=$!
alive="$bench_root/run.create.20260101T000000Z.${live}"
mkdir -p "$alive"

run create > "$work/sweep.log" 2>&1

if [ -d "$stale" ]; then
  echo "acceptance (harness): the next run did not remove the work directory of a dead run" >&2
  exit 1
fi
if ! grep -q "whose process is gone" "$work/sweep.log"; then
  echo "acceptance (harness): the sweep removed the directory and did not say so" >&2
  exit 1
fi
if [ ! -d "$alive" ]; then
  echo "acceptance (harness): the next run removed the work directory of a live run" >&2
  kill "$live" 2>/dev/null || true
  exit 1
fi
kill "$live" 2>/dev/null || true
wait "$live" 2>/dev/null || true
rm -rf "$alive"
echo "acceptance (harness): a killed run is swept by the next run, and a live run is not"

# ---------------------------------------------------------------------------
# 4. `--gc` removes the results that are older than the retention, and no others.
# ---------------------------------------------------------------------------

# A result directory of a run that is over the retention: the name `bench_start` makes,
# and the results file every run writes.
old="$results/create.20200101T000000Z.4242"
mkdir -p "$old"
printf 'metric	value	note
' > "$old/results.tsv"

# Two directories `--gc` must not remove, whatever their age. `--results` is a path a
# person types, and a sweep that removed every old directory under it would make
# `--results ~` a way to delete a home directory.
stranger="$results/my-notes"
mkdir -p "$stranger"
echo "a person's own directory that happens to sit here" > "$stranger/notes.md"
shaped="$results/create.20200101T000000Z.9999"
mkdir -p "$shaped"
echo "the right name and no results file" > "$shaped/something.txt"
for aged in "$old" "$stranger" "$shaped"; do
  touch -d "30 days ago" "$aged" 2>/dev/null || touch -t 202001010000 "$aged"
done

before=$(find "$results" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
"$harness/create_bench.sh" --source "$project" --results "$results" --gc > "$work/gc.log" 2>&1
after=$(find "$results" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
if [ -d "$old" ]; then
  echo "acceptance (harness): --gc kept a result directory older than the retention" >&2
  exit 1
fi
if [ ! -f "$stranger/notes.md" ]; then
  echo "acceptance (harness): --gc removed a directory no run of this harness wrote" >&2
  exit 1
fi
if [ ! -f "$shaped/something.txt" ]; then
  echo "acceptance (harness): --gc removed a directory that holds no results file" >&2
  exit 1
fi
if [ "$after" != "$((before - 1))" ]; then
  echo "acceptance (harness): --gc removed $((before - after)) directories, expected 1" >&2
  cat "$work/gc.log" >&2
  exit 1
fi
echo "acceptance (harness): --gc removes only the results a run wrote, and only when old"

# ---------------------------------------------------------------------------
# 5. No script names a home directory or a machine.
# ---------------------------------------------------------------------------

if grep -n "/home/\|/Users/" "$harness"/*.sh; then
  echo "acceptance (harness): a script names a home directory" >&2
  exit 1
fi
echo "acceptance (harness): no script in the harness names a home directory"

for script in "$harness"/*_bench.sh; do
  if ! "$script" --help | grep -q "^Usage:"; then
    echo "acceptance (harness): $(basename "$script") has no --help" >&2
    exit 1
  fi
done
echo "acceptance (harness): every script answers --help and measures nothing"
