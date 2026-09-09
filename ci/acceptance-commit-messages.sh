#!/usr/bin/env sh
# Acceptance test for the commit-message check.
#
# The suite builds a fixture repository in a temporary directory and runs
# `ci/commit-messages.sh` against it. Four claims:
#   - a branch of prose commits passes;
#   - a commit whose last paragraph holds a trailer line fails, and the output names
#     the commit and the line;
#   - a merge commit GitHub made passes, even when its body reads like a trailer;
#   - a subject line that reads like a trailer passes, because a subject is not a
#     trailer.
set -eu

script=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/commit-messages.sh

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

git init --quiet --initial-branch=main .
git config user.name fixture
git config user.email fixture@example.invalid
git config commit.gpgsign false

echo one > file
git add file
git commit --quiet -m "Add the file the rest of the fixture edits"
base=$(git rev-parse HEAD)

# A branch of prose commits passes.
echo two > file
git commit --quiet -am "Change what the file holds

The second paragraph says why, in prose, and names nobody."
if ! "$script" "$base" > /dev/null; then
    echo "acceptance (commit messages): a branch of prose commits was rejected" >&2
    exit 1
fi

# A trailer line in the last paragraph fails, and the output names commit and line.
echo three > file
git commit --quiet -am "Change what the file holds again

Co-Authored-By: A Tool <tool@example.invalid>"
bad=$(git rev-parse HEAD)
if output=$("$script" "$base" 2>&1); then
    echo "acceptance (commit messages): a trailer line was accepted" >&2
    exit 1
fi
case "$output" in
    *"$bad"*) ;;
    *) echo "acceptance (commit messages): the output does not name the commit" >&2
       echo "$output" >&2
       exit 1 ;;
esac
case "$output" in
    *"Co-Authored-By: A Tool"*) ;;
    *) echo "acceptance (commit messages): the output does not name the line" >&2
       echo "$output" >&2
       exit 1 ;;
esac

# A merge commit GitHub made passes, and so does a subject that reads like a trailer.
git checkout --quiet -b side "$base"
echo four > file
git commit --quiet -am "Fix: the subject line is not a trailer"
git checkout --quiet main
git reset --quiet --hard "$base"
git merge --quiet --no-ff -m "Merge pull request #1 from fixture/side

Fix: the subject line is not a trailer" side
if ! "$script" "$base" > /dev/null; then
    echo "acceptance (commit messages): a GitHub merge commit or a trailer-shaped subject was rejected" >&2
    exit 1
fi

echo "acceptance (commit messages): prose passes, a trailer line fails and is named, a GitHub merge commit is exempt"
