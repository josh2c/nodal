//! The JSON schema catalogue: one document per record type, versioned as a set.
//!
//! The schemas in `schemas/` are generated from these types and committed, and CI
//! regenerates them and fails on a diff. That is what makes the model a contract: a
//! field cannot change shape without the change showing up in review as a schema diff.
//!
//! Versioning is by directory, not per file. [`SCHEMA_VERSION`] is bumped when a change
//! would break a reader of the previous set, and the previous directory stays where it
//! is so old bundles remain readable.

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::Value;

use crate::model::{
    Base, DbTemplate, Environment, Event, Lease, Lock, Project, Recipe, Session, Unit,
};

/// The version of the schema set. Bumped only for a breaking change.
pub const SCHEMA_VERSION: u32 = 1;

/// Where the published schemas live. `$id` values are built from this.
pub const SCHEMA_BASE_URL: &str = "https://github.com/josh2c/nodal/blob/main/schemas";

/// One generated schema document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDoc {
    /// File stem, in snake case: `db_template` for [`DbTemplate`].
    pub name: &'static str,
    /// The Rust type this was generated from, so a test can tie the catalogue back to
    /// the model rather than to a hand-kept list of names.
    pub type_name: String,
    /// The schema itself, draft 2020-12.
    pub schema: Value,
}

impl SchemaDoc {
    /// The file this document is written to, relative to `schemas/`.
    #[must_use]
    pub fn path(&self) -> String {
        format!("v{SCHEMA_VERSION}/{}.json", self.name)
    }

    /// The exact bytes that belong in that file: pretty JSON with a trailing newline,
    /// so a schema diff is a readable diff.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text =
            serde_json::to_string_pretty(&self.schema).unwrap_or_else(|_| String::from("{}"));
        text.push('\n');
        text
    }
}

/// Generate the schema for one type, with the identity keywords filled in.
fn document<T: JsonSchema>(name: &'static str) -> SchemaDoc {
    let mut schema = SchemaSettings::draft2020_12().into_generator().into_root_schema_for::<T>();
    schema.insert(
        String::from("$id"),
        Value::String(format!("{SCHEMA_BASE_URL}/v{SCHEMA_VERSION}/{name}.json")),
    );
    SchemaDoc { name, type_name: T::schema_name().into_owned(), schema: schema.to_value() }
}

/// Every type the schema set publishes, in the order of the data model.
///
/// The list is the whole of the export: adding a type here is the only step needed to
/// publish it, and CI's diff then requires the generated file to be committed.
#[must_use]
pub fn catalog() -> Vec<SchemaDoc> {
    vec![
        document::<Project>("project"),
        document::<Base>("base"),
        document::<DbTemplate>("db_template"),
        document::<Unit>("unit"),
        document::<Environment>("environment"),
        document::<Session>("session"),
        document::<Event>("event"),
        document::<Lease>("lease"),
        document::<Lock>("lock"),
        document::<Recipe>("recipe"),
    ]
}

/// The index document for an already-generated catalogue.
#[must_use]
pub fn index(catalog: &[SchemaDoc]) -> SchemaDoc {
    let entries: Vec<Value> = catalog
        .iter()
        .map(|doc| {
            serde_json::json!({
                "name": doc.name,
                "type": doc.type_name,
                "file": format!("{}.json", doc.name),
                "$id": doc.schema.get("$id").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    SchemaDoc {
        name: "index",
        type_name: String::from("Index"),
        schema: serde_json::json!({
            "version": SCHEMA_VERSION,
            "generated_by": concat!("nodal-core ", env!("CARGO_PKG_VERSION")),
            "schemas": entries,
        }),
    }
}

/// Every document that belongs in `schemas/`, the index included. The catalogue is
/// generated once and the index is built from it.
#[must_use]
pub fn documents() -> Vec<SchemaDoc> {
    let mut documents = catalog();
    let index = index(&documents);
    documents.push(index);
    documents
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{SCHEMA_VERSION, catalog, documents, index};

    #[test]
    fn every_document_is_an_object_with_an_id() {
        for doc in catalog() {
            let object = doc.schema.as_object().expect("a schema is a JSON object");
            assert!(object.contains_key("$id"), "{} has no $id", doc.name);
            assert_eq!(
                object.get("$schema").and_then(serde_json::Value::as_str),
                Some("https://json-schema.org/draft/2020-12/schema"),
                "{} is not draft 2020-12",
                doc.name
            );
        }
    }

    #[test]
    fn paths_carry_the_version() {
        assert_eq!(index(&catalog()).path(), format!("v{SCHEMA_VERSION}/index.json"));
    }

    #[test]
    fn documents_are_the_catalogue_plus_the_index() {
        let documents = documents();
        assert_eq!(documents.len(), catalog().len() + 1);
        assert_eq!(documents.last().map(|doc| doc.name), Some("index"));
    }

    #[test]
    fn every_document_names_the_type_it_came_from() {
        for doc in catalog() {
            assert!(!doc.type_name.is_empty(), "{} has no type name", doc.name);
        }
    }

    #[test]
    fn rendering_ends_in_one_newline() {
        let rendered = index(&catalog()).render();
        assert!(rendered.ends_with("}\n"), "{rendered}");
    }
}
