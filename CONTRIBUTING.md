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

## Code rules

See `docs/code-structure.md`. The short version:

- The library (`nodal-core`) holds all behavior. The binary (`nodal-cli`) parses arguments and prints.
- Each lifecycle operation is a plan of idempotent steps. Each step has an undo.
- One module spawns each external process. No other module calls that tool directly.
- Clippy thresholds in `clippy.toml` are hard limits.

## Documentation rules

Write documentation to ASD-STE100 (Simplified Technical English) principles:

- Keep sentences short: no more than 20 words for an instruction, 25 for a description.
- Use the active voice. Name the actor.
- Give one instruction per sentence.
- Use one term for one thing. Do not alternate between synonyms.
- State facts. Do not use marketing language.
