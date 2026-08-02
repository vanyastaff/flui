//! Application identity used by the native log sinks.
//!
//! Two different things are called "the app name" by the platform log systems,
//! and conflating them is how the historical backend produced identifiers that
//! looked official and were not:
//!
//! * a **display name** — free-form, human-facing, used as the logcat tag when
//!   an event carries no usable target;
//! * a **bundle identifier** — a reverse-DNS name that Apple's unified logging
//!   indexes as the *subsystem*, and that `log stream --predicate` queries by.
//!
//! A display name cannot be turned into a bundle identifier. The historical
//! code built `com.{display_name}.app`, which produced entries such as
//! `com.My Game.app` (not a legal identifier) and, worse, could collide with a
//! reverse-DNS name owned by somebody else. FLUI therefore keeps them as
//! separate fields: the bundle identifier is supplied by the application or it
//! is absent, and absence resolves to one fixed, obviously-unclaimed subsystem
//! rather than to a guess.

use std::fmt;

use thiserror::Error;

/// Subsystem reported to Apple's unified logging when the application has not
/// declared its real bundle identifier.
///
/// Deliberately a single fixed value rather than a name derived from the
/// display name: an unconfigured application should be easy to spot in
/// `log stream`, and must never be filed under an identifier that belongs to
/// somebody else.
pub const UNIDENTIFIED_APPLE_SUBSYSTEM: &str = "dev.flui.unidentified";

/// Category reported alongside the subsystem to Apple's unified logging.
///
/// Stable by contract: log predicates written against a FLUI application keep
/// working across releases.
pub const APPLE_CATEGORY: &str = "flui";

/// Display name used when the application supplies none.
pub const DEFAULT_DISPLAY_NAME: &str = "flui";

/// Reasons an [`AppIdentity`] or [`AppleBundleId`] cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IdentityError {
    /// A bundle identifier needs at least two dot-separated labels.
    #[error(
        "bundle identifier `{0}` is not reverse-DNS; it needs at least two dot-separated labels, \
         for example `com.example.app`"
    )]
    NotReverseDns(String),

    /// A label between two dots (or at either end) was empty.
    #[error("bundle identifier `{identifier}` has an empty label at position {position}")]
    EmptyLabel {
        /// The rejected identifier.
        identifier: String,
        /// Zero-based index of the empty label.
        position: usize,
    },

    /// A label contained something other than an ASCII letter, digit, or `-`.
    #[error(
        "Apple bundle identifier `{identifier}` contains {character:?}; labels accept ASCII \
         letters, digits, and `-`"
    )]
    InvalidCharacter {
        /// The rejected identifier.
        identifier: String,
        /// The first character that was not accepted.
        character: char,
    },

    /// A label started or ended with `-`.
    #[error("Apple bundle identifier `{0}` has a label starting or ending with `-`")]
    HyphenAtLabelEdge(String),

    /// The display name was empty or only whitespace.
    #[error("application display name is empty")]
    EmptyDisplayName,

    /// The display name held an interior NUL byte.
    ///
    /// The logcat tag crosses a C boundary, so an interior NUL would silently
    /// truncate the tag rather than fail.
    #[error("application display name {0:?} contains an interior NUL and cannot be a logcat tag")]
    DisplayNameHasNul(String),
}

/// A validated reverse-DNS bundle identifier.
///
/// Parsed, not merely checked: once constructed, the value is known to be legal
/// as an Apple subsystem, so the Apple backend never has to re-inspect it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppleBundleId(String);

impl AppleBundleId {
    /// Parse a reverse-DNS bundle identifier such as `com.example.app`.
    ///
    /// Accepts two or more `.`-separated labels of ASCII letters, digits, and
    /// interior hyphens. Android package identifiers intentionally use a
    /// different type once a consumer needs them; platform grammars are not
    /// forced into an artificial intersection.
    ///
    /// # Errors
    ///
    /// Returns the [`IdentityError`] variant naming what was wrong: too few
    /// labels, an empty label, a character outside the accepted set, or a
    /// hyphen at a label boundary.
    pub fn new(identifier: impl Into<String>) -> Result<Self, IdentityError> {
        let identifier = identifier.into();

        let labels: Vec<&str> = identifier.split('.').collect();
        if labels.len() < 2 {
            return Err(IdentityError::NotReverseDns(identifier));
        }

        for (position, label) in labels.iter().enumerate() {
            if label.is_empty() {
                return Err(IdentityError::EmptyLabel {
                    identifier,
                    position,
                });
            }
            if let Some(character) = label
                .chars()
                .find(|character| !character.is_ascii_alphanumeric() && *character != '-')
            {
                return Err(IdentityError::InvalidCharacter {
                    identifier,
                    character,
                });
            }
            if label.starts_with('-') || label.ends_with('-') {
                return Err(IdentityError::HyphenAtLabelEdge(identifier));
            }
        }

        Ok(Self(identifier))
    }

    /// The identifier as a string slice.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppleBundleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// How an application names itself to the platform log systems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    display_name: String,
    apple_bundle_id: Option<AppleBundleId>,
}

impl Default for AppIdentity {
    fn default() -> Self {
        Self {
            display_name: DEFAULT_DISPLAY_NAME.to_owned(),
            apple_bundle_id: None,
        }
    }
}

impl AppIdentity {
    /// Build an identity from a human-facing display name.
    ///
    /// The display name is the logcat fallback tag. It is **not** promoted to a
    /// bundle identifier — call [`AppIdentity::with_apple_bundle_id`] for that.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::EmptyDisplayName`] for a blank name, and
    /// [`IdentityError::DisplayNameHasNul`] for one holding an interior NUL,
    /// which would silently truncate the logcat tag at the C boundary.
    pub fn new(display_name: impl Into<String>) -> Result<Self, IdentityError> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() {
            return Err(IdentityError::EmptyDisplayName);
        }
        if display_name.contains('\0') {
            return Err(IdentityError::DisplayNameHasNul(display_name));
        }
        Ok(Self {
            display_name,
            apple_bundle_id: None,
        })
    }

    /// Attach the application's real reverse-DNS bundle identifier.
    #[must_use]
    pub fn with_apple_bundle_id(mut self, bundle_id: AppleBundleId) -> Self {
        self.apple_bundle_id = Some(bundle_id);
        self
    }

    /// The human-facing display name.
    #[inline]
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The declared bundle identifier, if the application supplied one.
    #[inline]
    #[must_use]
    pub fn apple_bundle_id(&self) -> Option<&AppleBundleId> {
        self.apple_bundle_id.as_ref()
    }

    /// Subsystem reported to Apple's unified logging.
    ///
    /// The declared bundle identifier when there is one, otherwise the fixed
    /// [`UNIDENTIFIED_APPLE_SUBSYSTEM`]. Never a value synthesised from the
    /// display name.
    #[inline]
    #[must_use]
    pub fn apple_subsystem(&self) -> &str {
        self.apple_bundle_id
            .as_ref()
            .map_or(UNIDENTIFIED_APPLE_SUBSYSTEM, AppleBundleId::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_reverse_dns() {
        let bundle_id =
            AppleBundleId::new("com.example.app").expect("`com.example.app` is reverse-DNS");
        assert_eq!(bundle_id.as_str(), "com.example.app");
    }

    #[test]
    fn accepts_two_labels_digits_and_interior_hyphen() {
        assert!(AppleBundleId::new("dev.flui").is_ok());
        assert!(AppleBundleId::new("com.example2.my-app").is_ok());
    }

    #[test]
    fn accepts_a_digit_initial_label_apple_allows() {
        assert!(AppleBundleId::new("com.2example.app").is_ok());
    }

    #[test]
    fn rejects_an_underscore() {
        // Not part of Apple's bundle-identifier character set.
        assert_eq!(
            AppleBundleId::new("com.example.my_app"),
            Err(IdentityError::InvalidCharacter {
                identifier: "com.example.my_app".to_owned(),
                character: '_',
            })
        );
    }

    #[test]
    fn rejects_single_label() {
        assert_eq!(
            AppleBundleId::new("flui"),
            Err(IdentityError::NotReverseDns("flui".to_owned()))
        );
    }

    #[test]
    fn rejects_empty_label() {
        assert_eq!(
            AppleBundleId::new("com..app"),
            Err(IdentityError::EmptyLabel {
                identifier: "com..app".to_owned(),
                position: 1,
            })
        );
    }

    #[test]
    fn rejects_space_the_display_name_would_have_introduced() {
        // The exact shape the historical `com.{display_name}.app` synthesis
        // produced for a display name like "My Game".
        assert_eq!(
            AppleBundleId::new("com.My Game.app"),
            Err(IdentityError::InvalidCharacter {
                identifier: "com.My Game.app".to_owned(),
                character: ' ',
            })
        );
    }

    #[test]
    fn rejects_hyphen_at_label_edge() {
        assert_eq!(
            AppleBundleId::new("com.-example.app"),
            Err(IdentityError::HyphenAtLabelEdge(
                "com.-example.app".to_owned()
            ))
        );
    }

    #[test]
    fn display_name_is_not_promoted_to_a_subsystem() {
        let identity = AppIdentity::new("My Game").expect("a display name may contain spaces");
        assert_eq!(identity.display_name(), "My Game");
        assert_eq!(identity.apple_bundle_id(), None);
        assert_eq!(identity.apple_subsystem(), UNIDENTIFIED_APPLE_SUBSYSTEM);
    }

    #[test]
    fn declared_bundle_id_becomes_the_subsystem() {
        let identity = AppIdentity::new("My Game")
            .expect("a display name may contain spaces")
            .with_apple_bundle_id(AppleBundleId::new("com.example.mygame").expect("reverse-DNS"));
        assert_eq!(identity.apple_subsystem(), "com.example.mygame");
    }

    #[test]
    fn rejects_empty_and_nul_display_names() {
        assert_eq!(
            AppIdentity::new("   "),
            Err(IdentityError::EmptyDisplayName)
        );
        assert_eq!(
            AppIdentity::new("a\0b"),
            Err(IdentityError::DisplayNameHasNul("a\0b".to_owned()))
        );
    }

    #[test]
    fn default_identity_is_unidentified() {
        let identity = AppIdentity::default();
        assert_eq!(identity.display_name(), DEFAULT_DISPLAY_NAME);
        assert_eq!(identity.apple_subsystem(), UNIDENTIFIED_APPLE_SUBSYSTEM);
    }
}
