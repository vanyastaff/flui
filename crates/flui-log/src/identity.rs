//! Application identity used by the native log sinks and the storage host.
//!
//! Two different things are called "the app name" by the platforms, and
//! conflating them is how the historical backend produced identifiers that
//! looked official and were not:
//!
//! * a **display name** — free-form, human-facing, used as the logcat tag when
//!   an event carries no usable target;
//! * an **application identifier** — a reverse-DNS name such as
//!   `com.example.notes`, which Apple's unified logging indexes as the
//!   *subsystem* and the storage host names the application's data directory
//!   after ([`AppId`], whose grammar `flui-foundation` holds for every
//!   consumer).
//!
//! A display name cannot be turned into an identifier. The historical code
//! built `com.{display_name}.app`, which produced entries such as
//! `com.My Game.app` (not a legal identifier) and, worse, could collide with a
//! reverse-DNS name owned by somebody else. FLUI therefore keeps them as
//! separate fields: the identifier is supplied by the application or it is
//! absent, and absence resolves to one fixed, obviously-unclaimed subsystem and
//! to no default data directory rather than to a guess.

use thiserror::Error;

use flui_foundation::AppId;

/// Subsystem reported to Apple's unified logging when the application has not
/// declared its identifier.
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

/// Why a display name cannot name an [`AppIdentity`]; an identifier's own
/// refusals are `flui_foundation::AppIdError`'s.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IdentityError {
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

/// How an application names itself to the platform log systems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    display_name: String,
    app_id: Option<AppId>,
}

impl Default for AppIdentity {
    fn default() -> Self {
        Self {
            display_name: DEFAULT_DISPLAY_NAME.to_owned(),
            app_id: None,
        }
    }
}

impl AppIdentity {
    /// Build an identity from a human-facing display name.
    ///
    /// The display name is the logcat fallback tag. It is **not** promoted to a
    /// application identifier — call [`AppIdentity::with_app_id`] for that.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::EmptyDisplayName`] for a blank name, and
    /// [`IdentityError::DisplayNameHasNul`] for one holding an interior NUL,
    /// which would silently truncate the logcat tag at the C boundary.
    pub fn try_new(display_name: impl Into<String>) -> Result<Self, IdentityError> {
        let display_name = display_name.into();
        if display_name.trim().is_empty() {
            return Err(IdentityError::EmptyDisplayName);
        }
        if display_name.contains('\0') {
            return Err(IdentityError::DisplayNameHasNul(display_name));
        }
        Ok(Self {
            display_name,
            app_id: None,
        })
    }

    /// Attach the application's identifier.
    #[must_use]
    pub fn with_app_id(mut self, app_id: AppId) -> Self {
        self.app_id = Some(app_id);
        self
    }

    /// The human-facing display name.
    #[inline]
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The declared application identifier, if the application supplied one.
    #[inline]
    #[must_use]
    pub fn app_id(&self) -> Option<&AppId> {
        self.app_id.as_ref()
    }

    /// Subsystem reported to Apple's unified logging.
    ///
    /// The declared application identifier when there is one, otherwise the
    /// fixed [`UNIDENTIFIED_APPLE_SUBSYSTEM`]. Never a value synthesised from
    /// the display name.
    #[inline]
    #[must_use]
    pub fn apple_subsystem(&self) -> &str {
        self.app_id
            .as_ref()
            .map_or(UNIDENTIFIED_APPLE_SUBSYSTEM, AppId::as_str)
    }
}
