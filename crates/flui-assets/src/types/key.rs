//! Owned asset names with shared string storage.

use std::fmt;
use std::sync::Arc;

/// A nonempty asset name with shared ownership.
///
/// Equality and hashing use string contents: independently constructed keys
/// with the same name identify the same requested asset. Clones share their
/// string allocation; unrelated names do not share a retaining arena.
///
/// # Examples
///
/// ```
/// use flui_assets::AssetKey;
///
/// let key1 = AssetKey::new("logo.png");
/// let key2 = AssetKey::new("logo.png");
/// assert_eq!(key1, key2);
/// assert_eq!(key1.as_str(), "logo.png");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetKey(Arc<str>);

impl AssetKey {
    /// Creates a key owning the given name.
    ///
    /// # Panics
    ///
    /// Panics if the string is empty.
    ///
    /// ```should_panic
    /// use flui_assets::AssetKey;
    /// let key = AssetKey::new("");
    /// ```
    #[inline]
    pub fn new(name: &str) -> Self {
        Self::from(Arc::<str>::from(name))
    }

    /// Borrows the name for the lifetime of this key.
    ///
    /// ```
    /// use flui_assets::AssetKey;
    /// let key = AssetKey::new("icon.png");
    /// assert_eq!(key.as_str(), "icon.png");
    /// ```
    ///
    /// The borrowed name cannot outlive its owner:
    ///
    /// ```compile_fail
    /// use flui_assets::AssetKey;
    /// fn leaked_name() -> &'static str {
    ///     AssetKey::new("temporary.png").as_str()
    /// }
    /// ```
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AssetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<Arc<str>> for AssetKey {
    /// Keeps the existing shared allocation without copying its contents.
    ///
    /// # Panics
    ///
    /// Panics if the string is empty.
    fn from(name: Arc<str>) -> Self {
        assert!(!name.is_empty(), "Asset key cannot be empty");
        Self(name)
    }
}

impl From<&str> for AssetKey {
    #[inline]
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for AssetKey {
    #[inline]
    fn from(name: String) -> Self {
        Self::from(Arc::<str>::from(name))
    }
}

impl From<&String> for AssetKey {
    #[inline]
    fn from(name: &String) -> Self {
        Self::new(name)
    }
}
