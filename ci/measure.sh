#!/usr/bin/env sh
# Structural measurements of the workspace, with a ceiling on each one that has a cost.
#
# The refactor programme starts here. A refactor that is not measured before it starts
# cannot be shown to have helped, and a cost that nothing watches grows back. This
# script prints every number on every run, pass or fail, so the record a later
# recalibration needs is in the log of any green run. It gates only the numbers that
# have a stated ceiling, and it reports the rest.
#
# Every ceiling below states the value measured when the ceiling was written. The
# baselines come from the refactor-readiness audit of 2026-09-08, which measured
# `origin/main` at 2cbbad5. Where this script's own definition gives a different number
# from the audit's, both are stated and the ceiling follows this script, because this
# script is what CI runs.
#
# A ceiling is set just above the worst current acceptable state, not at a target. Each
# one ratchets down as the hotspot behind it is fixed; the row says what to lower it to.
#
# Usage: ci/measure.sh
# Environment:
#   NODAL_MEASURE_UNITS   how many units the git-process fixture builds (default 10)
#   NODAL_MEASURE_SKIP_GIT  set to 1 to leave out the git-process measurements, which
#                           build the release binary and create units on disk
set -eu

units=${NODAL_MEASURE_UNITS:-10}
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
failures=0

# Report one number, and fail when it is over its ceiling.
#
# $1 name, $2 measured value, $3 ceiling, $4 unit, $5 the baseline and the ratchet.
gate() {
    verdict=$(awk -v value="$2" -v ceiling="$3" 'BEGIN { print (value > ceiling) ? "OVER" : "ok" }')
    printf '  %-44s %10s %-14s ceiling %s\n' "$1" "$2" "$4" "$3"
    if [ "$verdict" = "OVER" ]; then
        printf '    OVER CEILING: %s is %s %s, over %s. %s\n' "$1" "$2" "$4" "$3" "$5" >&2
        failures=$((failures + 1))
    fi
}

# Report one number that nothing is gated on.
report() {
    printf '  %-44s %10s %-14s %s\n' "$1" "$2" "$3" "${4:-}"
}

echo "measure: structural ceilings, baselines from the audit of 2026-09-08 at 2cbbad5"
echo

# ---------------------------------------------------------------------------
# Git processes per command.
#
# Every command compiles every unit's memory, so the number of git processes a command
# spawns grows with the number of units in the project. The measurement puts a shim
# first on PATH that counts each call and then runs the real git, builds the shapes
# fixture, and creates units in a temporary NODAL_HOME.
# ---------------------------------------------------------------------------
if [ "${NODAL_MEASURE_SKIP_GIT:-0}" = "1" ]; then
    echo "git processes: left out (NODAL_MEASURE_SKIP_GIT=1)"
else
    echo "git processes (shapes fixture, $units units, release binary)"
    cargo build --release --locked -q -p nodal-cli -p nodal-fixture
    binary=$root/target/release/nodal

    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT INT TERM
    origin=$("$root/target/release/nodal-fixture" --shapes "$work/shapes" | head -n 1)
    git clone --quiet "$origin" "$work/project"

    mkdir -p "$work/shim"
    cat > "$work/shim/git" <<'SHIM'
#!/bin/sh
printf '%s\n' "$*" >> "$NODAL_GIT_LOG"
exec /usr/bin/git "$@"
SHIM
    chmod +x "$work/shim/git"

    NODAL_GIT_LOG=$work/git.log
    export NODAL_GIT_LOG
    NODAL_HOME=$work/state
    export NODAL_HOME
    PATH="$work/shim:$PATH"
    export PATH

    cd "$work/project"
    : > "$NODAL_GIT_LOG"
    "$binary" init > /dev/null 2>&1

    last_create=0
    number=1
    while [ "$number" -le "$units" ]; do
        : > "$NODAL_GIT_LOG"
        "$binary" new "measure unit $number" --name "u$number" > /dev/null 2>&1
        last_create=$(wc -l < "$NODAL_GIT_LOG" | tr -d ' ')
        number=$((number + 1))
    done

    : > "$NODAL_GIT_LOG"
    "$binary" ls > /dev/null 2>&1
    list_total=$(wc -l < "$NODAL_GIT_LOG" | tr -d ' ')
    cd "$root"

    per_row=$(awk -v total="$list_total" -v units="$units" 'BEGIN { printf "%.1f", total / units }')

    # Baseline 9.0 per row at 10 units on the shapes fixture, where every unit sits at
    # the base tip. A project whose units are ahead of the base measured 11.0 (DL-047),
    # so the ceiling holds both shapes. Ratchet to 10 after the pointer change, then 8.
    gate "git processes per list row" "$per_row" 12 "per row" \
        "baseline 9.0 at 10 units; ratchet to 10, then 8"

    # Baseline 112 for the tenth create, which is 40 for the create itself and 9 for
    # each of the nine units already there. Ratchet to 90.
    gate "git processes, create number $units" "$last_create" 120 "processes" \
        "baseline 112 at the tenth create; ratchet to 90"

    report "git processes, list total" "$list_total" "processes" "over $units units"
fi
echo

# ---------------------------------------------------------------------------
# What the build produces.
# ---------------------------------------------------------------------------
echo "build"
if [ "${NODAL_MEASURE_SKIP_GIT:-0}" = "1" ]; then
    cargo build --release --locked -q -p nodal-cli
fi
size=$(wc -c < "$root/target/release/nodal" | tr -d ' ')
# Baseline 8,048,288 bytes on x86_64-unknown-linux-gnu, stripped. The ceiling is the
# next round million above it.
gate "release binary size" "$size" 9000000 "bytes" "baseline 8,048,288 on linux"

packages=$(grep -c '^\[\[package\]\]' "$root/Cargo.lock")
# Baseline 127 packages in the lockfile; 83 crates in the nodal-cli normal tree.
gate "lockfile packages" "$packages" 135 "packages" "baseline 127"

duplicate_versions=$(
    cargo tree --workspace -d --locked 2>/dev/null | awk '/^[a-zA-Z]/ { print $1 }' | sort -u | wc -l | tr -d ' '
)
# Baseline 2: syn at 2 and 3, winnow at 0.7 and 1.0. Reported, not gated: cargo-deny
# already warns on it, and the ratchet to 0 belongs with a dependency change.
report "duplicate crate versions" "$duplicate_versions" "crates" "baseline 2 (syn, winnow)"
echo

# ---------------------------------------------------------------------------
# Duplication. The method is `ci/duplication.py`, which states it in full.
# ---------------------------------------------------------------------------
echo "duplication (6-line windows, ci/duplication.py)"
duplication=$(python3 "$root/ci/duplication.py" "$root" --json)
product_percent=$(printf '%s' "$duplication" | python3 -c 'import json,sys; print(f"{json.load(sys.stdin)["product"]["percent"]:.1f}")')
test_percent=$(printf '%s' "$duplication" | python3 -c 'import json,sys; print(f"{json.load(sys.stdin)["test"]["percent"]:.1f}")')

# Baseline 3.0% by this script. The audit measured 3.3% with the same method over a
# corpus it did not commit; this script's corpus is stated in its own header and is what
# the ceiling follows. The four lifecycle operations are the whole of it. Ratchet to 3%
# after the shared subject and the sweep land.
gate "duplication, product" "$product_percent" 4.0 "percent" \
    "baseline 3.0% here, 3.3% in the audit; ratchet to 3%"

# Baseline 7.3% by this script, which counts the inline test modules of the product
# files in the test corpus as well as the test crates. It was 7.1% before the behaviour
# locks were added; their fixture is one more copy of the one every test file makes, and
# the shared test kit is what removes all of them. Ratchet to 6% after that lands.
gate "duplication, test" "$test_percent" 8.0 "percent" \
    "baseline 7.3%; ratchet to 6% after the shared test kit"
echo

# ---------------------------------------------------------------------------
# Structure.
# ---------------------------------------------------------------------------
echo "structure"
# A doc-comment mention is a link, not an import, so lines that start a comment are
# dropped before the files are counted.
importers=$(
    {
        grep -rn 'crate::lifecycle' --include='*.rs' "$root/crates/nodal-core/src" \
            | grep -v '/src/lifecycle/' || true
        grep -rn 'nodal_core::lifecycle' --include='*.rs' "$root/crates/nodal-cli/src" || true
    } | grep -vE ':[0-9]+: *//' | cut -d: -f1 | sort -u | wc -l | tr -d ' '
)
# Baseline 26 files: 14 in the core outside the module, and 12 in the CLI. The audit
# counted 24 by reading the doc links out by hand rather than by this filter. The
# framework and the operations are one module with one name; the move to its own module
# takes this to 0.
gate "files importing lifecycle" "$importers" 26 "files" "baseline 26; ratchet to 0"

sinks=$(grep -r 'Arc<OnceLock' --include='*.rs' "$root/crates"/*/src | wc -l | tr -d ' ')
# Baseline 13 mentions over 7 distinct sinks. A step cannot hand a value to the step
# after it, and these are what four operations use instead. The step-output column takes
# this to 0.
gate "Arc<OnceLock> mentions in product src" "$sinks" 13 "mentions" \
    "baseline 13 over 7 sinks; ratchet to 0"

spawns=$(grep -r 'Command::new' --include='*.rs' "$root/crates"/*/src | wc -l | tr -d ' ')
# Baseline 9 sites for 6 tools: git, docker, the hook shell, the user's shell twice, the
# user's command, and the package manager. One spawn seam per tool is the rule.
gate "Command::new sites in product src" "$spawns" 9 "sites" "baseline 9 for 6 tools"

asynchronous=$(grep -rE '\basync\b|\.await\b|\btokio\b' --include='*.rs' "$root/crates"/*/src | wc -l | tr -d ' ')
# Baseline 0. Nodal is synchronous by decision.
gate "async, await and tokio in product src" "$asynchronous" 0 "occurrences" "baseline 0"
echo

# ---------------------------------------------------------------------------
# Unsafe.
# ---------------------------------------------------------------------------
echo "unsafe"
sites=$(grep -rc '\bunsafe\b' --include='*.rs' "$root/crates"/*/src | awk -F: '{ total += $2 } END { print total + 0 }')
report "unsafe mentions in product src" "$sites" "mentions" "baseline 31 blocks and functions"

undocumented=$(
    cargo clippy --workspace --all-targets --locked --message-format=short \
        -- -W clippy::undocumented_unsafe_blocks 2>&1 \
        | grep -c 'unsafe block missing a safety comment' || true
)
# Baseline 8 before this task, all in workspace/xattr.rs, and 0 after it. Every unsafe
# block in the workspace now states why it is sound. The ceiling is 0 from here on, so a
# new block without a comment fails the build rather than joining a backlog.
gate "undocumented unsafe blocks" "$undocumented" 0 "blocks" \
    "the audit measured 8, all in xattr.rs; every one is now documented"
echo

if [ "$failures" -ne 0 ]; then
    echo "measure: $failures measurement(s) over ceiling" >&2
    exit 1
fi
echo "measure: every gated measurement is inside its ceiling"
