//! The one place a fingerprint digest is produced.
//!
//! Every digest in this module is SHA-256 over length-prefixed fields. The prefix is
//! what makes the encoding unambiguous: without it, the fields `("ab", "c")` and
//! `("a", "bc")` would hash the same, and two different trees could share a key.
//!
//! Each digest also starts with a domain string, so a sub-fingerprint, a workspace key
//! and a schema key over the same bytes are three different values. Changing a domain
//! string changes every key of that kind, which is the intended way to retire a
//! generation of bases.

use sha2::{Digest as _, Sha256};

use crate::Result;
use crate::model::Digest;

/// The domain of a sub-fingerprint. Bumping the version invalidates every base.
pub(super) const SUB_DOMAIN: &str = "nodal.fingerprint.sub.v1";
/// The domain of a workspace key.
pub(super) const WORKSPACE_DOMAIN: &str = "nodal.fingerprint.workspace.v1";
/// The domain of a schema key.
pub(super) const SCHEMA_DOMAIN: &str = "nodal.fingerprint.schema.v1";
/// The domain of a recipe digest.
pub(super) const RECIPE_DOMAIN: &str = "nodal.fingerprint.recipe.v1";

/// Accumulates length-prefixed fields into one digest.
pub(super) struct Hasher(Sha256);

impl Hasher {
    /// Start a digest in `domain`.
    pub(super) fn new(domain: &str) -> Self {
        let mut hasher = Self(Sha256::new());
        hasher.field(domain.as_bytes());
        hasher
    }

    /// Add one field. The length goes in first, so no two field sequences collide.
    pub(super) fn field(&mut self, bytes: &[u8]) {
        self.0.update((bytes.len() as u64).to_be_bytes());
        self.0.update(bytes);
    }

    /// Add one field, borrowed as text.
    pub(super) fn text(&mut self, text: &str) {
        self.field(text.as_bytes());
    }

    /// Finish, as the lowercase hex the model's `Digest` shape requires.
    ///
    /// # Errors
    /// [`crate::Error::InvalidValue`] can only be raised if the hex encoding below
    /// stopped being hex; it is returned rather than unwrapped because this module
    /// denies `unwrap` like every other.
    pub(super) fn finish(self) -> Result<Digest> {
        let bytes = self.0.finalize();
        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            hex.push(nibble(byte >> 4));
            hex.push(nibble(byte & 0x0f));
        }
        Digest::parse(hex)
    }
}

/// One hex digit of a nibble, lowercase.
fn nibble(value: u8) -> char {
    char::from(match value {
        0..=9 => b'0' + value,
        _ => b'a' + (value - 10),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{Hasher, SUB_DOMAIN, WORKSPACE_DOMAIN};

    fn digest(domain: &str, fields: &[&str]) -> String {
        let mut hasher = Hasher::new(domain);
        for field in fields {
            hasher.text(field);
        }
        hasher.finish().unwrap().as_str().to_owned()
    }

    #[test]
    fn is_lowercase_hex_of_the_full_hash() {
        let value = digest(SUB_DOMAIN, &["a"]);
        assert_eq!(value.len(), 64);
        assert!(value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn field_boundaries_are_not_ambiguous() {
        assert_ne!(digest(SUB_DOMAIN, &["ab", "c"]), digest(SUB_DOMAIN, &["a", "bc"]));
    }

    #[test]
    fn domains_separate_otherwise_identical_inputs() {
        assert_ne!(digest(SUB_DOMAIN, &["x"]), digest(WORKSPACE_DOMAIN, &["x"]));
    }

    #[test]
    fn the_same_input_gives_the_same_digest() {
        assert_eq!(digest(SUB_DOMAIN, &["x", "y"]), digest(SUB_DOMAIN, &["x", "y"]));
    }
}
