#!/usr/bin/env sh
# Acceptance test for T0.1: a release artifact reports the workspace version.
# Usage: ci/acceptance-version.sh <path-to-nodal> [expected-version]
set -eu

bin=${1:?usage: ci/acceptance-version.sh <path-to-nodal> [expected-version]}
expected=${2:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)}

actual=$("$bin" --version)
if [ "$actual" != "nodal $expected" ]; then
	echo "acceptance: expected 'nodal $expected', got '$actual'" >&2
	exit 1
fi
echo "acceptance: $actual"
