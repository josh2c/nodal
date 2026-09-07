//! Identifier newtypes.
//!
//! Every row in the data model is keyed by a ULID: sortable by creation time, safe in a
//! path, and generated without coordination, which matters because units are created by
//! agents on more than one machine. Each table gets its own type so that an environment
//! id cannot be passed where a unit id is expected.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::model::scalar;

/// Define a ULID-backed identifier newtype.
macro_rules! ulid_newtype {
    ($(#[$meta:meta])* $name:ident, kind = $kind:literal) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            Serialize, Deserialize, JsonSchema,
        )]
        #[serde(try_from = "String", into = "String")]
        #[schemars(
            with = "String",
            extend("pattern" = scalar::ULID.pattern, "maxLength" = scalar::ULID.max_len)
        )]
        pub struct $name(Ulid);

        impl $name {
            #[doc = concat!("What this value is called when it is rejected: `", $kind, "`.")]
            pub const KIND: &'static str = $kind;

            #[doc = concat!("Parse a canonical ULID into a [`", stringify!($name), "`].")]
            ///
            /// Only the canonical form is accepted: 26 uppercase Crockford base-32
            /// characters that fit in 128 bits. `Ulid::from_string` is more generous
            /// than that — it takes lowercase, reads `I` and `L` as `1`, and wraps a
            /// 26-character string that overflows — so the shape is checked first and
            /// the decoder is only asked to turn accepted text into bits.
            ///
            /// # Errors
            ///
            /// [`crate::Error::InvalidValue`] if the text is not a canonical ULID.
            pub fn parse(value: impl AsRef<str>) -> crate::Result<Self> {
                let value = value.as_ref();
                let invalid = || crate::Error::InvalidValue { kind: $kind, value: value.to_owned() };
                if !scalar::ULID.accepts(value) {
                    return Err(invalid());
                }
                Ulid::from_string(value).map(Self).map_err(|_| invalid())
            }

            #[doc = concat!("Wrap an existing [`Ulid`] as a [`", stringify!($name), "`].")]
            #[must_use]
            pub const fn from_ulid(value: Ulid) -> Self {
                Self(value)
            }

            /// The underlying ULID, for time extraction and ordering.
            #[must_use]
            pub const fn as_ulid(self) -> Ulid {
                self.0
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}", self.0)
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
                value.0.to_string()
            }
        }
    };
}

ulid_newtype! {
    /// Identifies a project: one Git repository Nodal manages units for.
    ProjectId, kind = "project id"
}

ulid_newtype! {
    /// Identifies a warm base: the substrate a unit's home is cloned from.
    BaseId, kind = "base id"
}

ulid_newtype! {
    /// Identifies a frozen database template.
    TemplateId, kind = "template id"
}

ulid_newtype! {
    /// Identifies a `WorkUnit`. Stable for the life of the work, across machines.
    UnitId, kind = "unit id"
}

ulid_newtype! {
    /// Identifies one materialisation of a unit. A unit re-materialised after reclaim
    /// gets a new environment, not a new unit.
    EnvId, kind = "environment id"
}

ulid_newtype! {
    /// Identifies one attachment of an actor to an environment.
    SessionId, kind = "session id"
}

ulid_newtype! {
    /// Identifies one event in the log. Sorting by id sorts by time.
    EventId, kind = "event id"
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{EnvId, UnitId};

    const SAMPLE: &str = "01J8Z6H0000000000000000000";

    #[test]
    fn round_trips_through_text() {
        let id = UnitId::parse(SAMPLE).expect("sample is a valid ulid");
        assert_eq!(id.to_string(), SAMPLE);
        assert_eq!(UnitId::parse(id.to_string()).expect("re-parses"), id);
    }

    #[test]
    fn rejects_non_ulid_text() {
        let error = UnitId::parse("not-a-ulid").expect_err("must be rejected");
        assert!(error.to_string().contains("unit id"), "{error}");
        assert!(EnvId::parse("").is_err());
    }

    #[test]
    fn only_the_canonical_form_is_accepted() {
        for rejected in [
            "01j8z6h0000000000000000000",
            "ZZZZZZZZZZZZZZZZZZZZZZZZZZ",
            "01I8Z6H0000000000000000000",
            " 01J8Z6H000000000000000000",
        ] {
            assert!(
                UnitId::parse(rejected).is_err(),
                "{rejected:?} is not canonical and must not be decoded"
            );
        }
    }

    #[test]
    fn what_the_pattern_accepts_is_what_parse_accepts() {
        assert!(UnitId::parse(SAMPLE).is_ok());
        let overflowed = UnitId::parse("8ZZZZZZZZZZZZZZZZZZZZZZZZZ");
        assert!(overflowed.is_err(), "a value past the 128-bit range must not wrap");
    }

    #[test]
    fn ids_of_different_tables_are_distinct_types() {
        let unit = UnitId::parse(SAMPLE).expect("sample is a valid ulid");
        let env = EnvId::from_ulid(unit.as_ulid());
        assert_eq!(unit.to_string(), env.to_string());
    }
}
