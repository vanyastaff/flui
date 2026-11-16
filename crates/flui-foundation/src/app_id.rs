//! The application identifier: the one name an application is known by on
//! every platform, parsed once.
//!
//! The application's composition root learns it (from `FLUI_APP_ID`, which
//! every `flui` command compiles in from the `[app] id` of `flui.toml`, and
//! only from there) and hands the same value to each consumer: the storage
//! host names the data directory after it, Apple's unified logging files
//! events under it, a desktop host names its single instance by it, and a
//! Linux window carries it as its class and app id, which the desktop entry
//! installed as `<id>.desktop` matches. A consumer takes an [`AppId`], never a
//! string it would have to check again.
//!
//! An application without one is known on Linux by its executable's name, as
//! [`executable_class`] writes it, the same in its windows and in its desktop
//! entry.

use std::fmt;

use thiserror::Error;

/// The longest [`AppId`], in bytes: the tightest name the id is part of, the
/// Windows single-instance pipe `\\.\pipe\flui-<id>-<session>` (256
/// characters at most, a session of up to ten digits). Every shorter
/// identifier also fits the instance's mutex, a D-Bus well-known name (255),
/// a data directory and the Linux desktop entry `<id>.desktop` (255 each).
pub const APP_ID_MAX_LEN: usize = 256 - r"\\.\pipe\flui-".len() - "-4294967295".len();

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

/// Why a string is not an [`AppId`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AppIdError {
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
}

/// A validated reverse-DNS application identifier, such as
/// `com.example.notes`: the name the application is known by on every
/// platform.
///
/// The grammar is the one every target accepts as a name for the
/// application: two or more `.`-separated labels, each starting with an ASCII
/// letter and continuing with ASCII letters, digits and `_`, none a word Java
/// reserves, at most [`APP_ID_MAX_LEN`] bytes in all. That is a legal Android
/// package name, an Apple unified-logging subsystem, a directory name on every
/// file system, a D-Bus well-known name and a freedesktop desktop-file ID. It
/// is what `flui build` derives from `[app]` in `flui.toml` (`organization`
/// plus `name`).
///
/// Parsed, not merely checked: once constructed, the value is known to be
/// legal everywhere it is used, so no consumer re-inspects it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppId(Box<str>);

impl AppId {
    /// Parse a reverse-DNS application identifier such as `com.example.app`.
    ///
    /// # Errors
    ///
    /// Returns the [`AppIdError`] variant naming what was wrong: too few
    /// labels, an empty label, a label not starting with a letter, a
    /// character outside the accepted set, a reserved word, or too many bytes.
    pub fn try_new(identifier: impl Into<String>) -> Result<Self, AppIdError> {
        let identifier = identifier.into();
        if identifier.len() > APP_ID_MAX_LEN {
            return Err(AppIdError::TooLong(identifier));
        }
        if !identifier.contains('.') {
            return Err(AppIdError::NotReverseDns(identifier));
        }
        let refusal = identifier
            .split('.')
            .enumerate()
            .find_map(|(position, label)| label_refusal(position, label));
        match refusal {
            None => Ok(Self(identifier.into_boxed_str())),
            Some(refusal) => Err(refusal.of(identifier)),
        }
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

/// The name an application without an [`AppId`] is known by on Linux, from
/// its executable's file stem `stem`: each character outside ASCII letters,
/// digits, `-`, `_` and `.` becomes `_`, the result is cut to
/// [`APP_ID_MAX_LEN`] bytes, and an empty stem is `flui`. Its windows carry it
/// as their Wayland app id and X11 `WM_CLASS` (an X11 `STRING` is Latin-1, so
/// only ASCII reads back the same on every side), and `flui build linux`
/// names the desktop entry after it, so both sides write the same bytes.
#[must_use]
pub fn executable_class(stem: &str) -> String {
    let mut class: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect();
    class.truncate(APP_ID_MAX_LEN);
    if class.is_empty() {
        class.push_str("flui");
    }
    class
}

/// What is wrong with one label, before the identifier it belongs to is
/// moved into the error.
enum LabelRefusal {
    Empty(usize),
    Character(char),
    StartsWithNonLetter(usize),
    Reserved(String),
}

impl LabelRefusal {
    fn of(self, identifier: String) -> AppIdError {
        match self {
            Self::Empty(position) => AppIdError::EmptyLabel {
                identifier,
                position,
            },
            Self::Character(character) => AppIdError::InvalidCharacter {
                identifier,
                character,
            },
            Self::StartsWithNonLetter(position) => AppIdError::LabelStartsWithNonLetter {
                identifier,
                position,
            },
            Self::Reserved(label) => AppIdError::ReservedLabel { identifier, label },
        }
    }
}

/// Why the label at `position` cannot be one of an [`AppId`], if it cannot.
fn label_refusal(position: usize, label: &str) -> Option<LabelRefusal> {
    let Some(first) = label.chars().next() else {
        return Some(LabelRefusal::Empty(position));
    };
    if let Some(character) = label
        .chars()
        .find(|character| !character.is_ascii_alphanumeric() && *character != '_')
    {
        return Some(LabelRefusal::Character(character));
    }
    if !first.is_ascii_alphabetic() {
        return Some(LabelRefusal::StartsWithNonLetter(position));
    }
    if JAVA_RESERVED.contains(&label) || (position == 0 && is_windows_device(label)) {
        return Some(LabelRefusal::Reserved(label.to_owned()));
    }
    None
}
