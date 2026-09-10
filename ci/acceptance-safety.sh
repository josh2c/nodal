#!/usr/bin/env sh
# Acceptance test for the safety suite.
#
# The properties that make two units of one project independent, each one named test a
# reviewer can cite, each on the fixture project, driven through the binary:
#
#   - source isolation: a file written in one unit is in that unit and in no other tree;
#   - git isolation: a branch, a commit, a stash and a ref made in one unit reach
#     nothing else, and `git worktree list` in a unit names that unit alone;
#   - port isolation: two units are never granted one port, sixteen callers racing from
#     sixteen registry connections come away with distinct ports, and a socket opened on
#     one unit's port is reported for that unit only;
#   - base immutability: a unit's writes never reach the tree it was cloned from, and
#     the base is the same bytes after ten units are made from it;
#   - tracked excludes: a project that commits into a directory the exclusion table
#     names is refused, and the message names the path;
#   - reclaim refusal: each of the three kinds of work that exists nowhere else refuses
#     the reclaim, and the home and the work are still there afterwards;
#   - reclaim scope: a reclaim stops its tether and never a process that carries no unit
#     identifier, it names that process and refuses to move the home over it, and
#     `--force` moves the home and still leaves the process running;
#   - doctor: the checkout, a worktree of that checkout that lives beside it rather than
#     inside it, and the whole state directory are the same bytes afterwards.
#
# The suite is then run a second time with the temporary directory reached through a
# symbolic link, as `ci/acceptance-list.sh` and `ci/acceptance-doctor.sh` do and for the
# same reason. macOS names /var/folders and means /private/var/folders, so every path in
# a test arrives there under two names; Linux has no such link, so the condition is made
# rather than waited for, and both hosts check it.
#
# Output is not captured, because a check this host cannot make says so on standard
# output and the point of saying it is that somebody reads it.
set -eu

# The suite drives the binary, and Cargo names a binary to the package that declares it,
# so the package under test has to be built before the package that types it runs.
cargo build --locked -p nodal-cli
cargo test --locked -p nodal-safety -- --nocapture

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-safety -- --nocapture

echo "acceptance (safety): two units of one project cannot reach each other, under a linked path too"
