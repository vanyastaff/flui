//! Interned asset keys for efficient hashing and comparison.

use lasso::{Spur, ThreadedRodeo};
use std::fmt;
use std::hash::{Hash, Hasher};

/// Global string interner for asset keys.
///
/// This uses `lasso` for efficient string interning. Strings are stored once
/// and referenced by a 32-bit integer (`Spur`), making keys only 4 bytes.
///
/// `ThreadedRodeo` is internally synchronised (sharded, lock-free reads), so
/// interning from many threads needs no lock of our own — the previous
/// `RwLock<Rodeo>` serialised every `AssetKey::new` behind one write lock.
/// Interned strings are never removed, so a resolved `&str` borrows from the
/// `static` for `'static`.
static INTERNER: std::sync::LazyLock<ThreadedRodeo> = std::sync::LazyLock::new(ThreadedRodeo::new);

/// An interned asset key.
///
/// Asset keys are interned strings that serve as unique identifiers for assets.
/// Interning provides several performance benefits:
///
/// - **Small size**: Only 4 bytes instead of 24+ bytes for `String`
/// - **Fast comparison**: O(1) integer comparison instead of string comparison
/// - **Fast hashing**: Hash a single u32 instead of variable-length string
/// - **Memory efficient**: Identical strings share the same storage
///
/// # Examples
///
/// ```
/// use flui_assets::AssetKey;
///
/// let key1 = AssetKey::new("logo.png");
/// let key2 = AssetKey::new("logo.png");
///
/// // Fast comparison (just compares u32)
/// assert_eq!(key1, key2);
///
/// // Convert back to string when needed
/// assert_eq!(key1.as_str(), "logo.png");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetKey(Spur);

impl AssetKey {
    /// Creates a new asset key by interning the given string.
    ///
    /// If the string has been interned before, this returns the existing key.
    /// Otherwise, the string is added to the global interner.
    ///
    /// # Panics
    ///
    /// Panics if the input string is empty. Asset keys must be non-empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_assets::AssetKey;
    ///
    /// let key = AssetKey::new("textures/wall.png");
    /// ```
    ///
    /// ```should_panic
    /// use flui_assets::AssetKey;
    ///
    /// let key = AssetKey::new(""); // Panics!
    /// ```
    #[inline]
    pub fn new(s: &str) -> Self {
        assert!(!s.is_empty(), "Asset key cannot be empty");
        Self(INTERNER.get_or_intern(s))
    }

    /// Returns the string value of this key.
    ///
    /// This is a lookup in the global interner; no allocation. The result is
    /// `'static` because interned strings are never removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_assets::AssetKey;
    ///
    /// let key = AssetKey::new("icon.png");
    /// assert_eq!(key.as_str(), "icon.png");
    /// ```
    #[inline]
    pub fn as_str(&self) -> &'static str {
        INTERNER.resolve(&self.0)
    }

    /// Returns the internal integer representation.
    ///
    /// This is mainly useful for debugging or advanced use cases.
    #[inline]
    pub fn as_u32(&self) -> u32 {
        self.0.into_inner().get()
    }
}

impl Hash for AssetKey {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Just hash the u32, extremely fast
        self.0.into_inner().get().hash(state);
    }
}

impl fmt::Display for AssetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for AssetKey {
    #[inline]
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for AssetKey {
    #[inline]
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl From<&String> for AssetKey {
    #[inline]
    fn from(s: &String) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_multiple_keys_interned() {
        // Test that many different strings can be interned
        let keys: Vec<_> = (0..1000)
            .map(|i| AssetKey::new(&format!("asset_{i}.png")))
            .collect();

        // Each should be unique
        let unique: HashSet<_> = keys.iter().copied().collect();
        assert_eq!(unique.len(), 1000);
    }

    #[test]
    #[should_panic(expected = "Asset key cannot be empty")]
    fn test_key_empty_string_panics() {
        let _key = AssetKey::new(""); // Should panic
    }
}
