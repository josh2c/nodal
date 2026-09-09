#!/usr/bin/env sh
# Acceptance test for the commit-message check.
#
# The suite builds a fixture repository in a temporary directory, one commit for each
# claim, and runs `ci/commit-messages.sh` over it. The claims:
#   - a prose paragraph whose last line reads `Word: text` passes;
#   - a wrapped prose line that starts with a word and a colon passes;
#   - a last paragraph of nothing but trailer lines fails, and the output names the
#     commit and the lines;
#   - a banned key fails inside a prose paragraph, where no trailer block stands;
#   - `Key:value` with no blank after the colon fails, because git reads it as a trailer;
#   - a merge a person made locally, with a body of its own, is read like any other
#     commit and fails on the trailer in it;
#   - a merge of GitHub's shape, two parents and a body of one line, passes;
#   - a subject line that reads like a trailer passes, because a subject is not a trailer;
#   - a base that names no commit exits 2.
set -eu

script=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/commit-messages.sh

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1

git init --quiet --initial-branch=main .
git config user.name fixture
git config user.email fixture@example.invalid
git config commit.gpgsign false

echo one > file
git add file
git commit --quiet -m "Add the file the rest of the fixture edits"
base=$(git rev-parse HEAD)

serial=0
write() {
    serial=$((serial + 1))
    cat > "$work/message"
    echo "$serial" > file
    git commit --quiet -aF "$work/message"
}

fail() {
    echo "acceptance (commit messages): $1" >&2
    shift
    [ $# -eq 0 ] || printf '%s\n' "$@" >&2
    exit 1
}

# Run the check over the fixture branch and hold both output and exit status.
run() {
    if output=$("$script" "$base" 2>&1); then status=0; else status=$?; fi
}

passes() {
    run
    [ "$status" -eq 0 ] || fail "$1 was rejected" "$output"
    git reset --quiet --hard "$base"
}

rejects() {
    label=$1
    shift
    run
    [ "$status" -eq 1 ] || fail "$label was accepted" "$output"
    for needle in "$@"; do
        case "$output" in
            *"$needle"*) ;;
            *) fail "the output for $label does not name '$needle'" "$output" ;;
        esac
    done
    git reset --quiet --hard "$base"
}

# Prose passes, whatever punctuation it holds.
write <<'MSG'
Change what the file holds

The second paragraph says why, in prose, and names nobody. The check reads it and
finds: nothing worth a complaint.
MSG
write <<'MSG'
Give each attempt a directory nothing else is writing to

An attempt that stops part way leaves the directory behind and the next attempt finds
nothing: the next attempt will find it free.
MSG
write <<'MSG'
Fix: the subject line is not a trailer

A subject can read like a trailer and still be a subject. The body is prose.
MSG
passes "a branch of prose commits"

# A last paragraph of nothing but trailer lines fails, and the output names both lines.
write <<'MSG'
Change what the file holds again

Co-Authored-By: A Tool <tool@example.invalid>
Claude-Session: https://example.invalid/session
MSG
bad=$(git rev-parse HEAD)
rejects "a trailer block" "$bad" "Co-Authored-By: A Tool" "Claude-Session: https://example.invalid/session"

# A banned key fails where it stands, with prose around it and no trailer block.
write <<'MSG'
Change what the file holds once more

This paragraph is prose and the check would let it stand on its own.
Signed-off-by: A Person <person@example.invalid>
And the paragraph carries on in prose after the line, so it is no trailer block.
MSG
rejects "a banned key under prose" "Signed-off-by: A Person"

# No blank after the colon is still a trailer, and so are many blanks.
write <<'MSG'
Change what the file holds yet again

Co-Authored-By:A Tool <tool@example.invalid>
MSG
rejects "a trailer with no blank after the colon" "Co-Authored-By:A Tool"

# A merge a person made locally, with a body of its own, is read like any other commit.
git checkout --quiet -b local-side "$base"
echo side > other
git add other
git commit --quiet -m "Add a file on the side branch"
git checkout --quiet main
cat > "$work/message" <<'MSG'
Merge the side branch by hand

The person who ran the merge wrote this body, and a tool appended the line under it.

Co-Authored-By: A Tool <tool@example.invalid>
MSG
git merge --quiet --no-ff --no-verify -F "$work/message" local-side
merge=$(git rev-parse HEAD)
write <<'MSG'
Carry on after the merge

The merge is inside the range, not at its head.
MSG
rejects "a local merge with a trailer body" "$merge" "Co-Authored-By: A Tool"
git branch --quiet -D local-side

# A merge of GitHub's shape passes: two parents and a body of one line.
git checkout --quiet -b side "$base"
echo four > file
git commit --quiet -am "Fix: the subject line is not a trailer"
git checkout --quiet main
cat > "$work/message" <<'MSG'
Merge pull request #1 from fixture/side

Fix: the subject line is not a trailer
MSG
git merge --quiet --no-ff --no-verify -F "$work/message" side
run
[ "$status" -eq 0 ] || fail "a merge of GitHub's shape was rejected" "$output"
git reset --quiet --hard "$base"
git branch --quiet -D side

# A base that names no commit exits 2, and says so.
if output=$("$script" 0000000000000000000000000000000000000000 2>&1); then status=0; else status=$?; fi
[ "$status" -eq 2 ] || fail "an unknown base did not exit 2" "$output"
case "$output" in
    *"does not name a commit"*) ;;
    *) fail "an unknown base did not say what was wrong" "$output" ;;
esac

echo "acceptance (commit messages): prose passes, trailer blocks and banned keys fail and are named, a merge of GitHub's shape is exempt, an unknown base exits 2"
