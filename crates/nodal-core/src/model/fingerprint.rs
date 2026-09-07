//! Fingerprint value types. Computation lives in the `fingerprint` module.
//!
//! Two keys, not one: a workspace fingerprint over what makes a base warm (lockfiles,
//! manifests, toolchain pins, container definitions, the recipe, the platform triple)
//! and a schema fingerprint over what makes a database template current (migrations,
//! database config, seed). They move at different rates, so they are separate keys and
//! ordinary commits invalidate neither.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::scalar::{
    DIGEST_PATTERN, TOKEN_PATTERN, is_hex_digest, is_token, string_newtype,
};

string_newtype! {
    /// A content digest, lowercase hex. The algorithm is the fingerprint module's
    /// choice; the model only fixes the form.
    Digest, kind = "digest", pattern = DIGEST_PATTERN, validate = is_hex_digest
}

string_newtype! {
    /// A target triple, for example `aarch64-apple-darwin`. Part of the workspace
    /// fingerprint, because a base built on one platform is not warm on another.
    Platform, kind = "platform triple", pattern = TOKEN_PATTERN, validate = is_token
}

/// The key a warm base is stored under.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct WorkspaceFp(pub Digest);

/// The key a database template is stored under.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct SchemaFp(pub Digest);

/// The parts a workspace or schema fingerprint is composed of. A sync plans one step
/// per part that moved, which is why the parts are named rather than folded away.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FingerprintPart {
    /// Language and tool version pins.
    Toolchain,
    /// Lockfiles, package manifests and the package manager's configuration.
    Dependencies,
    /// Migrations, database configuration and seed.
    Schema,
    /// Container and service definitions.
    Services,
    /// Recipe-generated environment values.
    Generated,
    /// The names (never the values) of secrets the recipe requires.
    Secrets,
}

/// One named part of a fingerprint, so a diff can say what moved.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct SubFp {
    /// Which part this digest covers.
    pub part: FingerprintPart,
    /// The digest of that part's inputs.
    pub digest: Digest,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{Digest, FingerprintPart, Platform, SubFp};

    #[test]
    fn digests_reject_uppercase_and_empty() {
        assert!(Digest::parse("deadbeef").is_ok());
        assert!(Digest::parse("DEADBEEF").is_err());
        assert!(Digest::parse("").is_err());
    }

    #[test]
    fn platform_rejects_whitespace() {
        assert!(Platform::parse("x86_64-unknown-linux-gnu").is_ok());
        assert!(Platform::parse("two words").is_err());
    }

    #[test]
    fn parts_name_themselves_in_json() {
        let sub = SubFp {
            part: FingerprintPart::Dependencies,
            digest: Digest::parse("0f").expect("valid digest"),
        };
        let json = serde_json::to_string(&sub).expect("serialises");
        assert_eq!(json, r#"{"part":"dependencies","digest":"0f"}"#);
    }
}
