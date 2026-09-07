//! Object ids, validated once at the boundary where `git` output is parsed.

use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

/// Accepted hexadecimal lengths: SHA-1 and SHA-256 object ids.
const LENGTHS: [usize; 2] = [40, 64];

/// A full Git object id, lower-case hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid(String);

impl Oid {
    /// Parse a full object id.
    ///
    /// # Errors
    /// [`Error::GitOid`] when the text is not 40 or 64 lower-case hex digits.
    pub fn parse(text: &str) -> Result<Self> {
        let shaped = LENGTHS.contains(&text.len())
            && text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if shaped {
            Ok(Self(text.to_owned()))
        } else {
            Err(Error::GitOid { text: text.to_owned() })
        }
    }

    /// The id as it came from `git`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for Oid {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        Self::parse(text)
    }
}

impl AsRef<str> for Oid {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::Oid;

    const SHA1: &str = "1e2f3a4b5c6d7e8f90112233445566778899aabb";

    #[test]
    fn accepts_sha1_and_sha256() {
        assert_eq!(Oid::parse(SHA1).unwrap().as_str(), SHA1);
        assert!(Oid::parse(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn rejects_short_upper_case_and_non_hex() {
        assert!(Oid::parse("1e2f3a4b").is_err());
        assert!(Oid::parse(&SHA1.to_uppercase()).is_err());
        assert!(Oid::parse(&"z".repeat(40)).is_err());
    }
}
