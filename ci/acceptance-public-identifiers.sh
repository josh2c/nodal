#!/usr/bin/env sh
# Acceptance test for the public-identifier check.
#
# The suite builds a fixture repository in a temporary directory, plants text in it, and
# runs `ci/public-identifiers.sh` there. The claims:
#   - a tracked file holding one of each forbidden identifier fails, and the output names
#     the file, the line and the identifier for every rule the check states;
#   - a tracked file whose home paths stand under the invented fixture accounts passes,
#     because those are the paths the repository writes its examples with;
#   - an untracked file holding a real account's home passes, because a unit home keeps
#     Nodal's own files beside the checkout and each one records that path.
#
# The identifiers are planted one per line and each is read back out of the failure
# output, so a rule that stopped matching is a failed claim and not a quiet pass.
set -eu

script=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/public-identifiers.sh

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1

git init --quiet --initial-branch=main .
git config user.name fixture
git config user.email fixture@example.com
git config commit.gpgsign false

fail() {
    echo "acceptance (public identifiers): $1" >&2
    shift
    [ $# -eq 0 ] || printf '%s\n' "$@" >&2
    exit 1
}

# Run the check inside the fixture and hold both output and exit status.
run() {
    if output=$("$script" 2>&1); then status=0; else status=$?; fi
}

# One planted identifier, and the rule the check must name it under.
plant() {
    echo "$1" >> planted.md
    git add planted.md
}

# ---------------------------------------------------------------------------
# One of each identifier fails, and each one is named.
# ---------------------------------------------------------------------------

plant 'DL-069 ruled that the copy a trashed home rested on can go.'
plant 'The shape FS-14 was is a blobless clone.'
plant 'This landed in lane C of the sprint.'
plant "The founder's ruling settles it."
plant 'See docs/research/boundary-2026-09-22/safety-contract-draft.md for the clause.'
plant 'The home was /home/realaccount/.nodal/nodal/e/HRQNQD7B when this ran.'

run
[ "$status" -eq 1 ] || fail "a tracked file holding six identifiers exited $status" "$output"

# Each rule, with the identifier it must name and the line it stands on.
check_named() {
    rule=$1
    identifier=$2
    line=$3
    printf '%s\n' "$output" | grep -Fq "planted.md:$line:$identifier ($rule)" \
        || fail "the check did not name $identifier on line $line as a $rule" "$output"
}

check_named 'decision number'  'DL-069'              1
check_named 'finding code'     'FS-14'               2
check_named 'lane name'        'lane C'              3
check_named 'role name'        'founder'             4
check_named 'private document' 'boundary-2026-09-22' 5
check_named 'real home path'   '/home/realaccount/'  6

# ---------------------------------------------------------------------------
# A fixture account's home path is prose the repository writes on purpose.
# ---------------------------------------------------------------------------

rm planted.md
cat > allowed.md <<'EOF'
A project at /home/dev/code/acme, a worktree at /home/j/code/app, a store at
/home/u/unit, a registry at /home/you/.nodal/registry.db, and Homebrew's own
/home/linuxbrew/.linuxbrew. On macOS the same example is /Users/dev/code/acme.
EOF
git add -A
run
[ "$status" -eq 0 ] || fail "a fixture home path was read as a leak" "$output"

# ---------------------------------------------------------------------------
# The check reads the tracked tree, so a unit home's own files never fail it.
# ---------------------------------------------------------------------------

git commit --quiet -m "Hold the paths this project writes its examples with"
echo 'home = "/home/realaccount/.nodal/nodal/e/HRQNQD7B"' > WORKUNIT.md
run
[ "$status" -eq 0 ] || fail "an untracked file was read as part of the public tree" "$output"

echo "acceptance (public identifiers): every rule names its identifier, fixture paths pass, and an untracked file is not the public tree"
