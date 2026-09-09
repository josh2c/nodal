#!/usr/bin/env sh
# Check that the commits a change adds carry no trailer lines.
#
# A commit message in this project is prose. The person who wrote the commit is its
# author, and the author field already says so. A trailer line adds a second, weaker
# claim about who or what wrote the change, and every tool that appends one appends it
# to the same place, so the same rule catches all of them.
#
# A trailer line is a line of the form `Key:` and a value, where the key is letters,
# digits and hyphens, and any number of blanks can stand between the colon and the
# value. Git reads such lines only in the last paragraph of the body, and only when
# every line of that paragraph is a trailer line or an indented continuation of one.
# This check reads them the same way, so a prose paragraph that holds one `Word: text`
# line passes. Two kinds of key fail anywhere in the message, in prose or not: a key
# that ends in `-by` or `-session`, and `generated-with`. The comparison ignores case.
#
# A merge commit is exempt when its shape is the shape GitHub makes: two parents and a
# body of one line or none. GitHub writes the pull request title into that line, and a
# person wrote the title. A merge a person makes locally has a body of its own and is
# read like any other commit.
#
# Usage: ci/commit-messages.sh <base>
# The base is the commit the change branches from. When HEAD is a merge with two
# parents, which is the ref a pull request build checks out, the check reads
# `HEAD^1..HEAD^2` instead and the base is not used. This keeps a pull request from
# reading commits that landed on the base branch after the build started.
set -eu

base=${1:-}
if [ -z "$base" ]; then
    echo "usage: ci/commit-messages.sh <base>" >&2
    exit 2
fi

if git rev-parse --verify --quiet HEAD^2 > /dev/null; then
    range='HEAD^1..HEAD^2'
else
    if ! git rev-parse --verify --quiet "$base^{commit}" > /dev/null; then
        echo "commit-messages: $base does not name a commit in this repository" >&2
        exit 2
    fi
    range="$base..HEAD"
fi

# Read one commit: parents on the first line, then the message. Print the lines that
# break the rule and exit 1, or print nothing and exit 0.
read_commit() {
    awk -v sha="$1" '
        function blank(s)     { return s ~ /^[ \t]*$/ }
        function trailer(s)   { return s ~ /^[A-Za-z0-9][A-Za-z0-9-]*:[ \t]*[^ \t]/ }
        function indented(s)  { return s ~ /^[ \t]+[^ \t]/ }
        function key(s,   k)  { k = s; sub(/:.*$/, "", k); return tolower(k) }
        function banned(k)    { return k ~ /-by$/ || k ~ /-session$/ || k == "generated-with" }
        NR == 1 { parents = NF; next }
        { line[++n] = $0 }
        END {
            for (i = 1; i <= n; i++)
                if (blank(line[i])) { body = i + 1; break }
            for (i = n; i >= 1; i--)
                if (!blank(line[i])) { last = i; break }

            # A merge GitHub made: two parents and a body of one line or none.
            if (parents > 1) {
                written = 0
                for (i = body; body && i <= n; i++)
                    if (!blank(line[i])) written++
                if (written <= 1) exit 0
            }

            # The last paragraph is a trailer block when every line of it is a trailer
            # line or an indented continuation under one.
            if (body && last >= body) {
                first = last
                while (first > body && !blank(line[first - 1])) first--
                block = trailer(line[first])
                for (i = first + 1; block && i <= last; i++)
                    if (!trailer(line[i]) && !indented(line[i])) block = 0
                if (block)
                    for (i = first; i <= last; i++)
                        if (trailer(line[i])) bad[line[i]] = 1
            }

            # These keys fail wherever they stand.
            for (i = 1; i <= n; i++)
                if (trailer(line[i]) && banned(key(line[i]))) bad[line[i]] = 1

            found = 0
            for (i = 1; i <= n; i++)
                if (line[i] in bad && !seen[line[i]]++) {
                    if (!found++) print "commit-messages: " sha " " line[1]
                    print "    trailer line: " line[i]
                }
            exit found ? 1 : 0
        }
    '
}

count=$(git rev-list --count "$range")
failures=0
for sha in $(git rev-list "$range"); do
    if ! git log -1 --format='%P%n%B' "$sha" | read_commit "$sha" >&2; then
        failures=$((failures + 1))
    fi
done

if [ "$failures" -ne 0 ]; then
    echo "commit-messages: $failures commit message(s) hold a trailer line." >&2
    echo "A commit message is prose. The commit author is the author. Remove the line" >&2
    echo "and rewrite the history of the branch." >&2
    exit 1
fi

echo "commit messages: $count commit(s) in $range, no trailer lines"
