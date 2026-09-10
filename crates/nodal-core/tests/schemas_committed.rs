//! Acceptance test, second of the two schema halves: `schemas/` on disk is what the
//! model generates.
//!
//! CI runs `ci/schema-diff.sh`, which regenerates the directory and fails on a diff.
//! This test is the same check without writing anything, so a model change that was
//! not exported fails at `cargo test` too, before it reaches CI.

#![allow(clippy::expect_used)]

use std::path::PathBuf;

use nodal_core::model::schema;

/// Types that are part of another record rather than a record of their own, and so
/// are published inside the schemas that embed them instead of as files. Anything not
/// listed here and not in the catalogue fails the test below, which is what stops a new
/// model type from shipping without a schema.
const EMBEDDED_TYPES: &[&str] = &[
    "Actor",
    "SubFp",
    "SchemaDoc",
    // The sections of a recipe. `nodal.toml` is one document, so `Recipe` is the record
    // and every section is published inside it.
    "BaseSpec",
    "Commands",
    "Db",
    "Env",
    "Hooks",
    "Reclaim",
    "Services",
    "Sync",
    // A line of a manifest's missing-name report, published inside `Manifest`.
    "Missing",
];

fn model_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/model")
}

/// Every `pub struct X {` declared in the model, by name. Tuple structs are the
/// validated scalars; they are published inside the records that use them.
fn declared_record_types() -> Vec<String> {
    let mut names = Vec::new();
    let entries = std::fs::read_dir(model_dir()).expect("the model directory exists");
    for entry in entries {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for line in source.lines() {
            if let Some(rest) = line.strip_prefix("pub struct ")
                && let Some(name) = rest.strip_suffix(" {")
            {
                names.push(name.to_owned());
            }
        }
    }
    names.sort_unstable();
    names
}

fn schemas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

#[test]
fn committed_schemas_match_the_model() {
    let root = schemas_dir();
    for document in schema::documents() {
        let path = root.join(document.path());
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("{}: {error}\nrun ci/schema-diff.sh and commit the result", path.display())
        });
        assert_eq!(
            committed,
            document.render(),
            "{} is out of date; run ci/schema-diff.sh and commit the result",
            path.display()
        );
    }
}

#[test]
fn the_catalogue_covers_every_record_type_the_model_declares() {
    let published: Vec<String> = schema::catalog().into_iter().map(|doc| doc.type_name).collect();
    let declared = declared_record_types();
    assert!(!declared.is_empty(), "the source scan found no record types at all");
    for name in &declared {
        assert!(
            published.contains(name) || EMBEDDED_TYPES.contains(&name.as_str()),
            "{name} is a model record with no schema: add it to catalog(), or to \
             EMBEDDED_TYPES if it is only ever part of another record"
        );
    }
    for name in &published {
        assert!(
            declared.contains(name),
            "{name} is in the catalogue but is not a record type in src/model"
        );
    }
}

#[test]
fn no_stale_schema_files_are_left_behind() {
    let root = schemas_dir().join(format!("v{}", schema::SCHEMA_VERSION));
    let expected: Vec<String> =
        schema::documents().iter().map(|doc| format!("{}.json", doc.name)).collect();
    let entries = std::fs::read_dir(&root).expect("the schema directory exists");
    for entry in entries {
        let name = entry.expect("a readable directory entry").file_name();
        let name = name.to_string_lossy().into_owned();
        assert!(
            expected.contains(&name),
            "{name} is in schemas/ but not in the catalogue; delete it or add its type"
        );
    }
}
