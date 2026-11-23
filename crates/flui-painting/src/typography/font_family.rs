//! Font family names a text style asks for.
//!
//! A name is shared by every style that copies it: a copy costs a reference count, not an
//! allocation ([`SharedSlice`](super::SharedSlice) gives a style's fallback list the same
//! sharing). A style travels with the painter that lays it out, which may move between
//! threads, so the sharing is `Arc`.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// The name of a font family, as a font collection spells it (`"Roboto"`), or a CSS generic
/// (`"sans-serif"`, `"monospace"`).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct FontFamilyName(Arc<str>);

impl FontFamilyName {
    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The shared name, for a consumer that keeps it as an `Arc<str>`.
    #[must_use]
    pub fn as_arc(&self) -> &Arc<str> {
        &self.0
    }
}

impl Deref for FontFamilyName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for FontFamilyName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for FontFamilyName {
    fn from(name: &str) -> Self {
        Self(Arc::from(name))
    }
}

impl From<String> for FontFamilyName {
    fn from(name: String) -> Self {
        Self(Arc::from(name))
    }
}

impl From<&String> for FontFamilyName {
    fn from(name: &String) -> Self {
        Self(Arc::from(name.as_str()))
    }
}

impl From<Arc<str>> for FontFamilyName {
    fn from(name: Arc<str>) -> Self {
        Self(name)
    }
}

impl PartialEq<str> for FontFamilyName {
    fn eq(&self, other: &str) -> bool {
        *self.0 == *other
    }
}

impl PartialEq<&str> for FontFamilyName {
    fn eq(&self, other: &&str) -> bool {
        *self.0 == **other
    }
}

impl fmt::Debug for FontFamilyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl fmt::Display for FontFamilyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
