//! OpenType settings a text style hands the shaper: feature switches
//! ([`FontFeature`]) and variable-font axis positions ([`FontVariation`]),
//! each named by an [`OpenTypeTag`].

use std::fmt;
use std::str::FromStr;

/// A four-byte OpenType tag, such as `smcp` or `wght`.
///
/// Each byte is printable ASCII (space through `~`), as the OpenType spec
/// requires, so a tag the shaper would refuse never reaches it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(try_from = "String", into = "String")
)]
pub struct OpenTypeTag([u8; 4]);

/// A tag that is not four printable ASCII bytes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("an OpenType tag is four printable ASCII bytes, not {0:?}")]
pub struct InvalidOpenTypeTag(String);

impl OpenTypeTag {
    /// The weight axis of a variable font.
    pub const WEIGHT: Self = Self(*b"wght");
    /// The width axis of a variable font.
    pub const WIDTH: Self = Self(*b"wdth");
    /// The slant axis of a variable font.
    pub const SLANT: Self = Self(*b"slnt");
    /// The italic axis of a variable font.
    pub const ITALIC: Self = Self(*b"ital");
    /// The optical-size axis of a variable font.
    pub const OPTICAL_SIZE: Self = Self(*b"opsz");

    /// The tag `bytes`, such as `OpenTypeTag::new(*b"smcp")`.
    ///
    /// # Panics
    ///
    /// When a byte is not printable ASCII; [`OpenTypeTag::try_new`] is the
    /// fallible twin. In a `const` the check runs at compile time.
    #[must_use]
    pub const fn new(bytes: [u8; 4]) -> Self {
        match Self::try_from_bytes(bytes) {
            Some(tag) => tag,
            None => panic!("an OpenType tag is four printable ASCII bytes; use try_new"),
        }
    }

    /// The tag spelled by `tag`.
    ///
    /// # Errors
    ///
    /// [`InvalidOpenTypeTag`] when `tag` is not four printable ASCII bytes.
    pub fn try_new(tag: &str) -> Result<Self, InvalidOpenTypeTag> {
        <[u8; 4]>::try_from(tag.as_bytes())
            .ok()
            .and_then(Self::try_from_bytes)
            .ok_or_else(|| InvalidOpenTypeTag(tag.to_owned()))
    }

    /// The tag's four bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 4] {
        self.0
    }

    /// The tag as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("BUG: an OpenTypeTag holds printable ASCII")
    }

    const fn try_from_bytes(bytes: [u8; 4]) -> Option<Self> {
        let mut i = 0;
        while i < 4 {
            if !matches!(bytes[i], b' '..=b'~') {
                return None;
            }
            i += 1;
        }
        Some(Self(bytes))
    }
}

impl fmt::Display for OpenTypeTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OpenTypeTag {
    type Err = InvalidOpenTypeTag;

    fn from_str(tag: &str) -> Result<Self, Self::Err> {
        Self::try_new(tag)
    }
}

impl TryFrom<String> for OpenTypeTag {
    type Error = InvalidOpenTypeTag;

    fn try_from(tag: String) -> Result<Self, Self::Error> {
        Self::try_new(&tag)
    }
}

impl From<OpenTypeTag> for String {
    fn from(tag: OpenTypeTag) -> Self {
        tag.as_str().to_owned()
    }
}

/// An OpenType feature setting, such as `smcp` for small caps, applied
/// during shaping.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FontFeature {
    /// The feature's tag.
    pub tag: OpenTypeTag,
    /// The feature's value: 0 turns it off, 1 on, and a larger value picks
    /// an alternate.
    pub value: u16,
}

impl FontFeature {
    /// The feature `tag` set to `value`.
    #[inline]
    #[must_use]
    pub const fn new(tag: OpenTypeTag, value: u16) -> Self {
        Self { tag, value }
    }

    /// The feature `tag` turned on.
    #[inline]
    #[must_use]
    pub const fn enable(tag: OpenTypeTag) -> Self {
        Self::new(tag, 1)
    }

    /// The feature `tag` turned off.
    #[inline]
    #[must_use]
    pub const fn disable(tag: OpenTypeTag) -> Self {
        Self::new(tag, 0)
    }
}

/// A variable-font axis position, such as `wght` 550, applied during
/// shaping.
///
/// Shaping ignores a value that is not finite in the shaper's `f32`
/// coordinate space; an axis the font lacks is left to the font.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FontVariation {
    /// The axis's tag.
    pub axis: OpenTypeTag,
    /// The position on the axis, in the axis's own units.
    pub value: f64,
}

impl FontVariation {
    /// The axis `axis` at `value`.
    #[inline]
    #[must_use]
    pub const fn new(axis: OpenTypeTag, value: f64) -> Self {
        Self { axis, value }
    }
}
