//! Identifier newtypes.
//!
//! Every row in the data model is keyed by a ULID: sortable by creation time, safe in a
//! path, and generated without coordination, which matters because units are created by
//! agents on more than one machine. Each table gets its own type so that an environment
//! id cannot be passed where a unit id is expected.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::model::scalar::ULID_PATTERN;

/// Define a ULID-backed identifier newtype.
macro_rules! ulid_newtype {
    ($(#[$meta:meta])* $name:ident, kind = $kind:literal) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            Serialize, Deserialize, JsonSchema,
        )]
        #[serde(try_from = "String", into = "String")]
        #[schemars(with = "String", extend("pattern" = ULID_PATTERN))]
        pub struct $name(Ulid);

        impl $name {
            #[doc = concat!("What this value is called when it is rejected: `", $kind, "`.")]
            pub const KIND: &'static str = $kind;

            #[doc = concat!("Parse a canonical ULID into a [`", stringify!($name), "`].")]
            ///
            /// # Errors
            ///
            /// [`crate::Error::InvalidValue`] if the text is not a ULID.
            pub fn parse(value: impl AsRef<str>) -> crate::Result<Self> {
                let value = value.as_ref();
                Ulid::from_string(value).map(Self).map_err(|_| crate::Error::InvalidValue {
                    kind: $kind,
                    value: value.to_owned(),
                })
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
    fn ids_of_different_tables_are_distinct_types() {
        let unit = UnitId::parse(SAMPLE).expect("sample is a valid ulid");
        let env = EnvId::from_ulid(unit.as_ulid());
        assert_eq!(unit.to_string(), env.to_string());
    }
}
