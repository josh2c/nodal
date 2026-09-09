#!/usr/bin/env sh
# Check that the commits a change adds carry no trailer lines.
#
# A commit message in this project is prose. The person who wrote the commit is its
# author, and the author field already says so. A trailer line adds a second, weaker
# claim about who or what wrote the change.
#
# Git decides what a trailer is. A commit fails when
# `git log -1 --format='%(trailers:only,unfold)' <sha>` prints anything, which is when
# the last paragraph of the body is nothing but `Key: value` lines. A `Word: text` line
# inside a prose paragraph is not a trailer to git and is not one here. A line that
# stands alone as the last paragraph is one, whatever it holds: a bare link alone at the
# end reads as the key `https`, so write the link into a sentence of the paragraph above
# it. The failure output says this.
#
# One rule stands beside git's, for the lines a coding harness appends where git would
# not read them. A body line fails when it starts with `co-authored-by`, when its key
# ends in `session` (`Session` and `Claude-Session` both), or when it holds
# `generated with` anywhere in the line, because a harness puts an emoji before it. Case
# is ignored. The subject is never read, so a subject such as
# `Sort-by: accept a column list` passes.
#
# Usage: ci/commit-messages.sh <range>
#
# The range is given, never inferred from the shape of HEAD. The workflow passes
# `base..head` on a pull request and `before..after` on a push, so the check reads the
# commits the event added and nothing else. GitHub's synthetic merge commit is never
# inside such a range, so there is no merge to exempt: every merge in range is one a
# person made, its body is theirs, and it is read like any other commit.
#
# The left side of the range can be unreadable through no fault of a message. A first
# push and a force-push both leave `github.event.before` all zeros or naming a commit
# this clone does not hold. That is nothing to fail a person for, so the check says the
# range cannot be read and exits 0 on a push. Anywhere else an unreadable range is a
# broken call, and it exits 2.
set -eu

range=${1:-}
case "$range" in
    *..*) ;;
    *)
        echo "usage: ci/commit-messages.sh <range>" >&2
        exit 2
        ;;
esac
base=${range%%..*}

# Exit 0 where an unreadable base is ordinary, and 2 where it is a broken call.
unreadable() {
    echo "commit-messages: the range $range cannot be read: $1" >&2
    [ "${GITHUB_EVENT_NAME:-}" = push ] && exit 0
    exit 2
}

case "$base" in
    "") unreadable "it has no left side" ;;
    *[!0]*) ;;
    *) unreadable "its left side is all zeros" ;;
esac
git rev-parse --verify --quiet "$base^{commit}" > /dev/null ||
    unreadable "$base names no commit in this repository"

# The lines of one commit that break the rule: what git reads as a trailer, then the
# body lines a harness writes where git would not read one. A line both rules name is
# printed once.
harness='^(co-authored-by|[A-Za-z0-9-]*session)[[:blank:]]*:|generated with'

bad_lines() {
    trailers=$(git log -1 --format='%(trailers:only,unfold)' "$1")
    appended=$(git log -1 --format='%b' "$1" | grep -iE "$harness" || true)
    printf '%s\n%s\n' "$trailers" "$appended" |
        grep -v '^[[:blank:]]*$' | awk '!seen[$0]++'
}

count=0
failures=0
for sha in $(git rev-list "$range"); do
    count=$((count + 1))
    lines=$(bad_lines "$sha")
    [ -n "$lines" ] || continue
    failures=$((failures + 1))
    {
        echo "commit-messages: $sha $(git log -1 --format='%s' "$sha")"
        printf '%s\n' "$lines" | sed 's/^/    trailer line: /'
    } >&2
done

if [ "$failures" -ne 0 ]; then
    echo "commit-messages: $failures commit message(s) hold a trailer line." >&2
    echo "A commit message is prose. The commit author is the author. Remove the line" >&2
    echo "and rewrite the history of the branch. A last paragraph of one line that holds" >&2
    echo "a colon is a trailer to git, a bare link included: write the link into the" >&2
    echo "sentence above it instead." >&2
    exit 1
fi

echo "commit messages: $count commit(s) in $range, no trailer lines"
