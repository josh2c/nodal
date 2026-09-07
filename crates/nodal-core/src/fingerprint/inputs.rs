//! Which paths feed which sub-fingerprint: a table, not code.
//!
//! Everything that decides what a fingerprint covers is in [`CLASSES`]. Adding a
//! package manager or a migration tool is a row, and a row is reviewable next to the
//! measurement that justified it. The matching below is four rules and nothing else,
//! so a reader never has to run the code to know what a row selects.
//!
//! One path may feed two classes. `supabase/config.toml` is the example: it declares
//! the database the schema is built against *and* the services a workspace runs, so it
//! is an input to both, and either key moving is correct.

use std::path::Path;

use crate::model::FingerprintPart;

/// Which of the two keys a class contributes to. Bases and templates are keyed
/// separately, and neither key is a superset of the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Feeds the workspace fingerprint a base is stored under.
    Workspace,
    /// Feeds the schema fingerprint a database template is stored under.
    Schema,
}

/// How a row selects paths out of a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selector {
    /// Exactly this path, relative to the repository root.
    Path(&'static str),
    /// Any file with this name, at any depth. A monorepo has one manifest per
    /// package, and all of them belong to the dependency fingerprint.
    Name(&'static str),
    /// Any file whose name starts with this, at any depth: `Dockerfile`,
    /// `Dockerfile.web`, `docker-compose.prod.yml`.
    NamePrefix(&'static str),
    /// Every entry under this directory. A migrations directory is a tree, and a file
    /// added anywhere in it moves the schema.
    Dir(&'static str),
}

impl Selector {
    /// Whether this selector covers `path`.
    #[must_use]
    pub fn matches(self, path: &Path) -> bool {
        match self {
            Self::Path(exact) => path == Path::new(exact),
            Self::Name(name) => file_name(path) == Some(name),
            Self::NamePrefix(prefix) => {
                file_name(path).is_some_and(|name| name.starts_with(prefix))
            }
            Self::Dir(dir) => path.starts_with(dir) && path != Path::new(dir),
        }
    }
}

/// The file name of `path` as text, `None` when it has none or it is not UTF-8.
fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(std::ffi::OsStr::to_str)
}

/// One input class: a named part of a fingerprint and the paths that feed it.
#[derive(Debug, Clone, Copy)]
pub struct Class {
    /// The part this class computes, as a sync step reports it.
    pub part: FingerprintPart,
    /// Which key the part contributes to.
    pub key: Key,
    /// The paths that feed it.
    pub selectors: &'static [Selector],
}

impl Class {
    /// Whether any selector of this class covers `path`.
    #[must_use]
    pub fn matches(&self, path: &Path) -> bool {
        self.selectors.iter().any(|selector| selector.matches(path))
    }
}

/// Every class, in the order a fingerprint composes them. The order is part of the
/// key: reordering these rows changes every fingerprint, so rows are appended.
///
/// `FingerprintPart::Generated` and `FingerprintPart::Secrets` are not here. They are
/// named parts of a sync plan, but they are read from the environment and the recipe's
/// secret sources rather than from a tree, so they have no tree inputs to table.
pub const CLASSES: &[Class] = &[
    Class {
        part: FingerprintPart::Toolchain,
        key: Key::Workspace,
        selectors: &[
            Selector::Name(".nvmrc"),
            Selector::Name(".node-version"),
            Selector::Name(".tool-versions"),
            Selector::Name("mise.toml"),
            Selector::Name(".mise.toml"),
            Selector::Name(".python-version"),
            Selector::Name(".ruby-version"),
            Selector::Name(".java-version"),
            Selector::Name("rust-toolchain"),
            Selector::Name("rust-toolchain.toml"),
        ],
    },
    Class {
        part: FingerprintPart::Dependencies,
        key: Key::Workspace,
        selectors: &[
            Selector::Name("package.json"),
            Selector::Name(".npmrc"),
            Selector::Name(".yarnrc.yml"),
            Selector::Name("pnpm-workspace.yaml"),
            Selector::Name("pnpm-lock.yaml"),
            Selector::Name("package-lock.json"),
            Selector::Name("npm-shrinkwrap.json"),
            Selector::Name("yarn.lock"),
            Selector::Name("bun.lock"),
            Selector::Name("bun.lockb"),
            Selector::Name("Cargo.toml"),
            Selector::Name("Cargo.lock"),
            Selector::Name("go.mod"),
            Selector::Name("go.sum"),
            Selector::Name("pyproject.toml"),
            Selector::Name("poetry.lock"),
            Selector::Name("uv.lock"),
            Selector::Name("requirements.txt"),
            Selector::Name("Pipfile.lock"),
            Selector::Name("Gemfile"),
            Selector::Name("Gemfile.lock"),
            Selector::Name("composer.json"),
            Selector::Name("composer.lock"),
        ],
    },
    Class {
        part: FingerprintPart::Services,
        key: Key::Workspace,
        selectors: &[
            Selector::NamePrefix("Dockerfile"),
            Selector::NamePrefix("docker-compose"),
            Selector::Name(".dockerignore"),
            Selector::Name("compose.yml"),
            Selector::Name("compose.yaml"),
            Selector::Path("supabase/config.toml"),
        ],
    },
    Class {
        part: FingerprintPart::Recipe,
        key: Key::Workspace,
        selectors: &[
            Selector::Path("nodal.toml"),
            Selector::Path("turbo.json"),
            Selector::Path("nx.json"),
            Selector::Path("lerna.json"),
            Selector::Path(".env.example"),
        ],
    },
    Class {
        part: FingerprintPart::Schema,
        key: Key::Schema,
        selectors: &[
            Selector::Dir("supabase/migrations"),
            Selector::Path("supabase/config.toml"),
            Selector::Path("supabase/seed.sql"),
            Selector::Dir("prisma/migrations"),
            Selector::Path("prisma/schema.prisma"),
            Selector::Dir("db/migrations"),
            Selector::Dir("db/migrate"),
            Selector::Dir("migrations"),
        ],
    },
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{CLASSES, Class, Key, Selector};
    use crate::model::FingerprintPart;

    fn class(part: FingerprintPart) -> &'static Class {
        CLASSES.iter().find(|class| class.part == part).unwrap_or(&CLASSES[0])
    }

    #[test]
    fn a_name_matches_at_any_depth_and_a_path_only_at_the_root() {
        assert!(Selector::Name("package.json").matches(Path::new("apps/web/package.json")));
        assert!(Selector::Name("package.json").matches(Path::new("package.json")));
        assert!(Selector::Path("turbo.json").matches(Path::new("turbo.json")));
        assert!(!Selector::Path("turbo.json").matches(Path::new("apps/web/turbo.json")));
    }

    #[test]
    fn a_prefix_is_case_sensitive_so_a_script_is_not_a_dockerfile() {
        let prefix = Selector::NamePrefix("Dockerfile");
        assert!(prefix.matches(Path::new("packages/worker/Dockerfile")));
        assert!(prefix.matches(Path::new("Dockerfile.web")));
        assert!(!prefix.matches(Path::new("scripts/dockerfile-runtime.test.mjs")));
    }

    #[test]
    fn a_dir_covers_its_entries_and_not_a_sibling_with_the_same_prefix() {
        let dir = Selector::Dir("supabase/migrations");
        assert!(dir.matches(Path::new("supabase/migrations/0001_init.sql")));
        assert!(dir.matches(Path::new("supabase/migrations/nested/0002.sql")));
        assert!(!dir.matches(Path::new("supabase/migrations")));
        assert!(!dir.matches(Path::new("supabase/migrations-old/0001.sql")));
        assert!(!dir.matches(Path::new("docs/standards/migrations/style.md")));
    }

    #[test]
    fn supabase_config_feeds_both_keys() {
        let path = Path::new("supabase/config.toml");
        assert!(class(FingerprintPart::Services).matches(path));
        assert!(class(FingerprintPart::Schema).matches(path));
    }

    #[test]
    fn every_class_belongs_to_exactly_one_key_and_the_keys_are_not_empty() {
        let count = |key: Key| CLASSES.iter().filter(|class| class.key == key).count();
        let (workspace, schema) = (count(Key::Workspace), count(Key::Schema));
        assert_eq!(workspace + schema, CLASSES.len());
        assert_eq!(workspace, 4);
        assert_eq!(schema, 1);
    }

    #[test]
    fn no_part_is_tabled_twice() {
        let mut parts: Vec<_> = CLASSES.iter().map(|class| class.part).collect();
        parts.sort_unstable();
        let before = parts.len();
        parts.dedup();
        assert_eq!(parts.len(), before);
    }
}
