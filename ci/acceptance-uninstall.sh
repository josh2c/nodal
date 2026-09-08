#!/usr/bin/env sh
# Acceptance test for T0.10: uninstall and upgrade.
#
# Four claims:
#   - install then uninstall leaves a shell start-up file byte-identical. Five start-up
#     files are used and they are the awkward ones: one that ends with a newline, one
#     that does not, an empty one, one that does not exist yet, and one a person added
#     lines to after the install;
#   - a registry a version 1 Nodal wrote upgrades to the current schema with its rows
#     intact. The fixture is a real version 1 file, built by running the first migration
#     as it is committed and writing rows through SQL, so a writer that changed shape
#     since cannot make the check pass by accident;
#   - `nodal upgrade` and `nodal update` answer identically, and each names the channel
#     that installed the copy it is run from: the binary is copied into a cargo bin
#     directory and into a plain one, and each is asked;
#   - no code path in either crate can reach a network, and the shell hook evaluates
#     only what Nodal's own binary printed. Both are read out of the source and out of
#     the emitted script, in the way `done`'s no-pull-request check reads them.
#
# The suites are then run a second time with the temporary directory reached through a
# symbolic link, as `ci/acceptance-doctor.sh` does and for the same reason: an uninstall
# compares paths a shell wrote with paths a registry holds, and macOS gives that
# condition for free while Linux does not.
set -eu

cargo test --locked -p nodal-core --lib setup::
cargo test --locked -p nodal-core --lib output::view::setup
cargo test --locked -p nodal-core --test upgrade
cargo test --locked -p nodal-cli --test uninstall
cargo test --locked -p nodal-safety --test no_network

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/real"
ln -s "$work/real" "$work/by-another-name"
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-core --test upgrade
TMPDIR="$work/by-another-name" cargo test --locked -p nodal-cli --test uninstall

echo "acceptance (uninstall): a start-up file comes back byte for byte, a v1 registry upgrades, and neither verb fetches anything"
