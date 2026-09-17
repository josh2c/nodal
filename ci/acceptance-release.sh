#!/usr/bin/env sh
# Acceptance test: one version number, in every place a person reads it.
#
# The release job names its files after the manifest version, and the publish job cuts
# the release notes out of `CHANGELOG.md` by that same version. A changelog with no
# section for the version, or an install command that names another version, is found
# here and not on the tag.
#
# Usage: ci/acceptance-release.sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || { echo "acceptance: Cargo.toml states no version" >&2; exit 1; }
echo "acceptance: the manifest version is $version"

# The changelog section, cut the way the publish job cuts it.
notes=$(awk -v want="## $version" '
	index($0, want) == 1 { on = 1; next }
	on && /^## / { exit }
	on { print }
' CHANGELOG.md)
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
	echo "acceptance: CHANGELOG.md has no section for $version" >&2
	exit 1
fi
echo "acceptance: CHANGELOG.md holds a section for $version"

# Every version the README shows a person to type. Three shapes: the shell variable the
# download route sets, the file name it builds, and the tag the source route names.
named=$(
	{
		grep -o '^version=[0-9][0-9A-Za-z.+-]*' README.md | sed 's/^version=//'
		grep -o 'nodal-[0-9][0-9A-Za-z.+-]*-x86_64' README.md | sed 's/^nodal-//; s/-x86_64$//'
		grep -o -- '--tag v[0-9][0-9A-Za-z.+-]*' README.md | sed 's/^--tag v//'
	} | sort -u
)
[ -n "$named" ] || { echo "acceptance: README.md names no version" >&2; exit 1; }
for read_version in $named; do
	if [ "$read_version" != "$version" ]; then
		echo "acceptance: README.md names version $read_version, and the manifest says $version" >&2
		exit 1
	fi
done
echo "acceptance: README.md names $version and no other version"
