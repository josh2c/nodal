#!/usr/bin/env sh
# Acceptance test for the commit-message check.
#
# The suite builds a fixture repository in a temporary directory, writes one commit for
# each claim inside the range the check is given, and runs `ci/commit-messages.sh` over
# that range. The claims:
#   - a prose paragraph passes;
#   - a prose paragraph whose last line reads `Result: nothing changes` passes, because
#     git reads a trailer only where the whole last paragraph is trailer lines;
#   - a bare link as the last line of a prose paragraph passes, for the same reason;
#   - a bare link standing alone as the last paragraph fails, because git reads it as
#     the trailer key `https`, and the output tells the contributor what to do;
#   - a last paragraph of nothing but trailer lines fails, and the output names the
#     commit and the lines;
#   - `Co-Authored-By` glued under a prose line fails, where git reads no trailer;
#   - `Session:` glued under a prose line fails, for the key that ends in `-session`;
#   - `Generated with` in prose fails, behind the emoji a harness puts before it;
#   - a merge a person made locally, with a trailer in its body, fails: this is the case
#     that pays for reading merges, which is what a pull request is read without;
#   - `--no-merges` skips that same merge, and skips a merge of GitHub's shape whose
#     one-line body is a pull request title git reads as a trailer: this is what a push
#     to `main` is read with, because every merge that reaches `main` is GitHub's;
#   - a subject that reads like a trailer passes, because the subject is never read;
#   - a range whose left side is all zeros exits 0 on a push and says so;
#   - a range whose left side names no commit exits 2 on a pull request.
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

# Run the check over the fixture branch and hold both output and exit status. Any flags
# the check takes stand before the range.
run() {
    if output=$("$script" "$@" "$base..HEAD" 2>&1); then status=0; else status=$?; fi
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

# Prose passes, whatever punctuation it holds, and whatever the subject reads like.
write <<'MSG'
Change what the file holds

The second paragraph says why, in prose, and names nobody. The check reads it and
finds: nothing worth a complaint.
MSG
write <<'MSG'
Give each attempt a directory nothing else is writing to

An attempt that stops part way leaves the directory behind, and the next attempt has
to find it free.
Result: nothing changes
MSG
write <<'MSG'
Read the column list the caller gives

The change came from the page that documents the flag, at
https://example.invalid/flags/sort-by
MSG
write <<'MSG'
Sort-by: accept a column list

A subject can read like a trailer and still be a subject. The body is prose.
MSG
passes "a branch of prose commits"

# A bare link alone in the last paragraph is a trailer to git, and the check says what
# to do about it rather than leaving the contributor to guess.
write <<'MSG'
Change what the file holds after the link

The paragraph says where the change came from.

https://example.invalid/flags/sort-by
MSG
rejects "a bare link alone in the last paragraph" \
    "//example.invalid/flags/sort-by" "write the link into the"

# A last paragraph of nothing but trailer lines fails, and the output names both lines.
write <<'MSG'
Change what the file holds again

Co-Authored-By: A Tool <tool@example.invalid>
Claude-Session: https://example.invalid/session
MSG
bad=$(git rev-parse HEAD)
rejects "a trailer block" "$bad" "Co-Authored-By: A Tool" "Claude-Session: https://example.invalid/session"

# The lines a harness appends fail where they stand, under prose, where git reads no
# trailer at all.
write <<'MSG'
Change what the file holds once more

This paragraph is prose and the check would let it stand on its own.
Co-Authored-By: A Tool <tool@example.invalid>
And the paragraph carries on in prose after the line, so it is no trailer block.
MSG
rejects "Co-Authored-By under prose" "Co-Authored-By: A Tool"

write <<'MSG'
Change what the file holds yet again

This paragraph is prose and the check would let it stand on its own.
Session: https://example.invalid/session
And the paragraph carries on in prose after the line, so it is no trailer block.
MSG
rejects "a Session key under prose" "Session: https://example.invalid/session"

write <<'MSG'
Change what the file holds one last time

This paragraph is prose and the check would let it stand on its own.
Generated with a tool that writes the line, and the paragraph carries on after it.
MSG
rejects "Generated with in prose" "Generated with a tool"

# A merge a person made locally, with a trailer in its body, is read like any other
# commit. Deleting the merge from the range makes this claim fail.
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

# The push step reads the same range with `--no-merges`, and the merge goes unread.
run --no-merges
[ "$status" -eq 0 ] || fail "--no-merges read a merge" "$output"
case "$output" in
    *"merges not read"*) ;;
    *) fail "--no-merges did not say the merges went unread" "$output" ;;
esac
git reset --quiet --hard "$base"
git branch --quiet -D local-side

# A merge of GitHub's shape is what reaches main: two parents and a body of one line,
# the pull request title. Git reads that line as a trailer whenever the title holds a
# colon, and the title is a person's. The push step reads past it; a pull request, where
# such a merge cannot appear, still fails it.
git checkout --quiet -b github-side "$base"
echo four > file
git commit --quiet -am "Add the commit the pull request holds"
git checkout --quiet main
cat > "$work/message" <<'MSG'
Merge pull request #1 from fixture/github-side

Doctor: names attribute Docker leftovers, and branches join the report
MSG
git merge --quiet --no-ff --no-verify -F "$work/message" github-side
github=$(git rev-parse HEAD)
run --no-merges
[ "$status" -eq 0 ] || fail "--no-merges failed a merge of GitHub's shape" "$output"
rejects "a merge of GitHub's shape read without the flag" "$github" "Doctor: names attribute"
git branch --quiet -D github-side

# A range whose left side is all zeros is what a first push and a force-push give. On a
# push that is nothing to fail a person for, so the check says so and exits 0.
zeros=0000000000000000000000000000000000000000
if output=$(GITHUB_EVENT_NAME=push "$script" "$zeros..HEAD" 2>&1); then status=0; else status=$?; fi
[ "$status" -eq 0 ] || fail "an all-zero base on a push did not exit 0" "$output"
case "$output" in
    *"cannot be read"*) ;;
    *) fail "an all-zero base on a push did not say the range cannot be read" "$output" ;;
esac

# On a pull request the same range is a broken call, and so is a base that names no
# commit.
if output=$(GITHUB_EVENT_NAME=pull_request "$script" "$zeros..HEAD" 2>&1); then status=0; else status=$?; fi
[ "$status" -eq 2 ] || fail "an all-zero base on a pull request did not exit 2" "$output"

unknown=1111111111111111111111111111111111111111
if output=$(GITHUB_EVENT_NAME=pull_request "$script" "$unknown..HEAD" 2>&1); then status=0; else status=$?; fi
[ "$status" -eq 2 ] || fail "a base that names no commit did not exit 2" "$output"
case "$output" in
    *"names no commit"*) ;;
    *) fail "a base that names no commit did not say what was wrong" "$output" ;;
esac

echo "acceptance (commit messages): prose passes, what git calls a trailer and what a harness appends both fail and are named, a merge is read without --no-merges and skipped with it, an unreadable range exits 0 on a push and 2 on a pull request"
