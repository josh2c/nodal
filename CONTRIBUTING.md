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

## Code rules

See `docs/code-structure.md`. The short version:

- The library (`nodal-core`) holds all behavior. The binary (`nodal-cli`) parses arguments and prints.
- Each lifecycle operation is a plan of idempotent steps. Each step has an undo.
- One module spawns each external process. No other module calls that tool directly.
- Clippy thresholds in `clippy.toml` are hard limits.

## Commit messages

A commit message is prose. Write what the change does and why. The commit author is the
author, and the author field says so. Do not add trailer lines: no `Co-Authored-By`, no
`Signed-off-by`, no tool or session links. `ci/commit-messages.sh` reads every commit a
pull request adds and fails on a `Key: value` line in the last paragraph of a message.

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
