//! Two keys over a tree: the workspace fingerprint a base is stored under and the
//! schema fingerprint a database template is stored under.
//!
//! They are separate because they move at very different rates. On a project with
//! migrations, the schema moves several times a week while dependencies move a few
//! times a month and service definitions barely at all. One combined key would rebuild
//! an identical workspace every time a migration landed; two keys rebuild the workspace
//! only when the workspace moved, and build templates incrementally from their nearest
//! older neighbour.
//!
//! Both keys are built from `(path, mode, object id)` triples at a commit, so an
//! ordinary commit that touches only source moves neither, and the cost of taking a
//! fingerprint is one `git ls-tree`.
//!
//! Each key also reports its parts, so a stale environment can say *what* moved rather
//! than only *that* it moved. That is what `nodal sync` plans against.

mod digest;
pub mod inputs;
pub mod platform;
pub mod tree;

use std::path::PathBuf;

use self::digest::Hasher;
pub use self::inputs::{CLASSES, Class, Key, Selector};
pub use self::platform::current as current_platform;
pub use self::tree::{GitTreeAtCommit, TreeSource};
use crate::git::tree::Entry;
use crate::model::{Digest, FingerprintPart, Platform, Recipe, SchemaFp, SubFp, WorkspaceFp};
use crate::{Error, Result};

/// The workspace key at a commit, with the parts it was composed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// The key a base is stored under.
    pub key: WorkspaceFp,
    /// The platform the key was taken for; a base is warm for this one only.
    pub platform: Platform,
    /// One entry per workspace class, in table order.
    pub parts: Vec<SubFp>,
}

/// The schema key at a commit, with the parts it was composed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    /// The key a database template is stored under.
    pub key: SchemaFp,
    /// One entry per schema class, in table order.
    pub parts: Vec<SubFp>,
}

/// Both keys of one tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprints {
    /// The workspace key.
    pub workspace: Workspace,
    /// The schema key.
    pub schema: Schema,
}

impl Fingerprints {
    /// Every part of both keys, in table order: what a sync diff is taken over.
    #[must_use]
    pub fn parts(&self) -> Vec<SubFp> {
        let mut parts = self.workspace.parts.clone();
        parts.extend(self.schema.parts.iter().cloned());
        parts
    }
}

/// Both keys from one listing of `source`.
///
/// This is the entry point: it reads the tree once and composes both keys from it,
/// rather than listing the tree twice.
///
/// # Errors
/// Whatever `source` failed to read with.
pub fn compute(source: &impl TreeSource, platform: &Platform) -> Result<Fingerprints> {
    let selected = select(&source.entries()?);
    Ok(Fingerprints {
        workspace: compute_workspace(&selected, platform)?,
        schema: compute_schema(&selected)?,
    })
}

/// The workspace key of an already-selected tree.
///
/// # Errors
/// As [`compute`].
pub fn compute_workspace(selected: &Selected, platform: &Platform) -> Result<Workspace> {
    let parts = selected.parts(Key::Workspace)?;
    let mut hasher = Hasher::new(digest::WORKSPACE_DOMAIN);
    hasher.text(platform.as_str());
    fold(&mut hasher, &parts);
    Ok(Workspace { key: WorkspaceFp(hasher.finish()?), platform: platform.clone(), parts })
}

/// The schema key of an already-selected tree. The platform is deliberately absent: a
/// database template is a database, and it is the same database on any host.
///
/// # Errors
/// As [`compute`].
pub fn compute_schema(selected: &Selected) -> Result<Schema> {
    let parts = selected.parts(Key::Schema)?;
    let mut hasher = Hasher::new(digest::SCHEMA_DOMAIN);
    fold(&mut hasher, &parts);
    Ok(Schema { key: SchemaFp(hasher.finish()?), parts })
}

/// The digest of an effective recipe: what `project.recipe_hash` records.
///
/// It is not one of the two keys. A recipe changing does not make a base cold — the
/// keys above already cover every input a base is built from — but it does mean the
/// project a unit was created under is no longer the project the registry recorded, and
/// this is the one value that says so without re-reading `nodal.toml`.
///
/// The recipe is hashed in its JSON form, which is stable: every map in it is ordered,
/// and every field has a fixed place.
///
/// # Errors
/// [`Error::Render`] when the recipe cannot be encoded.
pub fn compute_recipe(recipe: &Recipe) -> Result<Digest> {
    let json =
        serde_json::to_string(recipe).map_err(|source| Error::Render { kind: "recipe", source })?;
    let mut hasher = Hasher::new(digest::RECIPE_DOMAIN);
    hasher.text(&json);
    hasher.finish()
}

/// The digest of one command line: what an approval of a hook is recorded as.
///
/// A hook is approved by its exact text, so a command that gains an argument, a pipe or
/// a second word is a different command and is refused until it is approved again. The
/// digest rather than the text is stored because the text is a line from a project's
/// recipe, and the file that records approvals belongs to the machine rather than to
/// any one project.
///
/// # Errors
/// [`Error::InvalidValue`] only if the hex encoding stopped being hex.
pub fn compute_command(command: &str) -> Result<Digest> {
    let mut hasher = Hasher::new(digest::COMMAND_DOMAIN);
    hasher.text(command);
    hasher.finish()
}

/// Fold named sub-fingerprints into a key, naming each part so that two classes
/// swapping their inputs cannot produce the same key.
fn fold(hasher: &mut Hasher, parts: &[SubFp]) {
    for part in parts {
        hasher.text(part_name(part.part));
        hasher.text(part.digest.as_str());
    }
}

/// The stable name of a part inside a key. Taken from the serialised form, so the
/// name in a key and the name in a report are one string.
fn part_name(part: FingerprintPart) -> &'static str {
    match part {
        FingerprintPart::Toolchain => "toolchain",
        FingerprintPart::Dependencies => "dependencies",
        FingerprintPart::Schema => "schema",
        FingerprintPart::Services => "services",
        FingerprintPart::Recipe => "recipe",
        FingerprintPart::Generated => "generated",
        FingerprintPart::Secrets => "secrets",
    }
}

/// One tree entry a class selected, reduced to what a fingerprint covers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Input {
    /// Path from the repository root; sorted on, so a rename moves the key.
    path: PathBuf,
    /// The six-digit octal mode, so making a lockfile executable moves the key.
    mode: String,
    /// The object id, which is Git's hash of the content.
    oid: String,
}

/// A tree's entries bucketed by input class, ready to be folded into keys.
///
/// Selection happens once for both keys, which matters because a path can feed both
/// (`supabase/config.toml`) and because the tree of a real monorepo is large.
#[derive(Debug, Clone, Default)]
pub struct Selected {
    /// One bucket per row of [`CLASSES`], in table order, each sorted by path.
    buckets: Vec<(&'static Class, Vec<Input>)>,
}

impl Selected {
    /// The sub-fingerprints of the classes feeding `key`, in table order.
    fn parts(&self, key: Key) -> Result<Vec<SubFp>> {
        self.buckets
            .iter()
            .filter(|(class, _)| class.key == key)
            .map(|(class, inputs)| {
                Ok(SubFp { part: class.part, digest: sub_digest(class.part, inputs)? })
            })
            .collect()
    }

    /// The paths one part covers, in key order. What `nodal explain` prints when a
    /// user asks why a base was rebuilt.
    #[must_use]
    pub fn paths(&self, part: FingerprintPart) -> Vec<&std::path::Path> {
        self.buckets
            .iter()
            .filter(|(class, _)| class.part == part)
            .flat_map(|(_, inputs)| inputs.iter().map(|input| input.path.as_path()))
            .collect()
    }
}

/// Bucket `entries` by input class.
///
/// Blobs and gitlinks only: a `tree` entry is a directory, whose own object id already
/// covers everything under it, and counting both would hash the same content twice.
/// A gitlink is kept, because a submodule bump is a real dependency change.
#[must_use]
pub fn select(entries: &[Entry]) -> Selected {
    let mut buckets: Vec<(&'static Class, Vec<Input>)> =
        CLASSES.iter().map(|class| (class, Vec::new())).collect();
    for entry in entries.iter().filter(|entry| is_content(entry)) {
        for (class, inputs) in &mut buckets {
            if class.matches(&entry.path) {
                inputs.push(Input {
                    path: entry.path.clone(),
                    mode: entry.mode.clone(),
                    oid: entry.oid.to_string(),
                });
            }
        }
    }
    for (_, inputs) in &mut buckets {
        inputs.sort_unstable();
    }
    Selected { buckets }
}

/// Whether an entry names content rather than a subtree.
fn is_content(entry: &Entry) -> bool {
    matches!(entry.kind, crate::git::tree::Kind::Blob | crate::git::tree::Kind::Commit)
}

/// The digest of one class's inputs. The part's name is in the digest, so an empty
/// dependency class and an empty schema class are different values rather than both
/// being "the digest of nothing".
fn sub_digest(part: FingerprintPart, inputs: &[Input]) -> Result<Digest> {
    let mut hasher = Hasher::new(digest::SUB_DOMAIN);
    hasher.text(part_name(part));
    for input in inputs {
        hasher.field(input.path.as_os_str().as_encoded_bytes());
        hasher.text(&input.mode);
        hasher.text(&input.oid);
    }
    hasher.finish()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{FingerprintPart, compute, select};
    use crate::git::Oid;
    use crate::git::tree::{Entry, Kind};
    use crate::model::Platform;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";

    fn blob(path: &str, oid: &str) -> Entry {
        Entry {
            mode: "100644".to_owned(),
            kind: Kind::Blob,
            oid: Oid::parse(oid).unwrap(),
            path: PathBuf::from(path),
        }
    }

    fn tree(entries: &[Entry]) -> Vec<Entry> {
        entries.to_vec()
    }

    fn platform() -> Platform {
        Platform::parse("x86_64-unknown-linux-gnu").unwrap()
    }

    fn base_tree() -> Vec<Entry> {
        tree(&[
            blob("package.json", A),
            blob("pnpm-lock.yaml", A),
            blob("apps/web/package.json", A),
            blob("apps/web/src/index.ts", A),
            blob("supabase/config.toml", A),
            blob("supabase/migrations/0001_init.sql", A),
            blob("Dockerfile", A),
            blob("turbo.json", A),
            blob("README.md", A),
        ])
    }

    #[test]
    fn a_source_only_change_moves_neither_key() {
        let mut changed = base_tree();
        changed[3] = blob("apps/web/src/index.ts", B);
        let before = compute(&base_tree(), &platform()).unwrap();
        let after = compute(&changed, &platform()).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn a_lockfile_change_moves_the_workspace_key_and_not_the_schema_key() {
        let mut changed = base_tree();
        changed[1] = blob("pnpm-lock.yaml", B);
        let before = compute(&base_tree(), &platform()).unwrap();
        let after = compute(&changed, &platform()).unwrap();
        assert_ne!(before.workspace.key, after.workspace.key);
        assert_eq!(before.schema.key, after.schema.key);
    }

    #[test]
    fn a_migration_moves_the_schema_key_and_not_the_workspace_key() {
        let mut changed = base_tree();
        changed.push(blob("supabase/migrations/0002_add.sql", B));
        let before = compute(&base_tree(), &platform()).unwrap();
        let after = compute(&changed, &platform()).unwrap();
        assert_ne!(before.schema.key, after.schema.key);
        assert_eq!(before.workspace.key, after.workspace.key);
    }

    #[test]
    fn the_shared_supabase_config_moves_both_keys() {
        let mut changed = base_tree();
        changed[4] = blob("supabase/config.toml", B);
        let before = compute(&base_tree(), &platform()).unwrap();
        let after = compute(&changed, &platform()).unwrap();
        assert_ne!(before.workspace.key, after.workspace.key);
        assert_ne!(before.schema.key, after.schema.key);
    }

    #[test]
    fn the_platform_is_part_of_the_workspace_key_only() {
        let other = Platform::parse("aarch64-apple-darwin").unwrap();
        let here = compute(&base_tree(), &platform()).unwrap();
        let there = compute(&base_tree(), &other).unwrap();
        assert_ne!(here.workspace.key, there.workspace.key);
        assert_eq!(here.schema.key, there.schema.key);
    }

    #[test]
    fn only_the_part_that_moved_reports_a_new_digest() {
        let mut changed = base_tree();
        changed[6] = blob("Dockerfile", B);
        let before = compute(&base_tree(), &platform()).unwrap();
        let after = compute(&changed, &platform()).unwrap();
        let moved: Vec<_> = before
            .parts()
            .into_iter()
            .zip(after.parts())
            .filter(|(a, b)| a.digest != b.digest)
            .map(|(a, _)| a.part)
            .collect();
        assert_eq!(moved, vec![FingerprintPart::Services]);
    }

    #[test]
    fn a_rename_moves_the_key_even_though_the_content_is_the_same() {
        let mut renamed = base_tree();
        renamed[5] = blob("supabase/migrations/0001_initial.sql", A);
        let before = compute(&base_tree(), &platform()).unwrap();
        let after = compute(&renamed, &platform()).unwrap();
        assert_ne!(before.schema.key, after.schema.key);
    }

    #[test]
    fn entry_order_does_not_change_a_key() {
        let mut shuffled = base_tree();
        shuffled.reverse();
        assert_eq!(
            compute(&base_tree(), &platform()).unwrap(),
            compute(&shuffled, &platform()).unwrap()
        );
    }

    #[test]
    fn a_directory_entry_is_not_hashed_twice() {
        let mut with_tree = base_tree();
        with_tree.push(Entry {
            mode: "040000".to_owned(),
            kind: Kind::Tree,
            oid: Oid::parse(B).unwrap(),
            path: PathBuf::from("supabase/migrations"),
        });
        assert_eq!(
            compute(&base_tree(), &platform()).unwrap(),
            compute(&with_tree, &platform()).unwrap()
        );
    }

    #[test]
    fn selection_reports_the_paths_a_part_covers() {
        let selected = select(&base_tree());
        assert_eq!(
            selected.paths(FingerprintPart::Dependencies),
            vec![
                Path::new("apps/web/package.json"),
                Path::new("package.json"),
                Path::new("pnpm-lock.yaml")
            ]
        );
        assert!(selected.paths(FingerprintPart::Toolchain).is_empty());
    }
}
