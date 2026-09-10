#!/usr/bin/env sh
# The clone curve: how long a base of many files takes at each worker count.
#
# A clone costs metadata and nothing else, so the whole clone is spent waiting on the
# filesystem and one thread leaves most of it idle. This script is how that claim is
# read, and how a change to `workspace/tree.rs` is shown to have helped.
#
# It builds a tree of NODAL_MEASURE_FILES files, flushes it to the disk, and clones it
# once at each of the worker counts in NODAL_MEASURE_WORKERS. It prints the wall time of
# each run and checks the copy against the first run's with `diff -r`, so a run that got
# faster by making a different tree is a failure rather than a number. The first run's
# copy is checked against the source itself, where the source holds nothing the exclusion
# list drops.
#
# NODAL_MEASURE_BASE points it at a tree that already exists instead, which is how a real
# base is read rather than a tree this script imagines. That tree is only read.
#
# The example is built in release, because a measurement of the debug build measures the
# build. `cp -a --reflink=always` of the same tree is timed first, where the platform has
# it. That is the same call per file with no bookkeeping around it, so it is the floor
# this copier is measured against.
#
# CI does not run this. It writes gigabytes of metadata and its numbers are a property
# of the disk under it, so it is a script a person runs on the machine they care about.
#
# Measured on 2026-09-10, on btrfs, on a twenty-eight core machine. Each number is what
# the example reported, which is the clone and nothing around it.
#
# Over 200000 files in 6000 directories, 4.1 GB:
#
#   cp -a --reflink=always   3.1 s
#   1 worker                 3.3 s     (3.2 s before this copier ran on more than one)
#   4 workers                1.6 s
#   8 workers                1.5 s
#   16 workers               1.5 s
#
# Over a real base of 57374 entries, a pnpm project with its dependencies installed:
#
#   cp -a --reflink=always   0.9 s
#   1 worker                 1.0 s     (1.0 s before)
#   4 workers                0.5 s
#   8 workers                0.4 s
#   16 workers               0.5 s
#
# The curve is flat from eight workers on, on both trees, which is where
# `workspace::tree::WORKER_CEILING` comes from.
#
# Usage: ci/measure-materialize.sh
# Environment:
#   NODAL_MEASURE_BASE      an existing tree to clone, instead of one built here
#   NODAL_MEASURE_FILES     how many files the tree holds (default 200000)
#   NODAL_MEASURE_WORKERS   the worker counts to time (default "1 4 8 16")
#   NODAL_MEASURE_ROOT      where the tree is built (default under the build directory)
#
# Each row prints the wall time of the run and, after it, what the example itself
# reported. The first number holds the process start; the second is the clone alone.
set -eu

files=${NODAL_MEASURE_FILES:-200000}
counts=${NODAL_MEASURE_WORKERS:-"1 4 8 16"}
scratch=${NODAL_MEASURE_ROOT:-${CARGO_TARGET_DIR:-target}}
mkdir -p "$scratch"
root=$(mktemp -d "$scratch/measure-materialize.XXXXXX")
trap 'chmod -R u+rwX "$root" 2> /dev/null || true; rm -rf "$root"' EXIT
base=$root/base

# The tree: files spread over directories the way a dependency store spreads them, plus
# the two shapes a copier gets wrong. One file in each directory is read-only, and one
# file has a second name in another directory.
if [ -n "${NODAL_MEASURE_BASE:-}" ]; then
    base=$NODAL_MEASURE_BASE
    echo "measure (materialize): cloning $base, which holds $(find "$base" | wc -l) entries"
else
    echo "measure (materialize): building a base of $files files"
    python3 - "$base" "$files" <<'BUILD'
import os, sys

base, count = sys.argv[1], int(sys.argv[2])
per = 100
os.makedirs(base, exist_ok=True)
for made in range(0, count, per):
    directory = os.path.join(base, "store", f"{made // per:05d}", "dist", "lib")
    os.makedirs(directory, exist_ok=True)
    for index in range(min(per, count - made)):
        path = os.path.join(directory, f"file-{index:03d}.txt")
        with open(path, "w") as file:
            file.write(f"{made + index}\n" * (8 + index * 64))
        if index == 0:
            os.chmod(path, 0o444)
        if index == 1 and made:
            os.link(path, os.path.join(base, "store", "00000", f"linked-{made:07d}.txt"))
BUILD
fi

# A base is built long before a home is made from it, so its data is on the disk. The
# tree above is seconds old, and a clone of a file still in memory has to write it out
# first. Flush it outside every timed section.
sync

# The state root goes on the filesystem being measured, because the backend comes from
# the answer recorded there rather than from a probe taken where the clone lands.
state=$root/state
mkdir -p "$state"
cargo build --locked --release -q -p nodal-core --example materialize
example=${CARGO_TARGET_DIR:-target}/release/examples/materialize

# The clone every later clone is compared against: the first one made.
reference=""

# One run: clone the base at $1 workers, time it, and check what it made.
run() {
    workers=$1
    home=$root/home-$workers
    # The run before this one wrote a whole tree. Let the disk finish with it, or its
    # writeback lands inside the next measurement rather than its own.
    sync
    started=$(date +%s.%N)
    report=$(NODAL_HOME=$state NODAL_MATERIALIZE_WORKERS=$workers "$example" "$base" "$home")
    ended=$(date +%s.%N)
    elapsed=$(awk -v a="$started" -v b="$ended" 'BEGIN { printf "%.2f", b - a }')
    if [ -z "$reference" ]; then
        reference=$home
    else
        diff -r "$reference" "$home"
        chmod -R u+rwX "$home"
        rm -rf "$home"
    fi
    printf '  %-24s %8s s   %s\n' "$workers workers" "$elapsed" "$report"
}

echo "measure (materialize): the floor, and then the curve"
if cp -a --reflink=always "$base" "$root/floor" 2> /dev/null; then
    rm -rf "$root/floor"
    sync
    started=$(date +%s.%N)
    cp -a --reflink=always "$base" "$root/floor"
    ended=$(date +%s.%N)
    printf '  %-24s %8s s\n' "cp -a --reflink" \
        "$(awk -v a="$started" -v b="$ended" 'BEGIN { printf "%.2f", b - a }')"
    chmod -R u+rwX "$root/floor"
    rm -rf "$root/floor"
else
    echo "  cp -a --reflink is not available here; there is no floor to compare against"
fi

for workers in $counts; do
    run "$workers"
done

# The source itself, where nothing in it is a row the exclusion list drops. A real base
# holds such rows, and a clone that left one out is right rather than different.
if [ -z "${NODAL_MEASURE_BASE:-}" ]; then
    diff -r "$base" "$reference"
    echo "measure (materialize): every clone holds the base, and every clone is the same"
else
    echo "measure (materialize): every clone is the same tree"
fi
