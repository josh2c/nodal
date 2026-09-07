//! Shared shape rules for the string-like values the domain model is built from.
//!
//! Every scalar in the model is a validated newtype so that an invalid identifier
//! cannot reach the store or a bundle. Each shape is declared once, as a [`Shape`]:
//! the regular expression that goes into the exported JSON schema, the length cap, and
//! the structural rule Rust applies. A newtype names one shape and gets all three, so a
//! rule cannot be changed for the validator without changing the published schema.
//!
//! The pattern and the structure function are two expressions of one rule, so
//! `shapes_agree_with_their_patterns` in this module's tests runs both over a generated
//! corpus and requires them to answer the same for every string.

/// One scalar's rule, in the three forms it is needed in.
pub(crate) struct Shape {
    /// What the shape is called, in test output.
    #[cfg_attr(not(test), allow(dead_code, reason = "only the agreement test names shapes"))]
    pub(crate) name: &'static str,
    /// The rule as an anchored regular expression, published as `pattern`.
    pub(crate) pattern: &'static str,
    /// The longest value allowed, in characters, published as `maxLength`. Every
    /// shape has one: an unbounded string is not a scalar, and `maxLength: null` is
    /// not a schema.
    pub(crate) max_len: u32,
    /// The rule as code. Emptiness and length are checked by [`Shape::accepts`], so
    /// this function covers structure only.
    pub(crate) structure: fn(&str) -> bool,
}

impl Shape {
    /// Whether `value` has this shape. Every shape rejects the empty string, because
    /// every pattern requires at least one character.
    pub(crate) fn accepts(&self, value: &str) -> bool {
        !value.is_empty()
            && value.chars().count() <= self.max_len as usize
            && (self.structure)(value)
    }
}

/// A canonical ULID: 26 Crockford base-32 characters, uppercase, and a first character
/// low enough that the 130 bits the text can hold do not overflow the 128 bits a ULID
/// has. Decoders accept more than this; what Nodal writes and accepts is the canonical
/// form only, so the schema and the parser agree.
pub(crate) const ULID: Shape = Shape {
    name: "ulid",
    pattern: "^[0-7][0-9A-HJKMNP-TV-Z]{25}$",
    max_len: 26,
    structure: is_canonical_ulid,
};

/// A content digest: lowercase hexadecimal. The length is the fingerprint module's
/// choice, so the cap is only wide enough for any hash it could reasonably pick.
pub(crate) const DIGEST: Shape =
    Shape { name: "digest", pattern: "^[0-9a-f]+$", max_len: 128, structure: is_hex_digest };

/// A Git object id: 40 (SHA-1) or 64 (SHA-256) lowercase hexadecimal characters.
pub(crate) const OBJECT_ID: Shape = Shape {
    name: "object id",
    pattern: "^([0-9a-f]{40}|[0-9a-f]{64})$",
    max_len: 64,
    structure: is_object_id,
};

/// A slug: lowercase alphanumerics in dash-separated groups.
pub(crate) const SLUG: Shape =
    Shape { name: "slug", pattern: "^[a-z0-9]+(-[a-z0-9]+)*$", max_len: 64, structure: is_slug };

/// An SQL identifier we are willing to create a database with. The cap is Postgres's
/// own limit on an identifier.
pub(crate) const SQL_IDENTIFIER: Shape = Shape {
    name: "sql identifier",
    pattern: "^[a-z_][a-z0-9_]*$",
    max_len: 63,
    structure: is_sql_identifier,
};

/// A Git branch name, by the subset of `git check-ref-format` that matters here: no
/// whitespace or control characters, none of Git's operators, no `..`, `//` or `@{`,
/// no leading `-` or `/`, no trailing `/` or `.`, and not ending in `.lock`.
pub(crate) const BRANCH: Shape = Shape {
    name: "branch name",
    pattern: concat!(
        "^(?![-/])(?!.*(?:\\.\\.|//|@\\{))(?!.*[/.]$)(?!.*\\.lock$)",
        "[^\\s~^:?*\\[\\\\\\x00-\\x1F\\x7F-\\x9F]+$"
    ),
    max_len: 255,
    structure: is_branch_name,
};

/// A single word for a machine to read: visible ASCII, no space.
pub(crate) const TOKEN: Shape =
    Shape { name: "token", pattern: "^[\\x21-\\x7E]+$", max_len: 255, structure: is_token };

/// A single line of text for a person to read: anything printable, on one line.
pub(crate) const LINE: Shape = Shape {
    name: "line",
    pattern: "^[^\\x00-\\x1F\\x7F-\\x9F]+$",
    max_len: 255,
    structure: is_line,
};

/// Every shape, so that the agreement test cannot miss one.
#[cfg(test)]
pub(crate) const SHAPES: &[&Shape] =
    &[&ULID, &DIGEST, &OBJECT_ID, &SLUG, &SQL_IDENTIFIER, &BRANCH, &TOKEN, &LINE];

/// The alphabet of a canonical ULID: Crockford base-32 without `I`, `L`, `O` and `U`.
fn is_crockford_upper(byte: u8) -> bool {
    byte.is_ascii_digit()
        || (byte.is_ascii_uppercase() && !matches!(byte, b'I' | b'L' | b'O' | b'U'))
}

fn is_canonical_ulid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 26
        && matches!(bytes[0], b'0'..=b'7')
        && bytes.iter().copied().all(is_crockford_upper)
}

fn is_hex_digest(value: &str) -> bool {
    value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && is_hex_digest(value)
}

fn is_slug(value: &str) -> bool {
    !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_sql_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn is_branch_name(value: &str) -> bool {
    const FORBIDDEN: [char; 7] = ['~', '^', ':', '?', '*', '[', '\\'];
    !value.starts_with(['-', '/'])
        && !value.ends_with(['/', '.'])
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && !matches!(value.rsplit_once('.'), Some((_, "lock")))
        && !value.chars().any(|c| c.is_whitespace() || c.is_control() || FORBIDDEN.contains(&c))
}

fn is_token(value: &str) -> bool {
    value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn is_line(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

/// Define a validated newtype over a `String`, with the rules of one [`Shape`].
///
/// The generated type serialises as its inner string, rejects a value that fails the
/// shape on construction and on deserialisation, and carries the shape's `pattern` and
/// `maxLength` into the JSON schema, so a consumer of the schema applies the same rule.
macro_rules! string_newtype {
    (
        $(#[$meta:meta])*
        $name:ident, kind = $kind:literal, shape = $shape:path
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize, schemars::JsonSchema,
        )]
        #[serde(try_from = "String", into = "String")]
        #[schemars(
            with = "String",
            extend("pattern" = $shape.pattern, "maxLength" = $shape.max_len)
        )]
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
                if $shape.accepts(&value) {
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
    #![allow(clippy::expect_used)]

    use fancy_regex::Regex;

    use super::{BRANCH, LINE, OBJECT_ID, SHAPES, SLUG, SQL_IDENTIFIER, Shape, TOKEN, ULID};

    /// Characters that matter to at least one rule, plus a few that matter to none.
    const ALPHABET: &[char] = &[
        'a', 'z', 'A', 'Z', '0', '7', '9', 'I', 'O', '-', '_', '/', '.', '@', '{', '~', '^', ':',
        '?', '*', '[', '\\', ' ', '\t', '\u{0}', '\u{7f}', '\u{85}', 'é',
    ];

    /// A deterministic generator: the corpus must be the same on every machine.
    struct Xorshift(u64);

    impl Xorshift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn string(&mut self, len: usize) -> String {
            (0..len)
                .map(|_| {
                    let index = usize::try_from(self.next() % ALPHABET.len() as u64)
                        .expect("an index into a short slice fits");
                    ALPHABET[index]
                })
                .collect()
        }
    }

    /// Every string the shapes are checked against: all combinations up to three
    /// characters, a random tail, and the values either side of each length cap.
    fn corpus(shape: &Shape) -> Vec<String> {
        let mut values = vec![String::new()];
        for first in ALPHABET {
            values.push(first.to_string());
            for second in ALPHABET {
                values.push(format!("{first}{second}"));
                values.push(format!("{first}{second}{first}"));
            }
        }
        let mut random = Xorshift(0x2026_0906_0000_0001);
        for len in 1..=12 {
            for _ in 0..400 {
                values.push(random.string(len));
            }
        }
        let max = shape.max_len as usize;
        for unit in ["a", "0", "é"] {
            values.push(unit.repeat(max - 1));
            values.push(unit.repeat(max));
            values.push(unit.repeat(max + 1));
        }
        values
    }

    #[test]
    fn shapes_agree_with_their_patterns() {
        for shape in SHAPES {
            let regex = Regex::new(shape.pattern)
                .unwrap_or_else(|e| panic!("{} has an invalid pattern: {e}", shape.name));
            for value in corpus(shape) {
                let by_pattern = regex.is_match(&value).unwrap_or(false)
                    && value.chars().count() <= shape.max_len as usize;
                assert_eq!(
                    shape.accepts(&value),
                    by_pattern,
                    "{}: the validator and the exported pattern disagree about {value:?}",
                    shape.name
                );
            }
        }
    }

    #[test]
    fn digests_are_lowercase_hex_within_a_cap() {
        use super::DIGEST;
        assert!(DIGEST.accepts("deadbeef"));
        assert!(!DIGEST.accepts("DEADBEEF"));
        assert!(!DIGEST.accepts(""));
        assert!(DIGEST.accepts(&"a".repeat(128)));
        assert!(!DIGEST.accepts(&"a".repeat(129)));
    }

    #[test]
    fn patterns_are_anchored_at_both_ends() {
        for shape in SHAPES {
            assert!(shape.pattern.starts_with('^'), "{} is not anchored", shape.name);
            assert!(shape.pattern.ends_with('$'), "{} is not anchored", shape.name);
        }
    }

    #[test]
    fn ulids_are_canonical_only() {
        assert!(ULID.accepts("01J8Z6H0000000000000000000"));
        assert!(!ULID.accepts("01j8z6h0000000000000000000"), "lowercase is not canonical");
        assert!(!ULID.accepts("ZZZZZZZZZZZZZZZZZZZZZZZZZZ"), "26 characters can overflow 128 bits");
        assert!(!ULID.accepts("01I8Z6H0000000000000000000"), "I is not in the alphabet");
        assert!(!ULID.accepts("01J8Z6H000000000000000000"), "25 characters is not a ulid");
    }

    #[test]
    fn slugs_are_dash_separated_groups() {
        assert!(SLUG.accepts("fix-worker-import"));
        assert!(!SLUG.accepts(""));
        assert!(!SLUG.accepts("-lead"));
        assert!(!SLUG.accepts("trail-"));
        assert!(!SLUG.accepts("double--dash"));
        assert!(!SLUG.accepts("Upper"));
        assert!(!SLUG.accepts(&"a".repeat(65)));
    }

    #[test]
    fn sql_identifiers_start_with_a_letter_or_underscore() {
        assert!(SQL_IDENTIFIER.accepts("nodal_unit_01j"));
        assert!(!SQL_IDENTIFIER.accepts("9lives"));
        assert!(!SQL_IDENTIFIER.accepts("has-dash"));
        assert!(!SQL_IDENTIFIER.accepts(&"a".repeat(64)));
    }

    #[test]
    fn branch_names_follow_check_ref_format() {
        assert!(BRANCH.accepts("nodal/fix-worker-import"));
        assert!(BRANCH.accepts("main"));
        assert!(!BRANCH.accepts("has space"));
        assert!(!BRANCH.accepts("a..b"));
        assert!(!BRANCH.accepts("/leading"));
        assert!(!BRANCH.accepts("trailing/"));
        assert!(!BRANCH.accepts("caret^"));
        assert!(!BRANCH.accepts("wip.lock"));
        assert!(!BRANCH.accepts("at@{1}"));
        assert!(!BRANCH.accepts(&"a".repeat(256)));
    }

    #[test]
    fn object_ids_are_full_length() {
        assert!(OBJECT_ID.accepts(&"a".repeat(40)));
        assert!(OBJECT_ID.accepts(&"a".repeat(64)));
        assert!(!OBJECT_ID.accepts(&"a".repeat(41)));
        assert!(!OBJECT_ID.accepts(&"A".repeat(40)));
    }

    #[test]
    fn tokens_are_visible_ascii_and_lines_are_printable() {
        assert!(TOKEN.accepts("x86_64-unknown-linux-gnu"));
        assert!(!TOKEN.accepts("two words"));
        assert!(!TOKEN.accepts("café"));
        assert!(LINE.accepts("fix the worker import"));
        assert!(LINE.accepts("café"));
        assert!(!LINE.accepts("two\nlines"));
    }
}
