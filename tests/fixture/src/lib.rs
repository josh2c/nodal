//! The project CI runs Nodal against, so CI never needs a real one.
//!
//! A small pnpm monorepo of the shape the engine was built for: two workspace packages
//! with a task cache over them, a Next application that actually builds, a pinned
//! toolchain, a plain migrations directory with a seed beside it, and a Compose file
//! naming the services. It is one project, generated into a directory, and every test
//! and CI job that needs a project uses this one.
//!
//! Two properties hold it together, and both are asserted rather than hoped for.
//!
//! The first is that it leaves **no gaps**: every key a recipe needs is stated somewhere
//! in the project, so `nodal init` on it has nothing to ask. All but one of those keys
//! is read out of the project's own files. The exception is which Compose services are
//! safe to share between working copies, which no file states because it is a judgement
//! about the project — so the fixture carries the `nodal.toml` that answers it, the way
//! an adopted project would. Delete that file and exactly one gap comes back.
//!
//! The second is that it **builds**, in a budget CI enforces. A fixture that only ever
//! gets read would drift into a shape no real project has; this one installs, builds,
//! lints, type-checks and tests, so the commands the recipe names are commands that run.
//!
//! It also carries content that denies every write and holds an extended attribute,
//! because a base is mostly such content and the two together are what a copier gets
//! wrong. [`read_only`] states why the fixture has to put the attribute there itself.
//!
//! It is generated rather than committed so that every file states what it contributes,
//! and so that a source which starts reading a new file fails here rather than silently
//! finding nothing. No value in it is a secret: the credential names are declared with
//! empty values, which is what a real `.env.example` does.

#![allow(dead_code, reason = "each caller uses the part of the fixture it needs")]

mod files;
pub mod read_only;
pub mod shapes;

use std::path::{Path, PathBuf};

/// One file of the fixture: where it goes, and what is in it.
type File = (&'static str, &'static str);

/// Every file the fixture is made of, in the order they are written.
///
/// This table is the fixture's readable files. A file that is not here is not written,
/// so adding one to [`files`] and forgetting it here fails the tests that read it rather
/// than quietly changing what is inferred. [`read_only::plant`] adds the one file that
/// is not a matter of contents, and [`paths`] names it too.
const FILES: &[File] = &[
    // The root: what names the package manager, the monorepo, the cache and the pins.
    ("pnpm-lock.yaml", files::LOCKFILE),
    ("pnpm-workspace.yaml", files::WORKSPACE),
    ("turbo.json", files::TURBO_JSON),
    ("package.json", files::PACKAGE_JSON),
    (".node-version", files::NODE_VERSION),
    (".gitignore", files::GITIGNORE),
    ("nodal.toml", files::NODAL_TOML),
    // The services, and the image the recipe records but does not act on.
    ("compose.yaml", files::COMPOSE),
    ("Dockerfile", files::DOCKERFILE),
    // The declared environment, in the two files that declare it.
    (".env.example", files::ENV_EXAMPLE),
    ("apps/web/.env.example", files::WEB_ENV_EXAMPLE),
    // The database: a plain migrations directory, a seed, and what applies them.
    ("migrations/0001_create_thing.sql", files::MIGRATION_0001),
    ("migrations/0002_add_thing_created_at.sql", files::MIGRATION_0002),
    ("seed.sql", files::SEED),
    ("scripts/db.mjs", files::DB_SCRIPT),
    ("scripts/generate.mjs", files::GENERATE_SCRIPT),
    ("tests/thing.test.mjs", files::SMOKE_TEST),
    // The Next application.
    ("apps/web/package.json", files::WEB_PACKAGE_JSON),
    ("apps/web/next.config.mjs", files::WEB_NEXT_CONFIG),
    ("apps/web/tsconfig.json", files::WEB_TSCONFIG),
    ("apps/web/next-env.d.ts", files::WEB_NEXT_ENV),
    ("apps/web/app/layout.tsx", files::WEB_LAYOUT),
    ("apps/web/app/page.tsx", files::WEB_PAGE),
    // The package it depends on, so the build has an order to respect.
    ("packages/config/package.json", files::CONFIG_PACKAGE_JSON),
    ("packages/config/tsconfig.json", files::CONFIG_TSCONFIG),
    ("packages/config/src/index.ts", files::CONFIG_INDEX),
    // Regenerated output, which a base clone leaves behind.
    ("test-results/.keep", files::KEEP),
    ("coverage/.keep", files::KEEP),
];

/// The recipe the fixture carries, relative to its root. Removing it is how a test asks
/// what the project states about itself with nothing written down.
pub const RECIPE: &str = "nodal.toml";

/// What a caller got wrong, or what the filesystem refused.
#[derive(Debug)]
pub struct Error {
    /// The file that could not be written.
    pub path: PathBuf,
    /// Why not.
    pub source: std::io::Error,
}

impl std::fmt::Display for Error {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "fixture: {}: {}", self.path.display(), self.source)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Write the fixture under `root` and return it. Overwrites whatever is there, so
/// writing it twice into the same directory leaves the same project.
///
/// # Errors
///
/// [`Error`] naming the first file that could not be created.
pub fn try_write(root: impl AsRef<Path>) -> Result<PathBuf, Error> {
    let root = root.as_ref().to_path_buf();
    for (relative, contents) in FILES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| Error { path: parent.to_path_buf(), source })?;
        }
        read_only::open(&path);
        std::fs::write(&path, contents).map_err(|source| Error { path, source })?;
    }
    read_only::plant(&root);
    Ok(root)
}

/// Write the fixture under `root` and return it, for a test that cannot continue
/// without it.
///
/// # Panics
///
/// If the fixture cannot be written, which means the test cannot run at all.
#[must_use]
pub fn write(root: impl AsRef<Path>) -> PathBuf {
    match try_write(root) {
        Ok(root) => root,
        Err(error) => panic!("{error}"),
    }
}

/// Every path the fixture writes, relative to its root, the read-only one included.
#[must_use]
pub fn paths() -> Vec<&'static str> {
    FILES.iter().map(|(relative, _)| *relative).chain([read_only::LOCKED]).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{FILES, RECIPE, paths, try_write};

    #[test]
    fn no_path_is_written_twice() {
        let mut seen: Vec<&str> = paths();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "a path appears twice in the table");
    }

    #[test]
    fn every_path_is_relative_and_stays_inside_the_root() {
        for (relative, _) in FILES {
            let path = std::path::Path::new(relative);
            assert!(path.is_relative(), "{relative} is not relative");
            assert!(!relative.contains(".."), "{relative} climbs out of the root");
        }
    }

    #[test]
    fn the_fixture_carries_the_recipe_that_answers_its_one_judgement() {
        assert!(paths().contains(&RECIPE), "the recipe is not in the table");
    }

    #[test]
    fn the_fixture_carries_a_read_only_file_that_holds_an_extended_attribute() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = try_write(directory.path()).expect("the fixture writes");
        let locked = root.join(super::read_only::LOCKED);

        let mode = {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::symlink_metadata(&locked).unwrap().permissions().mode() & 0o777
        };
        assert_eq!(mode, 0o444, "the planted file grants a write to somebody");
        assert!(
            std::fs::OpenOptions::new().write(true).open(&locked).is_err(),
            "a file nothing may write can be opened for writing"
        );
        if super::read_only::mark(&locked) {
            assert!(
                super::read_only::marked(&locked),
                "this filesystem holds extended attributes but the fixture put none on"
            );
        } else {
            eprintln!("this filesystem holds no extended attributes; only the mode is proved");
        }
    }

    #[test]
    fn writing_it_twice_leaves_the_same_project() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = try_write(directory.path()).expect("the fixture writes");
        let once: Vec<String> =
            paths().iter().map(|p| std::fs::read_to_string(root.join(p)).unwrap()).collect();
        try_write(directory.path()).expect("the fixture writes again");
        let twice: Vec<String> =
            paths().iter().map(|p| std::fs::read_to_string(root.join(p)).unwrap()).collect();
        assert_eq!(once, twice);
    }
}
