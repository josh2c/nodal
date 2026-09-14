//! A version of a program, compared as numbers.
//!
//! Provenance needs to answer "is the binary that made this older than the one reading
//! it", and a version kept as text cannot: `0.10.0` sorts before `0.9.0` in every
//! lexical order there is. So the value holds the numbers it was written with, and the
//! text is derived from them rather than carried beside them.
//!
//! The numbers are read once, where the value enters: from `CARGO_PKG_VERSION` at a
//! build, or from a row the store hands back. Nothing below this type sees a string.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::scalar;
use crate::{Error, Result};

/// The longest a version may be, in dot-separated numbers. Four covers every scheme
/// this program records; a fifth is a value that is not a version.
const MAX_NUMBERS: usize = 4;

/// A program's version: dot-separated numbers, and what a pre-release adds after them.
///
/// Ordered by the numbers, most significant first, with a missing number read as zero
/// so that `1.2` and `1.2.0` are the same version. A pre-release is older than the
/// release it leads to, which is the one rule that is not arithmetic.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(with = "String", extend("pattern" = scalar::VERSION.pattern, "maxLength" = scalar::VERSION.max_len))]
pub struct Version {
    /// The numbers, most significant first.
    numbers: Vec<u32>,
    /// What a pre-release writes after them, `-rc.1` and its leading dash included.
    pre: Option<String>,
}

impl Version {
    /// What this value is called when it is rejected.
    pub const KIND: &'static str = "version";

    /// The version of the binary that is running.
    #[must_use]
    pub fn of_this_binary() -> Self {
        // The crate's own version is a literal this crate is compiled with, and the
        // `this_binary_has_a_readable_version` test holds it to the shape.
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self { numbers: vec![0], pre: None })
    }

    /// What a record written before it carried a version reads as.
    ///
    /// Zero, which is older than every release, so a comparison says what is true: the
    /// thing was made by a Nodal that did not say which one it was.
    #[must_use]
    pub fn before_this_was_recorded() -> Self {
        Self { numbers: vec![0], pre: None }
    }

    /// Read a version.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidValue`] when the value is not dot-separated numbers, with or
    /// without a pre-release after them.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let invalid = || Error::InvalidValue { kind: Self::KIND, value: value.clone() };
        if !scalar::VERSION.accepts(&value) {
            return Err(invalid());
        }
        let cut = value.find(['-', '+']).unwrap_or(value.len());
        let (head, pre) = value.split_at(cut);
        let numbers: Vec<u32> = head
            .split('.')
            .map(|part| part.parse().map_err(|_| invalid()))
            .collect::<Result<_>>()?;
        if numbers.is_empty() || numbers.len() > MAX_NUMBERS {
            return Err(invalid());
        }
        Ok(Self { numbers, pre: (!pre.is_empty()).then(|| pre.to_owned()) })
    }

    /// The numbers this version was written with, most significant first.
    #[must_use]
    pub fn numbers(&self) -> &[u32] {
        &self.numbers
    }

    /// The numbers with the trailing zeros a shorter spelling would have left out.
    ///
    /// `1.2` and `1.2.0` are one version written two ways, so they have to be equal and
    /// they have to hash the same. This is the form both of them reduce to.
    fn significant(&self) -> &[u32] {
        let mut end = self.numbers.len();
        while end > 1 && self.numbers[end - 1] == 0 {
            end -= 1;
        }
        &self.numbers[..end]
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Version {}

impl std::hash::Hash for Version {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.significant().hash(state);
        self.pre.hash(state);
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let width = self.numbers.len().max(other.numbers.len());
        let at = |numbers: &[u32], index: usize| numbers.get(index).copied().unwrap_or(0);
        for index in 0..width {
            let order = at(&self.numbers, index).cmp(&at(&other.numbers, index));
            if order != std::cmp::Ordering::Equal {
                return order;
            }
        }
        // A pre-release comes before the release of the same numbers; two pre-releases
        // of one release are ordered by their own text, which is all they state.
        match (&self.pre, &other.pre) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(mine), Some(theirs)) => mine.cmp(theirs),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let numbers: Vec<String> = self.numbers.iter().map(u32::to_string).collect();
        write!(out, "{}{}", numbers.join("."), self.pre.as_deref().unwrap_or(""))
    }
}

impl std::str::FromStr for Version {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

impl TryFrom<String> for Version {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(value)
    }
}

impl From<Version> for String {
    fn from(value: Version) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::Version;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn a_version_is_ordered_by_its_numbers_and_not_by_its_text() {
        assert!(v("0.9.0") < v("0.10.0"), "the lexical order is the bug this type exists for");
        assert!(v("1.2.3") < v("1.3.0"));
        assert!(v("2.0.0") > v("1.99.99"));
    }

    #[test]
    fn a_missing_number_is_a_zero() {
        assert_eq!(v("1.2"), v("1.2.0"), "one version written two ways is one version");
        assert!(v("1.2") < v("1.2.1"));
    }

    /// Two values that are equal must hash the same, or a set holds both of them.
    #[test]
    fn two_spellings_of_one_version_hash_the_same() {
        use std::collections::HashSet;
        let held: HashSet<Version> = [v("1.2"), v("1.2.0"), v("1.2.0.0")].into_iter().collect();
        assert_eq!(held.len(), 1);
    }

    #[test]
    fn a_pre_release_comes_before_the_release_it_leads_to() {
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0-rc.1") < v("1.0.0-rc.2"));
    }

    #[test]
    fn the_text_is_what_was_written() {
        for text in ["0.1.0", "1.2", "10.0.0-rc.1", "1.0.0+build.5"] {
            assert_eq!(v(text).to_string(), text);
        }
    }

    #[test]
    fn a_value_that_is_not_a_version_is_refused_where_it_enters() {
        for text in ["", "pnpm@9", "1.2.3.4.5", "v1.2.3", "9999999999", "1..2"] {
            assert!(Version::parse(text).is_err(), "{text} was accepted");
        }
    }

    #[test]
    fn this_binary_has_a_readable_version() {
        let running = Version::of_this_binary();
        assert_eq!(running.to_string(), env!("CARGO_PKG_VERSION"));
        assert!(!running.numbers().is_empty());
    }
}
