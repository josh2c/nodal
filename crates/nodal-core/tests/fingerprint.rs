//! Integration tests for the fingerprint module against temporary repositories.
//!
//! The last test is the acceptance test: it walks a synthetic history commit by commit,
//! counting distinct states and changes per sub-fingerprint, and requires the counts the
//! history was built to produce. That churn walk is the mechanism the module is measured
//! by against a real repository; the real repository is private, so its numbers are
//! reported elsewhere and CI exercises the mechanism on a repository it builds itself.
//!
//! Nothing here touches a repository it did not create.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeSet;
use std::path::Path;

use nodal_core::fingerprint::{self, GitTreeAtCommit};
use nodal_core::git::Git;
use nodal_core::model::{FingerprintPart, Platform};
use tempfile::TempDir;

/// A throwaway repository whose history is written commit by commit.
struct Repo {
    dir: TempDir,
}

impl Repo {
    /// A repository on `main` with the tree a small pnpm + Supabase project starts from.
    fn seeded() -> Self {
        let repo = Self { dir: TempDir::new().unwrap() };
        repo.git(&["init", "--initial-branch=main"]);
        for (key, value) in [
            ("user.email", "tests@nodal.invalid"),
            ("user.name", "Nodal tests"),
            ("commit.gpgsign", "false"),
            ("gc.auto", "6700"),
        ] {
            repo.git(&["config", "--local", key, value]);
        }
        for (path, body) in [
            ("package.json", "{\"name\":\"root\"}\n"),
            ("pnpm-lock.yaml", "lockfileVersion: 9\n"),
            ("apps/web/package.json", "{\"name\":\"web\"}\n"),
            ("apps/web/src/index.ts", "export const a = 1;\n"),
            ("supabase/config.toml", "[db]\nport = 54322\n"),
            ("supabase/seed.sql", "select 1;\n"),
            ("supabase/migrations/0001_init.sql", "create table t ();\n"),
            ("Dockerfile", "FROM node:24\n"),
            ("turbo.json", "{\"tasks\":{}}\n"),
            ("README.md", "seed\n"),
        ] {
            repo.write(path, body);
        }
        repo.commit("seed");
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        nodal_safety::git::git(self.path(), args)
    }

    fn write(&self, path: &str, body: &str) {
        let full = self.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, body).unwrap();
    }

    /// Write one file and commit it, returning nothing: the history is walked later.
    fn commit_file(&self, path: &str, body: &str) {
        self.write(path, body);
        self.commit(path);
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "--quiet", "-m", message]);
    }

    /// Every commit of `main`, oldest first.
    fn history(&self) -> Vec<String> {
        self.git(&["log", "--first-parent", "--reverse", "--format=%H", "main"])
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect()
    }
}

fn platform() -> Platform {
    Platform::parse("x86_64-unknown-linux-gnu").unwrap()
}

/// Both keys of `repo` at `rev`, read the way `nodal new` reads them.
fn at(repo: &Repo, rev: &str) -> fingerprint::Fingerprints {
    let git = Git::open(repo.path()).unwrap();
    fingerprint::compute(&GitTreeAtCommit::new(&git, rev), &platform()).unwrap()
}

#[test]
fn the_same_commit_fingerprints_the_same_every_time() {
    let repo = Repo::seeded();
    assert_eq!(at(&repo, "HEAD"), at(&repo, "HEAD"));
}

#[test]
fn a_source_commit_moves_neither_key() {
    let repo = Repo::seeded();
    let before = at(&repo, "HEAD");
    repo.commit_file("apps/web/src/index.ts", "export const a = 2;\n");
    assert_eq!(before, at(&repo, "HEAD"));
}

#[test]
fn a_lockfile_commit_moves_the_workspace_key_only() {
    let repo = Repo::seeded();
    let before = at(&repo, "HEAD");
    repo.commit_file("pnpm-lock.yaml", "lockfileVersion: 9\nchanged: true\n");
    let after = at(&repo, "HEAD");
    assert_ne!(before.workspace.key, after.workspace.key);
    assert_eq!(before.schema.key, after.schema.key);
    assert_eq!(moved(&before, &after), vec![FingerprintPart::Dependencies]);
}

#[test]
fn a_migration_commit_moves_the_schema_key_only() {
    let repo = Repo::seeded();
    let before = at(&repo, "HEAD");
    repo.commit_file("supabase/migrations/0002_add.sql", "alter table t add c int;\n");
    let after = at(&repo, "HEAD");
    assert_ne!(before.schema.key, after.schema.key);
    assert_eq!(before.workspace.key, after.workspace.key);
    assert_eq!(moved(&before, &after), vec![FingerprintPart::Schema]);
}

#[test]
fn a_package_manifest_deep_in_a_monorepo_counts() {
    let repo = Repo::seeded();
    let before = at(&repo, "HEAD");
    repo.commit_file("packages/worker/package.json", "{\"name\":\"worker\"}\n");
    assert_eq!(moved(&before, &at(&repo, "HEAD")), vec![FingerprintPart::Dependencies]);
}

#[test]
fn the_shared_database_config_moves_both_keys_at_once() {
    let repo = Repo::seeded();
    let before = at(&repo, "HEAD");
    repo.commit_file("supabase/config.toml", "[db]\nport = 54332\n");
    let after = at(&repo, "HEAD");
    assert_ne!(before.workspace.key, after.workspace.key);
    assert_ne!(before.schema.key, after.schema.key);
    assert_eq!(moved(&before, &after), vec![FingerprintPart::Services, FingerprintPart::Schema]);
}

#[test]
fn keys_can_be_taken_at_any_commit_not_only_at_head() {
    let repo = Repo::seeded();
    let seed = at(&repo, "HEAD");
    repo.commit_file("pnpm-lock.yaml", "lockfileVersion: 9\nb: 1\n");
    repo.commit_file("pnpm-lock.yaml", "lockfileVersion: 9\nc: 1\n");
    let history = repo.history();
    assert_eq!(at(&repo, &history[0]), seed);
    assert_ne!(at(&repo, &history[1]).workspace.key, seed.workspace.key);
}

#[test]
fn a_history_walk_counts_distinct_states_and_changes_per_part() {
    let repo = Repo::seeded();
    // A history built to a known shape: 3 dependency states over 2 changes, 4 schema
    // states over 3, 2 service states over 1, and a recipe and toolchain that never
    // move — schema-dominated churn, at a size a test can assert on.
    let commits: &[(&str, &str)] = &[
        ("apps/web/src/index.ts", "export const a = 2;\n"),
        ("supabase/migrations/0002_a.sql", "select 1;\n"),
        ("pnpm-lock.yaml", "lockfileVersion: 9\nb: 1\n"),
        ("docs/notes.md", "notes\n"),
        ("supabase/migrations/0003_b.sql", "select 2;\n"),
        ("Dockerfile", "FROM node:26\n"),
        ("apps/web/src/other.ts", "export const b = 1;\n"),
        ("supabase/migrations/0004_c.sql", "select 3;\n"),
        ("pnpm-lock.yaml", "lockfileVersion: 9\nc: 1\n"),
        ("README.md", "more\n"),
    ];
    for (path, body) in commits {
        repo.commit_file(path, body);
    }

    let series: Vec<_> = repo.history().iter().map(|rev| at(&repo, rev)).collect();
    assert_eq!(series.len(), commits.len() + 1);

    for (part, distinct, changes) in [
        (FingerprintPart::Dependencies, 3, 2),
        (FingerprintPart::Schema, 4, 3),
        (FingerprintPart::Services, 2, 1),
        (FingerprintPart::Recipe, 1, 0),
        (FingerprintPart::Toolchain, 1, 0),
    ] {
        let states: Vec<_> = series.iter().map(|fps| digest_of(fps, part)).collect();
        assert_eq!(states.iter().collect::<BTreeSet<_>>().len(), distinct, "{part:?} distinct");
        assert_eq!(changed(&states), changes, "{part:?} changes");
    }

    // The two keys as a rebuild counter reads them: the workspace key is what a base
    // is rebuilt for and the schema key what a template is rebuilt for.
    let workspace: Vec<_> = series.iter().map(|fps| fps.workspace.key.0.to_string()).collect();
    let schema: Vec<_> = series.iter().map(|fps| fps.schema.key.0.to_string()).collect();
    assert_eq!((workspace.iter().collect::<BTreeSet<_>>().len(), changed(&workspace)), (4, 3));
    assert_eq!((schema.iter().collect::<BTreeSet<_>>().len(), changed(&schema)), (4, 3));

    // The point of two keys: a combined key moves on every change of either, which is
    // more rebuilds than either key alone.
    let combined: Vec<_> = workspace.iter().zip(&schema).map(|(w, s)| format!("{w}:{s}")).collect();
    assert_eq!((combined.iter().collect::<BTreeSet<_>>().len(), changed(&combined)), (7, 6));
}

/// The parts whose digest differs between two fingerprints, in table order.
fn moved(
    before: &fingerprint::Fingerprints,
    after: &fingerprint::Fingerprints,
) -> Vec<FingerprintPart> {
    before
        .parts()
        .into_iter()
        .zip(after.parts())
        .filter(|(a, b)| a.digest != b.digest)
        .map(|(a, _)| a.part)
        .collect()
}

/// The digest of one part.
fn digest_of(fingerprints: &fingerprint::Fingerprints, part: FingerprintPart) -> String {
    fingerprints
        .parts()
        .into_iter()
        .find(|sub| sub.part == part)
        .map(|sub| sub.digest.to_string())
        .unwrap_or_default()
}

/// How many times a series changed from one commit to the next.
fn changed(series: &[String]) -> usize {
    series.windows(2).filter(|pair| pair[0] != pair[1]).count()
}
