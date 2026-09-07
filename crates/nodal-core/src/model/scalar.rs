//! Shared shape rules for the string-like values the domain model is built from.
//!
//! Every scalar in the model is a validated newtype so that an invalid identifier
//! cannot reach the store or a bundle: the same rule validates a constructor call, a
//! `serde` deserialisation and the `pattern` in the exported JSON schema.

/// A canonical ULID: 26 Crockford base-32 characters.
pub(crate) const ULID_PATTERN: &str = "^[0-9A-HJKMNP-TV-Z]{26}$";

/// A content digest: lowercase hexadecimal.
pub(crate) const DIGEST_PATTERN: &str = "^[0-9a-f]+$";

/// A Git object id: 40 or 64 lowercase hexadecimal characters.
pub(crate) const OBJECT_ID_PATTERN: &str = "^([0-9a-f]{40}|[0-9a-f]{64})$";

/// A slug: lowercase alphanumerics in dash-separated groups.
pub(crate) const SLUG_PATTERN: &str = "^[a-z0-9]+(-[a-z0-9]+)*$";

/// An SQL identifier we are willing to create a database with.
pub(crate) const SQL_IDENTIFIER_PATTERN: &str = "^[a-z_][a-z0-9_]*$";

/// A Git branch name, by the subset of `git check-ref-format` that matters here.
pub(crate) const BRANCH_PATTERN: &str = "^[^\\s~^:?*\\[\\\\]+$";

/// A single word: no whitespace.
pub(crate) const TOKEN_PATTERN: &str = "^\\S+$";

/// A single line of text.
pub(crate) const LINE_PATTERN: &str = "^.+$";

/// True when `value` is a non-empty lowercase hexadecimal digest.
pub(crate) fn is_hex_digest(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// True when `value` is a Git object id: 40 (SHA-1) or 64 (SHA-256) hex characters.
pub(crate) fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && is_hex_digest(value)
}

/// True when `value` is a slug: lowercase alphanumerics in dash-separated groups.
pub(crate) fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// True when `value` is an SQL identifier we are willing to create a database with.
pub(crate) fn is_sql_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let leads = bytes.next().is_some_and(|b| b.is_ascii_lowercase() || b == b'_');
    leads
        && value.len() <= 63
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// True when `value` is a Git branch name, by the subset of `git check-ref-format`
/// that matters here: no whitespace, no special characters, no `..`, no leading or
/// trailing separator.
pub(crate) fn is_branch_name(value: &str) -> bool {
    const FORBIDDEN: [char; 8] = ['~', '^', ':', '?', '*', '[', '\\', '\u{7f}'];
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with(['-', '/'])
        && !value.ends_with(['/', '.'])
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && !matches!(value.rsplit_once('.'), Some((_, "lock")))
        && !value.chars().any(|c| c.is_whitespace() || c.is_control() || FORBIDDEN.contains(&c))
}

/// True when `value` is a non-empty word: no whitespace, no control characters.
pub(crate) fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// True when `value` is a non-empty single line: the weakest rule we still enforce.
pub(crate) fn is_line(value: &str) -> bool {
    !value.is_empty() && value.len() <= 255 && !value.chars().any(char::is_control)
}

/// Define a validated newtype over a `String`.
///
/// The generated type serialises as its inner string, rejects a value that fails
/// `validate` on construction and on deserialisation, and carries `pattern` into the
/// JSON schema so that a consumer of the schema applies the same rule we do.
macro_rules! string_newtype {
    (
        $(#[$meta:meta])*
        $name:ident, kind = $kind:literal, pattern = $pattern:expr, validate = $validate:path
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize, schemars::JsonSchema,
        )]
        #[serde(try_from = "String", into = "String")]
        #[schemars(with = "String", extend("pattern" = $pattern))]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("What this value is called when it is rejected: `", $kind, "`.")]
            pub const KIND: &'static str = $kind;

            #[doc = concat!("Validate `value` and wrap it as a [`", stringify!($name), "`].")]
            ///
            /// # Errors
            ///
            /// [`crate::Error::InvalidValue`] if the value does not match the type's shape.
            pub fn parse(value: impl Into<String>) -> crate::Result<Self> {
                let value = value.into();
                if $validate(&value) {
                    Ok(Self(value))
                } else {
                    Err(crate::Error::InvalidValue { kind: $kind, value })
                }
            }

            /// Borrow the value as a string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl core::str::FromStr for $name {
            type Err = crate::Error;
            fn from_str(value: &str) -> crate::Result<Self> {
                Self::parse(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = crate::Error;
            fn try_from(value: String) -> crate::Result<Self> {
                Self::parse(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

pub(crate) use string_newtype;

#[cfg(test)]
mod tests {
    use super::{
        is_branch_name, is_hex_digest, is_object_id, is_slug, is_sql_identifier, is_token,
    };

    #[test]
    fn digests_are_lowercase_hex() {
        assert!(is_hex_digest("0a9f"));
        assert!(!is_hex_digest(""));
        assert!(!is_hex_digest("0A9F"));
        assert!(!is_hex_digest("0g"));
        assert!(is_object_id(&"a".repeat(40)));
        assert!(is_object_id(&"a".repeat(64)));
        assert!(!is_object_id(&"a".repeat(41)));
    }

    #[test]
    fn slugs_are_dash_separated_groups() {
        assert!(is_slug("fix-worker-import"));
        assert!(is_slug("t0"));
        assert!(!is_slug(""));
        assert!(!is_slug("-lead"));
        assert!(!is_slug("trail-"));
        assert!(!is_slug("double--dash"));
        assert!(!is_slug("Upper"));
        assert!(!is_slug(&"a".repeat(65)));
    }

    #[test]
    fn sql_identifiers_start_with_a_letter_or_underscore() {
        assert!(is_sql_identifier("nodal_unit_01j"));
        assert!(is_sql_identifier("_t"));
        assert!(!is_sql_identifier("9lives"));
        assert!(!is_sql_identifier("has-dash"));
        assert!(!is_sql_identifier(""));
        assert!(!is_sql_identifier(&"a".repeat(64)));
    }

    #[test]
    fn tokens_have_no_whitespace() {
        assert!(is_token("x86_64-unknown-linux-gnu"));
        assert!(!is_token("two words"));
        assert!(!is_token(""));
    }

    #[test]
    fn branch_names_follow_check_ref_format() {
        assert!(is_branch_name("nodal/fix-worker-import"));
        assert!(is_branch_name("main"));
        assert!(!is_branch_name(""));
        assert!(!is_branch_name("has space"));
        assert!(!is_branch_name("a..b"));
        assert!(!is_branch_name("/leading"));
        assert!(!is_branch_name("trailing/"));
        assert!(!is_branch_name("caret^"));
        assert!(!is_branch_name("wip.lock"));
    }
}
