//! Font family names a text style asks for.
//!
//! A style is copied wherever it is inherited, merged or resolved (each `DefaultTextStyle`
//! read, each paragraph a theme styles), so its names are shared rather than owned: a copy of
//! either type below costs a reference count, not an allocation. A style travels with the
//! painter that lays it out, which may move between threads, so the sharing is `Arc`.

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

/// The families a style falls back to, in order, after its own: an immutable shared list.
/// The empty list holds no allocation.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(from = "Vec<FontFamilyName>", into = "Vec<FontFamilyName>")
)]
pub struct FontFamilyFallback(Option<Arc<[FontFamilyName]>>);

impl FontFamilyFallback {
    /// The empty list.
    #[must_use]
    pub const fn new() -> Self {
        Self(None)
    }

    /// The families, in order.
    #[must_use]
    pub fn as_slice(&self) -> &[FontFamilyName] {
        self.0.as_deref().unwrap_or_default()
    }
}

impl Deref for FontFamilyFallback {
    type Target = [FontFamilyName];

    fn deref(&self) -> &[FontFamilyName] {
        self.as_slice()
    }
}

impl<N: Into<FontFamilyName>> FromIterator<N> for FontFamilyFallback {
    fn from_iter<I: IntoIterator<Item = N>>(names: I) -> Self {
        let names: Vec<FontFamilyName> = names.into_iter().map(Into::into).collect();
        Self::from(names)
    }
}

impl From<Vec<FontFamilyName>> for FontFamilyFallback {
    fn from(names: Vec<FontFamilyName>) -> Self {
        if names.is_empty() {
            Self(None)
        } else {
            Self(Some(Arc::from(names)))
        }
    }
}

impl<N: Into<FontFamilyName>, const LEN: usize> From<[N; LEN]> for FontFamilyFallback {
    fn from(names: [N; LEN]) -> Self {
        names.into_iter().collect()
    }
}

impl From<FontFamilyFallback> for Vec<FontFamilyName> {
    fn from(fallback: FontFamilyFallback) -> Self {
        fallback.as_slice().to_vec()
    }
}

impl<'a> IntoIterator for &'a FontFamilyFallback {
    type Item = &'a FontFamilyName;
    type IntoIter = std::slice::Iter<'a, FontFamilyName>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl fmt::Debug for FontFamilyFallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}
