#!/usr/bin/env sh
# Acceptance test: one version number, in every place a person reads it.
#
# The release job names its files after the manifest version, and the publish job cuts
# the release notes out of `CHANGELOG.md` by that same version. A changelog with no
# section for the version, an install command that names another version, or a document
# that states a version this build is not, is found here and not on the tag.
#
# The registry schema number is checked the same way. Two documents tell a person which
# registry schema this build reads, and the number they state must be the number the
# code carries.
#
# Usage: ci/acceptance-release.sh [ROOT]
#        ci/acceptance-release.sh --self-test
#
# ROOT is the tree to read, and defaults to the repository this script is in.
# `--self-test` copies the tree, moves the manifest version in the copy alone, and
# requires this script to fail against it.
set -eu

script=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/$(basename -- "$0")
repository=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

# Run this script against a copy whose manifest version nothing else agrees with, and
# fail when it passes. A gate nobody has seen fail is not known to be a gate.
self_test() {
	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT INT TERM
	for file in Cargo.toml CHANGELOG.md README.md docs/contracts.md schemas/README.md \
		crates/nodal-core/src/store/migrations.rs; do
		mkdir -p "$work/$(dirname "$file")"
		cp "$repository/$file" "$work/$file"
	done

	# The copy is the tree this script reads, so it must first pass unchanged. Without
	# this the test below could fail for any reason at all and still look like a pass.
	if ! "$script" "$work" > /dev/null 2>&1; then
		echo "acceptance: the self-test copy does not pass before it is changed" >&2
		"$script" "$work" >&2 || true
		exit 1
	fi

	sed -i.bak '0,/^version = ".*"$/s//version = "0.0.0-selftest.1"/' "$work/Cargo.toml"
	rm -f "$work/Cargo.toml.bak"
	if "$script" "$work" > /dev/null 2>&1; then
		echo "acceptance: the self-test passed a tree whose versions disagree" >&2
		exit 1
	fi
	echo "acceptance: the check passes this tree and fails one whose versions disagree"
}

if [ "${1:-}" = --self-test ]; then
	self_test
	exit 0
fi

root=${1:-$repository}
cd "$root"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || { echo "acceptance: Cargo.toml states no version" >&2; exit 1; }
echo "acceptance: the manifest version is $version"

# The changelog section, cut the way the publish job cuts it. The heading must be the
# whole version: `## 0.1.0-rc.1` is not the section of `0.1.0-rc.10`.
notes=$(awk -v want="## $version" '
	index($0, want) == 1 && substr($0, length(want) + 1, 1) ~ /^( |)$/ { on = 1; next }
	on && /^## / { exit }
	on { print }
' CHANGELOG.md)
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
	echo "acceptance: CHANGELOG.md has no section for $version" >&2
	exit 1
fi
echo "acceptance: CHANGELOG.md holds a section for $version"

# Every version a document states. Four shapes: the shell variable the download route
# sets, the file name it builds, the tag the source route names, and the sentence each
# of the two documents opens with.
named=$(
	{
		grep -o '^version=[0-9][0-9A-Za-z.+-]*' README.md | sed 's/^version=//'
		grep -o 'nodal-[0-9][0-9A-Za-z.+-]*-x86_64' README.md | sed 's/^nodal-//; s/-x86_64$//'
		grep -o -- '--tag v[0-9][0-9A-Za-z.+-]*' README.md | sed 's/^--tag v//'
		grep -o 'Version [0-9][0-9A-Za-z.+-]* reads registry schema' docs/contracts.md \
			| sed 's/^Version //; s/ reads registry schema$//'
		grep -o 'This release, `[0-9][0-9A-Za-z.+-]*`' schemas/README.md \
			| sed 's/^This release, `//; s/`$//'
	} | sort -u
)
[ -n "$named" ] || { echo "acceptance: no document names a version" >&2; exit 1; }
for read_version in $named; do
	if [ "$read_version" != "$version" ]; then
		echo "acceptance: a document names version $read_version, and the manifest says $version" >&2
		exit 1
	fi
done
echo "acceptance: README.md, docs/contracts.md and schemas/README.md name $version and no other version"

# The registry schema number the two documents state, against the number the code
# carries. A document that names the wrong schema tells a person the wrong thing about
# what their registry is compatible with.
schema=$(
	sed -n 's/^pub const SCHEMA_VERSION: u32 = \([0-9]*\);$/\1/p' \
		crates/nodal-core/src/store/migrations.rs 2>/dev/null | head -n 1
)
if [ -n "$schema" ]; then
	for stated in $(
		{
			grep -o 'reads registry schema [0-9][0-9]*' docs/contracts.md
			grep -o 'reads registry schema [0-9][0-9]*' schemas/README.md
			grep -o 'reads registry schema [0-9][0-9]*' CHANGELOG.md
		} | sed 's/^reads registry schema //' | sort -u
	); do
		if [ "$stated" != "$schema" ]; then
			echo "acceptance: a document says registry schema $stated, and the code says $schema" >&2
			exit 1
		fi
	done
	echo "acceptance: every document that names the registry schema says $schema"
else
	echo "acceptance: the registry schema number was not read, so it was not checked" >&2
	exit 1
fi
