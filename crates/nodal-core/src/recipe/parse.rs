//! Read a `nodal.toml` that a person wrote.
//!
//! Strict, unlike inference: an unknown key is an error rather than something ignored,
//! because a key that is silently dropped is a line someone wrote and Nodal did not
//! honour. A typo in a recipe should be a message about that line, not a unit that
//! quietly comes up wrong.

use std::path::Path;

use crate::error::{Error, Result};
use crate::model::recipe::Recipe;

/// Parse the text of a recipe file. `path` is carried only so a failure names the file.
///
/// # Errors
///
/// [`Error::Recipe`] if the text is not TOML, sets a key the recipe does not have, or
/// gives a value a shape its type rejects.
pub fn parse(text: &str, path: impl AsRef<Path>) -> Result<Recipe> {
    toml::from_str(text).map_err(|source| Error::Recipe {
        path: path.as_ref().to_path_buf(),
        source: Box::new(source),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::parse;
    use crate::model::recipe::{Backend, PackageManager};

    #[test]
    fn reads_the_keys_it_is_given_and_leaves_the_rest_unset() {
        let recipe =
            parse("package_manager = \"pnpm\"\n[commands]\ntest = \"just test\"\n", "x").unwrap();
        assert_eq!(recipe.package_manager, [PackageManager::Pnpm]);
        assert_eq!(
            recipe.commands.test.as_ref().map(ToString::to_string),
            Some("just test".into())
        );
        assert_eq!(recipe.backend, None);
        assert_eq!(recipe.backend(), Backend::Native);
    }

    /// A recipe written before a project had more than one ecosystem still loads, and
    /// a recipe that names several installs them in the order it wrote.
    #[test]
    fn the_package_manager_key_is_read_as_one_manager_or_as_several() {
        let one = parse("package_manager = \"pnpm\"\n", "x").unwrap();
        assert_eq!(one.package_manager, [PackageManager::Pnpm]);
        assert_eq!(one.package_manager.first().copied(), Some(PackageManager::Pnpm));

        let many = parse("package_manager = [\"cargo\", \"pnpm\", \"uv\"]\n", "x").unwrap();
        assert_eq!(
            many.package_manager,
            [PackageManager::Cargo, PackageManager::Pnpm, PackageManager::Uv]
        );
        assert_eq!(many.package_manager.first().copied(), Some(PackageManager::Cargo));
        assert_eq!(many.script_manager(), Some(PackageManager::Pnpm));
    }

    /// A recipe naming a manager that is not one of the seven.
    ///
    /// `pip` is not one of the seven managers, and the message a person got named the
    /// reader's own type instead of their line. The three facts a correction needs are
    /// the key, the word and the accepted words.
    #[test]
    fn a_package_manager_nodal_does_not_know_is_named_with_the_words_that_are_accepted() {
        let Err(refused) = parse("package_manager = [\"npm\", \"cargo\", \"pip\"]\n", "x") else {
            panic!("pip is not a package manager Nodal knows")
        };
        let said = refused.to_string();
        assert!(said.contains("package_manager"), "the key is not named: {said}");
        assert!(said.contains("pip"), "the value that was refused is not named: {said}");
        for accepted in ["npm", "pnpm", "yarn", "bun", "cargo", "uv", "poetry"] {
            assert!(said.contains(accepted), "{accepted} is not offered: {said}");
        }
        assert!(!said.contains("OneOrMany"), "the reader's own type is named: {said}");
        assert!(!said.contains("untagged"), "the reader's own shape is named: {said}");
    }

    /// One word is read the same way as a list of them, so the message is the same.
    #[test]
    fn one_package_manager_nodal_does_not_know_fails_the_same_way() {
        let Err(refused) = parse("package_manager = \"pip\"\n", "x") else {
            panic!("pip is not a package manager Nodal knows")
        };
        let said = refused.to_string();
        assert!(said.contains("pip"), "{said}");
        assert!(said.contains("poetry"), "{said}");
    }

    #[test]
    fn an_unknown_key_is_an_error_rather_than_a_silent_drop() {
        assert!(parse("packagemanager = \"pnpm\"\n", "x").is_err());
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_rejected_where_it_enters() {
        assert!(parse("[env]\nsecrets = [\"lower_case\"]\n", "x").is_err());
    }
}
