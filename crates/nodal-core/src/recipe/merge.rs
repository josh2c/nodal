//! Precedence: an explicit `nodal.toml` beats what inference proposed.
//!
//! The rule is one line — "a key someone wrote wins over a key we guessed" — and it is
//! applied field by field rather than document by document, so a file that sets only
//! `commands.test` keeps every inferred key around it. Because a recipe is sparse
//! ([`Recipe`]), "set" means `Some` for a scalar and non-empty for a list or a table;
//! there is no third state to reason about.
//!
//! The field lists below are the whole of the rule. Adding a key to [`Recipe`] and
//! forgetting it here is a compile error, because [`merge_fields!`] constructs each
//! struct literally.

use std::collections::BTreeMap;

use crate::model::recipe::{BaseSpec, Commands, Db, Env, Hooks, Recipe, Reclaim, Services, Sync};

/// Combine two values of one shape, keeping whatever the higher-precedence side set.
pub trait Merge: Sized {
    /// `self` is the higher precedence side; `lower` supplies every key `self` left unset.
    #[must_use]
    fn merge(self, lower: Self) -> Self;
}

impl<T> Merge for Option<T> {
    fn merge(self, lower: Self) -> Self {
        self.or(lower)
    }
}

impl<T> Merge for Vec<T> {
    fn merge(self, lower: Self) -> Self {
        if self.is_empty() { lower } else { self }
    }
}

impl<K: Ord, V> Merge for BTreeMap<K, V> {
    fn merge(self, lower: Self) -> Self {
        if self.is_empty() { lower } else { self }
    }
}

/// Implement [`Merge`] for a struct by naming every one of its fields.
macro_rules! merge_fields {
    ($type:ty { $($field:ident),+ $(,)? }) => {
        impl Merge for $type {
            fn merge(self, lower: Self) -> Self {
                Self { $($field: Merge::merge(self.$field, lower.$field)),+ }
            }
        }
    };
}

merge_fields!(Commands { dev, build, test, lint, typecheck, migrate, seed, reset });
merge_fields!(Db { kind, tool, migrations_dir, url_var, fixed_ports });
merge_fields!(Services { shared, per_unit });
merge_fields!(Env { required_local, generated, secrets, stand_in });
merge_fields!(BaseSpec { exclude, invalidate });
merge_fields!(Hooks { pre_new, post_new, pre_merge, post_merge, pre_reclaim, post_reclaim });
merge_fields!(Sync { auto_irreversible });
merge_fields!(Reclaim { trash_retention });
merge_fields!(Recipe {
    backend,
    package_manager,
    package_manager_pin,
    monorepo,
    task_cache,
    dockerfile,
    compose,
    toolchain,
    commands,
    db,
    services,
    env,
    base,
    hooks,
    sync,
    reclaim,
});

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::Merge;
    use crate::model::recipe::{CommandLine, PackageManager, Recipe};

    fn line(text: &str) -> CommandLine {
        CommandLine::parse(text).unwrap()
    }

    #[test]
    fn explicit_beats_inferred_key_by_key() {
        let mut explicit = Recipe::default();
        explicit.commands.test = Some(line("cargo test"));
        let mut inferred = Recipe::default();
        inferred.commands.test = Some(line("pnpm run test"));
        inferred.commands.dev = Some(line("pnpm run dev"));
        inferred.package_manager = Some(PackageManager::Pnpm);

        let merged = explicit.merge(inferred);
        assert_eq!(merged.commands.test, Some(line("cargo test")));
        assert_eq!(merged.commands.dev, Some(line("pnpm run dev")));
        assert_eq!(merged.package_manager, Some(PackageManager::Pnpm));
    }

    #[test]
    fn a_written_list_replaces_the_inferred_one_rather_than_adding_to_it() {
        let mut explicit = Recipe::default();
        explicit.base.exclude = vec![".next".into()];
        let mut inferred = Recipe::default();
        inferred.base.exclude = vec!["coverage".into(), "test-results".into()];

        let merged = explicit.merge(inferred);
        assert_eq!(merged.base.exclude, vec![std::path::PathBuf::from(".next")]);
    }

    #[test]
    fn merging_into_nothing_is_the_inferred_recipe() {
        let mut inferred = Recipe::default();
        inferred.commands.build = Some(line("pnpm run build"));
        assert_eq!(Recipe::default().merge(inferred.clone()), inferred);
    }
}
