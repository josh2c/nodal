//! Acceptance test for T0.2, second half: `schemas/` on disk is what the model
//! generates.
//!
//! CI runs `ci/schema-diff.sh`, which regenerates the directory and fails on a diff.
//! This test is the same check without writing anything, so a model change that was
//! not exported fails at `cargo test` too, before it reaches CI.

#![allow(clippy::expect_used)]

use std::path::PathBuf;

use nodal_core::model::schema;

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
