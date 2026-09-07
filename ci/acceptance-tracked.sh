#!/usr/bin/env sh
# Acceptance test for T1.4b: base.exclude never drops a tracked path.
#
# The fixture project (tests/fixture) is made a repository that tracks `coverage`, a
# directory the exclusion table names and the fixture's own .gitignore ignores. Four
# claims follow from that one shape:
#   - inference proposes the heavy directory the project does not track, and not the
#     one it does;
#   - a copy refuses a list that holds the tracked path, and the message names it;
#   - the copy that refusal prevents is dirty at birth: `git status` in it reports a
#     deletion for every file under the dropped path;
#   - the rule covers the rows Nodal ships, not only the rows a project adds.
set -eu

cargo test --locked -p nodal-core --test tracked
cargo test --locked -p nodal-core --lib workspace::tracked
cargo test --locked -p nodal-core --lib recipe::infer
echo "acceptance (tracked): an exclusion list never drops a path the project tracks"
