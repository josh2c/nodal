#!/usr/bin/env sh
# Acceptance test for making a unit home out of a base.
#
# Three claims, checked on whatever filesystem the checkout is on.
#
#   1. A clone leaves out what the exclusion list names, and keeps everything else.
#      The tree here is the fixture project plus the generated state a real base
#      carries: installed dependencies, which a home keeps warm, and a path-bound
#      build cache and test output, which it never receives.
#   2. A clone is fast. The budget is NODAL_MATERIALIZE_BUDGET_SECONDS, one second on
#      a filesystem that shares blocks, and NODAL_MATERIALIZE_COPY_BUDGET_SECONDS
#      where the bytes have to be copied.
#   3. Where the filesystem shares blocks, the clone holds none of its own. On btrfs
#      that is `btrfs filesystem du`, which reports how much of a tree is exclusive to
#      it; a clone that shares every block is exclusive of nothing.
#
# The third claim is skipped where the filesystem cannot share blocks, which is what a
# continuous-integration runner usually has. The first two are checked everywhere.
set -eu

payload=${NODAL_MATERIALIZE_PAYLOAD_MB:-64}
budget=${NODAL_MATERIALIZE_BUDGET_SECONDS:-1}
copy_budget=${NODAL_MATERIALIZE_COPY_BUDGET_SECONDS:-30}

# The trees are made inside the build directory, so they are on the filesystem the
# project is checked out on and the backend measured is the one a person here gets.
# The directory is made first: on a clean checkout nothing has built yet, so the build
# directory is not there and mktemp has nowhere to put the tree.
scratch=${CARGO_TARGET_DIR:-target}
mkdir -p "$scratch"
root=$(mktemp -d "$scratch/acceptance-materialize.XXXXXX")
trap 'rm -rf "$root"' EXIT
base=$root/base
home=$root/home

cargo run --locked -q -p nodal-fixture -- "$base"

# Installed dependencies: bytes a home keeps, because a warm install is the point.
mkdir -p "$base/node_modules/.pnpm/react/lib"
dd if=/dev/urandom of="$base/node_modules/.pnpm/react/lib/index.js" bs=1M count="$payload" status=none
ln "$base/node_modules/.pnpm/react/lib/index.js" "$base/node_modules/react-linked.js"
ln -s .pnpm/react/lib "$base/node_modules/react"
# Generated state a home never receives: a cache that records its own path, output of
# a run that did not happen here, and a checkout another tool made.
mkdir -p "$base/.next/cache/webpack" "$base/test-results/run" "$base/.claude/worktrees/other"
dd if=/dev/urandom of="$base/.next/cache/webpack/0.pack" bs=1M count=8 status=none
dd if=/dev/urandom of="$base/test-results/run/report.bin" bs=1M count=8 status=none
dd if=/dev/urandom of="$base/.claude/worktrees/other/checkout.bin" bs=1M count=8 status=none

# A base is built long before a home is made from it, so its data is on the disk. The
# bytes above are seconds old, and a clone of a file still in memory has to write it
# out first: measured here, that flush is the whole cost. Flush it outside the timed
# section, so what is measured is the clone.
sync

# The backend comes from the answer recorded for the state root, not from a probe taken
# where the clone lands. So the state root is put on the filesystem being measured, and
# the example records the answer there on its first run.
report=$(NODAL_HOME="$root/state" cargo run --locked -q -p nodal-core --example materialize -- "$base" "$home")
echo "acceptance (materialize): $report"
backend=$(echo "$report" | sed 's/.*"backend":"\([^"]*\)".*/\1/')
seconds=$(echo "$report" | sed 's/.*"seconds":\([0-9.]*\).*/\1/')

for left_out in .next/cache test-results coverage .claude/worktrees; do
  if [ -e "$home/$left_out" ]; then
    echo "acceptance (materialize): $left_out is in the home" >&2
    exit 1
  fi
done
for kept in package.json apps/web/app/page.tsx node_modules/.pnpm/react/lib/index.js; do
  if [ ! -e "$home/$kept" ]; then
    echo "acceptance (materialize): $kept is not in the home" >&2
    exit 1
  fi
done
if [ ! -L "$home/node_modules/react" ]; then
  echo "acceptance (materialize): the link in node_modules is not a link" >&2
  exit 1
fi
diff -r "$base/apps" "$home/apps"
cmp "$base/node_modules/.pnpm/react/lib/index.js" "$home/node_modules/.pnpm/react/lib/index.js"
first=$(ls -i "$home/node_modules/.pnpm/react/lib/index.js" | cut -d' ' -f1)
second=$(ls -i "$home/node_modules/react-linked.js" | cut -d' ' -f1)
if [ "$first" != "$second" ]; then
  echo "acceptance (materialize): two names for one file became two files" >&2
  exit 1
fi
echo "acceptance (materialize): the home holds the project and none of the excluded state"

if [ "$backend" = "copy" ]; then
  limit=$copy_budget
else
  limit=$budget
fi
over=$(awk -v seconds="$seconds" -v limit="$limit" 'BEGIN { print (seconds > limit) ? 1 : 0 }')
echo "acceptance (materialize): $backend cloned ${payload} MB in ${seconds}s (budget ${limit}s)"
if [ "$over" = "1" ]; then
  echo "acceptance (materialize): over budget" >&2
  exit 1
fi

if [ "$backend" = "copy" ]; then
  echo "acceptance (materialize): this filesystem cannot share blocks; nothing to measure"
  exit 0
fi
if ! command -v btrfs > /dev/null 2>&1 || ! btrfs filesystem du -s "$home" > /dev/null 2>&1; then
  echo "acceptance (materialize): no tool here reports shared blocks; the claim is unmeasured"
  exit 0
fi
exclusive=$(btrfs filesystem du -s "$home" | awk 'NR == 2 { print $2 }')
echo "acceptance (materialize): the home is exclusive of $exclusive"
if [ "$exclusive" != "0.00B" ]; then
  echo "acceptance (materialize): the clone holds blocks of its own" >&2
  exit 1
fi
echo "acceptance (materialize): every block of the home is shared with the base"
