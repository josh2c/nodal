//! The one instant type the model uses.
//!
//! Times are UTC and serialise as RFC 3339, so an event line in `events.jsonl` is
//! readable by a human, sortable as text, and unambiguous across machines and time
//! zones — the record has to survive a handoff to another host.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// A UTC instant, serialised as an RFC 3339 string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, JsonSchema)]
#[schemars(with = "String", extend("format" = "date-time"))]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// The current instant. Reading the clock is the only ambient value the model
    /// takes; it touches nothing on the machine.
    #[must_use]
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    /// Parse an RFC 3339 timestamp.
    ///
    /// # Errors
    ///
    /// [`crate::Error::InvalidValue`] if the text is not RFC 3339.
    pub fn parse(value: &str) -> crate::Result<Self> {
        OffsetDateTime::parse(value, &Rfc3339)
            .map(|at| Self(at.to_offset(time::UtcOffset::UTC)))
            .map_err(|_| crate::Error::InvalidValue { kind: "timestamp", value: value.to_owned() })
    }

    /// Build from an offset date-time, normalised to UTC.
    #[must_use]
    pub fn from_offset_date_time(value: OffsetDateTime) -> Self {
        Self(value.to_offset(time::UtcOffset::UTC))
    }

    /// The instant as an offset date-time.
    #[must_use]
    pub const fn to_offset_date_time(self) -> OffsetDateTime {
        self.0
    }

    /// Whole seconds since the Unix epoch, the form the store keeps.
    #[must_use]
    pub const fn unix_seconds(self) -> i64 {
        self.0.unix_timestamp()
    }

    /// Build from whole seconds since the Unix epoch.
    ///
    /// # Errors
    ///
    /// [`crate::Error::InvalidValue`] if the count is outside the supported range.
    pub fn from_unix_seconds(seconds: i64) -> crate::Result<Self> {
        OffsetDateTime::from_unix_timestamp(seconds).map(Self).map_err(|_| {
            crate::Error::InvalidValue { kind: "unix timestamp", value: seconds.to_string() }
        })
    }
}

impl core::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0.format(&Rfc3339) {
            Ok(text) => f.write_str(&text),
            Err(_) => Err(core::fmt::Error),
        }
    }
}

impl core::str::FromStr for Timestamp {
    type Err = crate::Error;
    fn from_str(value: &str) -> crate::Result<Self> {
        Self::parse(value)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        time::serde::rfc3339::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        time::serde::rfc3339::deserialize(deserializer).map(Self::from_offset_date_time)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::Timestamp;

    #[test]
    fn parses_and_prints_rfc3339_utc() {
        let at = Timestamp::parse("2026-09-06T10:11:12Z").expect("valid rfc 3339");
        assert_eq!(at.to_string(), "2026-09-06T10:11:12Z");
        assert_eq!(at.unix_seconds(), 1_788_689_472);
    }

    #[test]
    fn offsets_are_normalised_to_utc() {
        let east = Timestamp::parse("2026-09-06T12:11:12+02:00").expect("valid rfc 3339");
        assert_eq!(east, Timestamp::parse("2026-09-06T10:11:12Z").expect("valid rfc 3339"));
    }

    #[test]
    fn rejects_other_formats() {
        assert!(Timestamp::parse("2026-09-06 10:11:12").is_err());
        assert!(Timestamp::parse("").is_err());
    }

    #[test]
    fn unix_seconds_round_trip() {
        let at = Timestamp::from_unix_seconds(1_788_689_472).expect("in range");
        assert_eq!(at.to_string(), "2026-09-06T10:11:12Z");
    }
}
