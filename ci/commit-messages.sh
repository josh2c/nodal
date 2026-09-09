#!/usr/bin/env sh
# Check that the commits a pull request adds carry no trailer lines.
#
# A commit message in this project is prose. The person who wrote the commit is its
# author, and the author field already says so. A trailer line adds a second, weaker
# claim about who or what wrote the change, and every tool that appends one appends it
# to the same place, so the same rule catches all of them.
#
# A trailer line here is a line of the form `Key: value`, where `Key` is one or more
# words joined by hyphens, in the last paragraph of the message body. The subject line
# is never a trailer, which is why the check starts after the first blank line. A
# message with no body has no last paragraph and so cannot fail.
#
# A merge commit GitHub made is exempt. Its body is the pull request title, which the
# person who opened the pull request wrote, and this check does not read titles.
#
# Usage: ci/commit-messages.sh <base>
# The base is the commit the pull request branches from. CI passes the base of the pull
# request. Locally, pass `origin/main` or the merge base you want to read from.
set -eu

base=${1:-}
if [ -z "$base" ]; then
    echo "usage: ci/commit-messages.sh <base>" >&2
    exit 2
fi

# Print every trailer line in the last paragraph of the body, one per line.
trailers() {
    awk '
        { line[NR] = $0 }
        function blank(s) { return s ~ /^[ \t]*$/ }
        END {
            for (i = 1; i <= NR; i++)
                if (blank(line[i])) { start = i + 1; break }
            if (!start) exit
            for (i = NR; i >= start; i--)
                if (!blank(line[i])) { last = i; break }
            if (!last) exit
            first = last
            while (first > start && !blank(line[first - 1])) first--
            for (i = first; i <= last; i++)
                if (line[i] ~ /^[A-Za-z][A-Za-z0-9]*(-[A-Za-z0-9]+)*: [^ ]/)
                    print line[i]
        }
    '
}

count=0
failures=0
for sha in $(git log --format=%H "$base..HEAD"); do
    count=$((count + 1))
    subject=$(git log -1 --format=%s "$sha")
    parents=$(git log -1 --format=%P "$sha" | wc -w)
    if [ "$parents" -gt 1 ]; then
        case "$subject" in
            "Merge pull request #"* | "Merge branch "*) continue ;;
        esac
    fi
    found=$(git log -1 --format=%B "$sha" | trailers)
    if [ -n "$found" ]; then
        echo "commit-messages: $sha $subject" >&2
        echo "$found" | sed 's/^/    trailer line: /' >&2
        failures=$((failures + 1))
    fi
done

if [ "$failures" -ne 0 ]; then
    echo "commit-messages: $failures commit message(s) hold a trailer line." >&2
    echo "A commit message is prose. The commit author is the author. Remove the line" >&2
    echo "and rewrite the history of the branch." >&2
    exit 1
fi

echo "commit messages: $count commit(s) between $base and HEAD, no trailer lines"
