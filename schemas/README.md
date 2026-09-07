# JSON schemas

Generated from the domain model in `crates/nodal-core/src/model`, committed here, and
regenerated and diffed by CI. They are the machine-readable form of the data model, so a
tool that reads a bundle, an `events.jsonl` line or `--json` output can validate it
without linking against Nodal.

- `v1/index.json` lists the set and its version.
- `v1/<type>.json` is one record type, JSON Schema draft 2020-12.

## Regenerating

```
ci/schema-diff.sh          # regenerate and fail if anything changed
cargo run -p nodal-core --example export-schemas
```

Every change to a model type must land with its regenerated schema in the same commit;
`cargo test` fails otherwise, and so does the `schemas` job in CI.

## Versioning

The set is versioned as a whole, by directory. `v1` is stable: a field may be added,
because a reader that ignores unknown fields keeps working, but removing a field,
renaming one, narrowing a pattern or changing a type is a breaking change and lands as a
new `v2/` directory with the previous one left in place, so older bundles stay readable.
The version lives in `SCHEMA_VERSION` in `crates/nodal-core/src/model/schema.rs`.
