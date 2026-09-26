#!/usr/bin/env sh
# Check that the commits a change adds carry no trailer lines, and that every commit
# names only people this project knows in its author and committer fields.
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
# Usage: ci/commit-messages.sh [--no-merges] <range>
#
# The range is given, never inferred from the shape of HEAD. The workflow passes
# `base..head` on a pull request and `before..after` on a push, so the check reads the
# commits the event added and nothing else. GitHub's synthetic merge commit is never
# inside such a range.
#
# `--no-merges` hands the flag of that name to `git rev-list`, and the push step uses
# it. Every merge that reaches `main` is one GitHub made, and its body is the pull
# request title: one line, which git reads as a trailer whenever the title holds a
# colon. That is a person's title and no reason to fail a push. A pull request is read
# without the flag, so a merge a person made on the branch is read like any other
# commit, which is where a trailer in a merge body is caught.
#
# One rule stands over the people a commit names rather than over its body. Those fields
# are the only claim this project keeps about who wrote a commit, which is the reason there
# are no trailer lines: they have to be true. A unit home holds no identity of its own, so
# a home the project's identity was never set in writes its commits under the machine's
# global one, and a merge into this repository keeps every field of every commit it brings.
# So each commit in the range is read, and one this project does not know fails the check
# and is named with the field and the identity it carries.
#
# Both the author and the committer are read, because the two move apart. The author is who
# wrote the change and a rebase keeps it; the committer is who wrote the commit down last,
# and a rebase, an amend and a cherry-pick each replace it with whoever ran them. So work
# written in a home with the identity set and rebased in one without it arrives with every
# author right and every committer the machine's fallback, and reading the author alone let
# that branch through. That is the hole this closes.
#
# Two identities are this project's and both are the same person: the account the work is
# pushed under, which a unit home commits as, and the one the project's own checkout is
# configured with. A commit under either, in either field, is a commit a person wrote.
#
# A merge is read for neither field. Every merge that reaches this repository is one GitHub
# made: its author is the account that pressed the button, and its committer is GitHub.
#
# The left side of the range can be unreadable through no fault of a message. A first
# push and a force-push both leave `github.event.before` all zeros or naming a commit
# this clone does not hold. That is nothing to fail a person for, so the check says the
# range cannot be read and exits 0 on a push. Anywhere else an unreadable range is a
# broken call, and it exits 2.
set -eu

merges=
if [ "${1:-}" = --no-merges ]; then
    merges=--no-merges
    shift
fi

range=${1:-}
case "$range" in
    *..*) ;;
    *)
        echo "usage: ci/commit-messages.sh [--no-merges] <range>" >&2
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

# The identities a commit of this project may carry, one per line and stated nowhere else.
IDENTITIES='j2c <113136101+josh2c@users.noreply.github.com>
Josh Garcia <garciajosh313@gmail.com>'

# Whether this is one of them, read whole: a name alone and an address alone each pass a
# commit the pair would fail.
known_identity() {
    printf '%s\n' "$IDENTITIES" | grep -Fxq "$1"
}

# The fields of one commit that name somebody this project does not know, as
# `<field>: <name> <address>` lines, and nothing for a commit whose fields are all known.
#
# A merge names nobody this check reads, so it answers nothing for one.
strange_fields() {
    if is_merge "$1"; then
        return 0
    fi
    for field in author committer; do
        case "$field" in
            author) who=$(git log -1 --format='%an <%ae>' "$1") ;;
            *) who=$(git log -1 --format='%cn <%ce>' "$1") ;;
        esac
        known_identity "$who" || echo "$field: $who"
    done
}

# Whether the commit has a second parent, which is what makes it a merge.
is_merge() {
    case "$(git log -1 --format='%P' "$1")" in
        *' '*) return 0 ;;
        *) return 1 ;;
    esac
}

bad_lines() {
    trailers=$(git log -1 --format='%(trailers:only,unfold)' "$1")
    appended=$(git log -1 --format='%b' "$1" | grep -iE "$harness" || true)
    printf '%s\n%s\n' "$trailers" "$appended" |
        grep -v '^[[:blank:]]*$' | awk '!seen[$0]++'
}

count=0
failures=0
strangers=0
for sha in $(git rev-list $merges "$range"); do
    count=$((count + 1))
    strange=$(strange_fields "$sha")
    if [ -n "$strange" ]; then
        strangers=$((strangers + 1))
        {
            echo "commit-messages: $sha $(git log -1 --format='%s' "$sha")"
            printf '%s\n' "$strange" | sed 's/^/    /'
        } >&2
    fi
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

if [ "$strangers" -ne 0 ]; then
    echo "commit-messages: $strangers commit(s) name somebody this project does not know." >&2
    echo "A commit is authored by the person who wrote it and committed by the person who" >&2
    echo "wrote it down, under one of the identities this project uses. A unit home the" >&2
    echo "project's identity was never set in writes the machine's global one instead, which" >&2
    echo "is how another name gets into either field: the author when the commit was made" >&2
    echo "there, the committer when it was rebased, amended or cherry-picked there. Set the" >&2
    echo "identity in the home, then write the named field again over the whole branch. An" >&2
    echo "amend writes the committer and keeps the author:" >&2
    echo "    git rebase --exec 'git commit --amend --no-edit' <base>" >&2
    echo "Add --reset-author to that amend where the field named above is the author." >&2
    exit 1
fi

known='every author and committer known'
if [ -n "$merges" ]; then
    echo "commit messages: $count commit(s) in $range, merges not read, no trailer lines, $known"
else
    echo "commit messages: $count commit(s) in $range, no trailer lines, $known"
fi
