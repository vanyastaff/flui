//! The version of the agent-protocol schema, independent of this crate's own
//! version, of AccessKit's and of the MCP SDK's (ADR-0095 §1).
//!
//! # The rule
//!
//! - Any change to what the schema describes bumps [`PROTOCOL_VERSION`]'s
//!   `minor` and publishes the new schema as a golden file beside the old
//!   ones (`tests/schema/protocol-<version>.json`). The `wire_schema` test
//!   refuses a schema that differs from its version's golden file.
//! - An additive change in ADR-0080's sense (a new optional field, tool, role,
//!   action or error code) needs nothing more: the same test checks that every
//!   published schema is a subset of the next one. A reader built on an older
//!   schema still reads the newer reply: an unknown field is ignored, an
//!   unknown role reads as [`Role::Unknown`](crate::Role::Unknown), an
//!   unknown action name is dropped from a node's `actions`, and an unknown
//!   error code reads as [`ErrorCode::Platform`](crate::ErrorCode::Platform).
//! - A change that is not additive (a field renamed or removed, a name
//!   respelled, an optional field made required, or an existing field given
//!   a new meaning) is listed in [`BREAKING`] with the ADR that decided it.
//!   After 1.0 it also bumps `major`, and that ADR supersedes ADR-0080. The
//!   test sees only the schema's shape, so a new meaning is caught in review:
//!   it goes under a new name instead, as `surface_rect` did beside `rect`.

use std::fmt;
use std::str::FromStr;

/// A version of the agent-protocol schema: `major.minor`, spelled `"0.1"`.
///
/// Ordered by `major`, then `minor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    /// Bumped by a change that is not additive, after 1.0.
    pub major: u16,
    /// Bumped by every change to the schema.
    pub minor: u16,
}

impl ProtocolVersion {
    /// The version `major.minor`.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

/// The schema this build of the crate speaks.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(0, 2);

/// The versions whose schema is not additive to the one before it, each with
/// the ADR that decided the break. Empty: every published schema so far only
/// adds to the last.
pub const BREAKING: &[(ProtocolVersion, &str)] = &[];

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Why a string is not a protocol version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseVersionError(String);

impl fmt::Display for ParseVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a protocol version; expected `<major>.<minor>`, such as `0.1`",
            self.0
        )
    }
}

impl std::error::Error for ParseVersionError {}

/// One decimal component with no sign and no leading zero, so each version
/// has exactly one spelling.
pub(crate) fn canonical_decimal<T: FromStr>(text: &str) -> Option<T> {
    let digits_only = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let no_leading_zero = text == "0" || !text.starts_with('0');
    (digits_only && no_leading_zero)
        .then(|| text.parse().ok())
        .flatten()
}

impl FromStr for ProtocolVersion {
    type Err = ParseVersionError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let refuse = || ParseVersionError(text.to_owned());
        let (major, minor) = text.split_once('.').ok_or_else(refuse)?;
        Ok(Self {
            major: canonical_decimal(major).ok_or_else(refuse)?,
            minor: canonical_decimal(minor).ok_or_else(refuse)?,
        })
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ProtocolVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ProtocolVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ProtocolVersion {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProtocolVersion".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "The agent-protocol schema version, `<major>.<minor>`",
            "type": "string",
            "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_spelled_major_dot_minor_and_parses_back() {
        assert_eq!(PROTOCOL_VERSION.to_string(), "0.2");
        assert_eq!("0.2".parse(), Ok(PROTOCOL_VERSION));
        assert_eq!("0.1".parse(), Ok(ProtocolVersion::new(0, 1)));
        assert_eq!("12.30".parse(), Ok(ProtocolVersion::new(12, 30)));
        for refused in ["", "1", "0.01", "+0.1", "0.1.2", "0.x", " 0.1", "00.1"] {
            assert!(
                refused.parse::<ProtocolVersion>().is_err(),
                "`{refused}` must not parse"
            );
        }
    }

    #[test]
    fn versions_order_by_major_then_minor() {
        assert!(ProtocolVersion::new(0, 9) < ProtocolVersion::new(0, 10));
        assert!(ProtocolVersion::new(0, 10) < ProtocolVersion::new(1, 0));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn a_version_serializes_as_its_string() {
        assert_eq!(
            serde_json::to_value(PROTOCOL_VERSION).ok(),
            Some(serde_json::json!("0.2"))
        );
        assert_eq!(
            serde_json::from_value::<ProtocolVersion>(serde_json::json!("0.2")).ok(),
            Some(PROTOCOL_VERSION)
        );
        assert!(serde_json::from_value::<ProtocolVersion>(serde_json::json!(0.1)).is_err());
    }
}
