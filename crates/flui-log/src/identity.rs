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
//!   after.
//!
//! A display name cannot be turned into an identifier. The historical code
//! built `com.{display_name}.app`, which produced entries such as
//! `com.My Game.app` (not a legal identifier) and, worse, could collide with a
//! reverse-DNS name owned by somebody else. FLUI therefore keeps them as
//! separate fields: the identifier is supplied by the application or it is
//! absent, and absence resolves to one fixed, obviously-unclaimed subsystem and
//! to no default data directory rather than to a guess.

use std::fmt;

use thiserror::Error;

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

/// The longest [`AppId`], in bytes: the longest file name the desktop file
/// systems accept, since the identifier names the data directory.
pub const APP_ID_MAX_LEN: usize = 255;

/// Words Java reserves, which Android refuses as a package-name label.
const JAVA_RESERVED: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "void",
    "volatile",
    "while",
];

/// Whether `label` is a name Windows reserves for a device in every
/// directory, in any case: the first label names the data directory, and
/// Windows refuses `con.example` as a file name as it refuses `con`.
fn is_windows_device(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    match label.as_bytes() {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', digit] | [b'l', b'p', b't', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

/// Reasons an [`AppIdentity`] or [`AppId`] cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum IdentityError {
    /// An application identifier needs at least two dot-separated labels.
    #[error(
        "application identifier `{0}` is not reverse-DNS; it needs at least two dot-separated \
         labels, for example `com.example.app`"
    )]
    NotReverseDns(String),

    /// A label between two dots (or at either end) was empty.
    #[error("application identifier `{identifier}` has an empty label at position {position}")]
    EmptyLabel {
        /// The rejected identifier.
        identifier: String,
        /// Zero-based index of the empty label.
        position: usize,
    },

    /// A label contained something other than an ASCII letter, digit, or `_`.
    #[error(
        "application identifier `{identifier}` contains {character:?}; labels accept ASCII \
         letters, digits, and `_`"
    )]
    InvalidCharacter {
        /// The rejected identifier.
        identifier: String,
        /// The first character that was not accepted.
        character: char,
    },

    /// A label started with something other than an ASCII letter.
    #[error(
        "application identifier `{identifier}` has label {position} starting with something \
         other than an ASCII letter"
    )]
    LabelStartsWithNonLetter {
        /// The rejected identifier.
        identifier: String,
        /// Zero-based index of the label.
        position: usize,
    },

    /// A label was a word Java reserves, which Android refuses in a package
    /// name, or the first label was a name Windows reserves for a device
    /// (`con`, `aux`, `com1`, any case), which cannot name the data directory.
    #[error("application identifier `{identifier}` uses the reserved word `{label}` as a label")]
    ReservedLabel {
        /// The rejected identifier.
        identifier: String,
        /// The reserved label.
        label: String,
    },

    /// The identifier was longer than [`APP_ID_MAX_LEN`] bytes.
    #[error("application identifier `{0}` is longer than {APP_ID_MAX_LEN} bytes")]
    TooLong(String),

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

/// A validated reverse-DNS application identifier, such as
/// `com.example.notes`: the name the application is known by on every
/// platform.
///
/// The grammar is the one every target accepts as a name for the
/// application: two or more `.`-separated labels, each starting with an ASCII
/// letter and continuing with ASCII letters, digits and `_`, none a word Java
/// reserves, at most [`APP_ID_MAX_LEN`] bytes in all. That is a legal Android
/// package name, an Apple unified-logging subsystem and a directory name on
/// every file system. It is what `flui build` derives from `[app]` in
/// `flui.toml` (`organization` plus `name`).
///
/// Parsed, not merely checked: once constructed, the value is known to be
/// legal everywhere it is used, so no consumer re-inspects it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppId(String);

impl AppId {
    /// Parse a reverse-DNS application identifier such as `com.example.app`.
    ///
    /// # Errors
    ///
    /// Returns the [`IdentityError`] variant naming what was wrong: too few
    /// labels, an empty label, a label not starting with a letter, a
    /// character outside the accepted set, a reserved word, or too many bytes.
    pub fn try_new(identifier: impl Into<String>) -> Result<Self, IdentityError> {
        let identifier = identifier.into();
        if identifier.len() > APP_ID_MAX_LEN {
            return Err(IdentityError::TooLong(identifier));
        }

        let labels: Vec<&str> = identifier.split('.').collect();
        if labels.len() < 2 {
            return Err(IdentityError::NotReverseDns(identifier));
        }

        for (position, label) in labels.iter().enumerate() {
            let Some(first) = label.chars().next() else {
                return Err(IdentityError::EmptyLabel {
                    identifier,
                    position,
                });
            };
            if let Some(character) = label
                .chars()
                .find(|character| !character.is_ascii_alphanumeric() && *character != '_')
            {
                return Err(IdentityError::InvalidCharacter {
                    identifier,
                    character,
                });
            }
            if !first.is_ascii_alphabetic() {
                return Err(IdentityError::LabelStartsWithNonLetter {
                    identifier,
                    position,
                });
            }
            if JAVA_RESERVED.contains(label) || (position == 0 && is_windows_device(label)) {
                let label = (*label).to_owned();
                return Err(IdentityError::ReservedLabel { identifier, label });
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

impl fmt::Display for AppId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
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
