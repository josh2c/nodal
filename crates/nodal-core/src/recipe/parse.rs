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
    #![allow(clippy::unwrap_used)]

    use super::parse;
    use crate::model::recipe::{Backend, PackageManager};

    #[test]
    fn reads_the_keys_it_is_given_and_leaves_the_rest_unset() {
        let recipe =
            parse("package_manager = \"pnpm\"\n[commands]\ntest = \"just test\"\n", "x").unwrap();
        assert_eq!(recipe.package_manager, Some(PackageManager::Pnpm));
        assert_eq!(
            recipe.commands.test.as_ref().map(ToString::to_string),
            Some("just test".into())
        );
        assert_eq!(recipe.backend, None);
        assert_eq!(recipe.backend(), Backend::Native);
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
