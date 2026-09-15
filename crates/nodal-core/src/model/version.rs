//! A program's version, as the program writes it.
//!
//! A validated string and no more. Nothing in the repository orders, sorts or compares
//! two versions: they are recorded so that a person reading a base row or a home's
//! manifest can see which release made it, and a person compares them. The day a caller
//! has to decide that `0.10.0` is newer than `0.9.0`, this type gets the numbers and the
//! ordering that goes with them; until then it would be machinery holding nothing up.

use crate::model::scalar::{self, string_newtype};

string_newtype! {
    /// A program's version: dot-separated numbers, with a pre-release or a build after
    /// them, as the program writes it.
    Version, kind = "version", shape = scalar::VERSION
}

// The order this type derives is the order of its text, which is not the order of a
// version: `0.10.0` sorts before `0.9.0`. Nothing compares two versions, so nothing is
// wrong today; a caller that needs to compare them reads the numbers and brings the
// ordering with it, as the module doc says.

impl Version {
    /// The version of the binary that is running.
    #[must_use]
    pub fn of_this_binary() -> Self {
        // The crate's own version is a literal this crate is compiled with, and the
        // `this_binary_has_a_readable_version` test holds it to the shape.
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Self::before_this_was_recorded())
    }

    /// What a record written before it carried a version reads as.
    ///
    /// Zero, which no release is, so a person reading it is told what is true: the
    /// thing was made by a Nodal that did not say which one it was.
    #[must_use]
    pub fn before_this_was_recorded() -> Self {
        Self(String::from("0"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::Version;

    #[test]
    fn the_text_is_what_was_written() {
        for text in ["0.1.0", "1.2", "10.0.0-rc.1", "1.0.0+build.5"] {
            assert_eq!(Version::parse(text).unwrap().to_string(), text);
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
        assert_eq!(Version::of_this_binary().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_record_that_named_no_version_reads_as_one_no_release_is() {
        assert_eq!(Version::before_this_was_recorded().to_string(), "0");
        assert_ne!(Version::before_this_was_recorded(), Version::of_this_binary());
    }
}
