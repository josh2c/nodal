#!/usr/bin/env sh
# Check that the public tree carries no identifier only a private coordination tree can
# decode.
#
# A reader of this repository must be able to follow every reference in it. A decision
# number, a finding code, a lane name, a role name and an absolute path under a real
# account's home all point outside the tree: the reader sees a label, cannot open what the
# label names, and the sentence loses the fact it stood for. So each one is written as the
# thing it names, and this check holds that state.
#
# The rules, and why each one is here:
#
#   decision number   `DL-0NN` names a record in a private decision log. Write the premise
#                     or the ruling instead.
#   finding code      `FS-N` names a row of a private investigation. Write the shape the
#                     row was: "the blobless clone", not the code.
#   lane name         `lane A` names a slot in a private sprint plan, and the plan is gone
#                     the day the sprint ends.
#   role name         `founder` names a role inside one company. This repository has
#                     contributors and maintainers.
#   private document  a path under `docs/research/boundary-<date>/` is a draft that stays
#                     in the private tree, so a citation of it cannot be followed here.
#   real home path    an absolute path under a real account's home is one machine's
#                     layout. A fixture path stands under one of the invented accounts
#                     this check allows; any other account is the machine a commit was
#                     made on, leaked into the tree.
#
# Two things this check does deliberately.
#
# **It reads the tracked tree and not the working directory.** A unit home is a checkout
# with Nodal's own files beside it, and `WORKUNIT.md`, `.nodal/env` and
# `.nodal/manifest.toml` each record the absolute home of the operator who made the unit.
# Git excludes those files and they are not the public tree. A check that walked the
# directory would fail in every unit home and pass only in a plain clone, which is a check
# nobody could run where the work is done.
#
# **It excludes itself and its acceptance test.** Both files hold the forbidden patterns
# as text, because one states them and the other plants one of each to prove this check
# names it.
set -eu

cd "$(git rev-parse --show-toplevel)"

# The two files that hold these patterns as their subject rather than in prose.
EXCLUDED='^ci/(public-identifiers|acceptance-public-identifiers)\.sh$'

# The invented accounts a fixture path may stand under, and the one system path that is a
# real location rather than a person's home.
FIXTURE_ACCOUNTS='(dev|j|u|you|linuxbrew)'

found=''

# Add every match of one rule to `found`, as `file:line:identifier (rule)`.
#
# $1 the rule's name, printed with each hit. $2 an extended regular expression. $3 an
# extended regular expression a hit must not match, or an empty string for none; it is
# applied to `file:line:identifier`, so it anchors on the identifier at the end. $4 is
# `-w` where the identifier is a whole word, and empty where it is not.
#
# `-w` and not `\b`: the word boundary of GNU grep is not in the expression syntax macOS
# reads, and `-w` is. The two agree on every pattern here, `FS-145` included.
#
# The file list passes through newlines, so a tracked path that holds a newline would
# split. No path in this tree does, and a path that did could not be excluded by name
# either.
sweep() {
    hits=$(
        git ls-files -z \
            | tr '\0' '\n' \
            | grep -Ev "$EXCLUDED" \
            | tr '\n' '\0' \
            | xargs -0 grep -HnoEI ${4:+"$4"} -e "$2" \
            || true
    )
    [ -z "$3" ] || hits=$(printf '%s' "$hits" | grep -Ev -e "$3" || true)
    [ -n "$hits" ] || return 0
    found=$found$(printf '%s' "$hits" | sed "s/\$/ ($1)/")'
'
}

sweep 'decision number'  'DL-0[0-9][0-9]'                      '' ''
sweep 'finding code'     'FS-[0-9]+'                           '' -w
sweep 'lane name'        'lane [A-Z]'                          '' -w
sweep 'role name'        '[Ff]ounder'                          '' ''
sweep 'private document' 'boundary-2[0-9]{3}-[0-9]{2}-[0-9]{2}' '' ''
sweep 'real home path'   '/(home|Users)/[A-Za-z0-9_.-]+/'      ":/(home|Users)/$FIXTURE_ACCOUNTS/\$" ''

if [ -n "$found" ]; then
    printf '%s' "$found" | sed 's/^/public identifiers: /' >&2
    echo "public identifiers: the tracked tree holds an identifier a reader outside this project cannot follow; write the thing each one names" >&2
    exit 1
fi
echo "public identifiers: the tracked tree names no decision, finding, lane, role, private document or real home"
