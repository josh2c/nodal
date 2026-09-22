# Contributing

## Build and test

```
cargo build --workspace
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo deny check
```

CI runs the same gates. The `ci/` directory holds the acceptance scripts CI runs; each one also runs locally.

## Performance rules

The hot paths have a 5 ms cold-start budget. `ci/startup-budget.sh` measures the
release binary and fails above a threshold calibrated for the CI runner. Read the top of that
script before you change the threshold. The harness is `benches/startup`.

A clone is the other measured path. `ci/measure-materialize.sh` clones a large tree at
each worker count. It prints the curve and checks that every clone is the same tree. CI
does not run it. Run it yourself when you change `workspace/tree.rs`. Record the new
numbers in its header.

## Code rules

See `docs/code-structure.md`. The short version:

- The library (`nodal-core`) holds all behavior. The binary (`nodal-cli`) parses arguments and prints.
- Each lifecycle operation is a plan of idempotent steps. Each step has an undo.
- One module spawns each external process. No other module calls that tool directly.
- Clippy thresholds in `clippy.toml` are hard limits.

## Commit messages

A commit message is prose. Write what the change does and why. The commit author is the
author, and the author field says so. Do not add trailer lines: no `Co-Authored-By`, no
`Signed-off-by`, no tool or session links. `ci/commit-messages.sh` fails a pull request,
and a push to `main`, whose commits hold one. Its header comment states the exact rule,
including what git itself counts as a trailer.

A pull-request title holds no colon. GitHub's merge commit carries the title as its body,
and a last line with a colon reads as a trailer to git.

## Releases

A change that a person can see adds one line to `CHANGELOG.md`, under the section for the
version that is being prepared. Write the line in the same form as the lines beside it:
one behaviour, one sentence.

To make a release:

1. Set the version in `Cargo.toml` under `[workspace.package]`, and in the `nodal-core`
   requirement below it. Run `cargo check --workspace` to move `Cargo.lock`, and
   `ci/schema-diff.sh` to move `schemas/v1/index.json`.
2. Give `CHANGELOG.md` a section with that version. Update the version named by the
   README install commands, by `docs/contracts.md` and by `schemas/README.md`.
   `ci/acceptance-release.sh` checks that all of these agree, and that the registry
   schema those documents state is the one the code carries.
3. Merge the pull request.
4. Tag the merge commit `v<version>` and push the tag.

The tag runs the workflow. The `release` job builds `nodal-<version>-<target>` for each
platform and writes the sha256 beside it. The `publish` job creates the GitHub release
from the tag, attaches both files for each platform, and takes the release notes from the
section in `CHANGELOG.md`. It fails when the tag and the manifest name different versions.

A publish that fails after it made the release leaves a release with some files on it.
Delete that release by hand, then run the workflow again against the tag ref. The job
does not add to a release it finds; it makes one.

Every refusal `CHANGELOG.md` states must be a refusal some code prints. Name the file
and the test in the commit message that adds the line.

## Your workstation

This repository builds a large Rust workspace. One built checkout holds 5 to 45 GB of
`target/`. A machine that holds many checkouts holds that many times over. The rules
below keep the cost visible and bounded.

### Build output lives per unit, and goes with the unit

Each unit home keeps its own `target/`. Nodal does not share one `CARGO_TARGET_DIR`
between homes: Cargo build output records the path it was built at, and two units that
shared a directory would race and would rebuild each other's work.

A home Nodal made goes to the trash on `nodal reclaim`, and the trash does not keep its
build output. `nodal gc` then removes the trashed home after the retention.

A checkout you adopted with `nodal adopt --in-place` is your own directory. A reclaim
unregisters the unit and leaves the directory exactly as it is, build output included.
Read what it holds first, then remove it:

```
nodal reclaim <unit> --check        # names the paths and the bytes; removes nothing
nodal reclaim <unit> --prune        # removes the build output; keeps the directory
```

`--prune` removes a path only when an ignore rule covers it and the exclusion table
calls it regenerable. It never removes a tracked file, and it never removes the
directory.

A clone that is not a unit is a clone Nodal cannot answer for. `nodal doctor --machine
~/Projects` lists them with their ignored bytes.

### Exclude build output from the file indexer

Most systems index the file system each day. The index usually excludes `.git`, `.hg`
and `.svn`, and does not exclude build output. On a machine that holds several
checkouts of this repository the indexer then reads hundreds of thousands of files it
will never be asked about.

Exclude `target`, `node_modules` and `.cache` from your indexer. Where that is
`plocate`, the line is `PRUNENAMES` in `/etc/updatedb.conf`:

```
PRUNENAMES = ".git .hg .svn target node_modules .cache"
```

Other systems name the setting differently; the three directories are the same. Your
operating system owns that file. This repository does not change it.

### Scratch directories and their retention

The benchmark harness in `benches/harness/` makes one work directory per run and
removes it when the run exits, including a run that fails. A run that gets `SIGKILL`
leaves a directory whose name carries the script and the process identifier; the next
run removes it.

Harness results go to `benches/results/`, which is not committed. They are held for the
retention `nodal.toml` states under `[reclaim] trash_retention`. This project states
none, so runs use 14 days, the same number as Nodal's own default. Remove the older
ones:

```
benches/harness/create_bench.sh --gc
```

Every script prints its retention rule before its first measurement.

### The cost of a slow disk

A build-heavy repository on a slow disk blocks every other program on the machine.
`rustc` and `rust-lld` fill the disk queue, and everything else waits behind them. The
machine then looks faulty: load average climbs with no CPU demand and with memory free.
It is not faulty. It is waiting for the disk.

Check what a build costs you before you conclude otherwise. `nodal doctor --machine
~/Projects` prints what the checkouts hold.

## What belongs in the repository

Commit only what the project needs to build, test, and document itself. Do not commit:

- editor or AI-tool configuration (`.vscode/`, `.claude/`, `CLAUDE.md`, `AGENTS.md`)
- local environment files (`.env`, secrets, machine paths)
- generated output, logs, or scratch files
- large binary files

A PR adds the files its task requires and nothing else.

## Documentation rules

Write documentation to ASD-STE100 (Simplified Technical English) principles:

- Keep sentences short: no more than 20 words for an instruction, 25 for a description.
- Use the active voice. Name the actor.
- Give one instruction per sentence.
- Use one term for one thing. Do not alternate between synonyms.
- State facts. Do not use marketing language.
