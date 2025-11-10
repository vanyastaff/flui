//! Interned asset keys for efficient hashing and comparison.

use lasso::{Rodeo, Spur};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Global string interner for asset keys.
///
/// This uses `lasso` for efficient string interning. Strings are stored once
/// and referenced by a 32-bit integer (`Spur`), making keys only 4 bytes.
static INTERNER: Lazy<RwLock<Rodeo>> = Lazy::new(|| RwLock::new(Rodeo::new()));

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
    /// # Examples
    ///
    /// ```
    /// use flui_assets::AssetKey;
    ///
    /// let key = AssetKey::new("textures/wall.png");
    /// ```
    #[inline]
    pub fn new(s: &str) -> Self {
        let mut interner = INTERNER.write();
        Self(interner.get_or_intern(s))
    }

    /// Returns the string value of this key.
    ///
    /// This performs a lookup in the global interner and clones the string.
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
    pub fn as_str(&self) -> String {
        let interner = INTERNER.read();
        interner.resolve(&self.0).to_string()
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
        write!(f, "{}", self.as_str())
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
    fn test_key_creation() {
        let key1 = AssetKey::new("test.png");
        let key2 = AssetKey::new("test.png");

        // Same string should produce same key
        assert_eq!(key1, key2);
        assert_eq!(key1.as_str(), "test.png");
    }

    #[test]
    fn test_key_different_strings() {
        let key1 = AssetKey::new("test1.png");
        let key2 = AssetKey::new("test2.png");

        // Different strings should produce different keys
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_key_size() {
        // Verify that AssetKey is only 4 bytes
        assert_eq!(std::mem::size_of::<AssetKey>(), 4);
    }

    #[test]
    fn test_key_hash() {
        let key1 = AssetKey::new("test.png");
        let key2 = AssetKey::new("test.png");

        // Same keys should have same hash
        let mut set = HashSet::new();
        set.insert(key1);
        assert!(set.contains(&key2));
    }

    #[test]
    fn test_key_display() {
        let key = AssetKey::new("image.png");
        assert_eq!(format!("{}", key), "image.png");
    }

    #[test]
    fn test_key_from_string() {
        let s = "test.png".to_string();
        let key: AssetKey = s.into();
        assert_eq!(key.as_str(), "test.png");
    }

    #[test]
    fn test_key_clone_copy() {
        let key1 = AssetKey::new("test.png");
        let key2 = key1; // Should be Copy

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_multiple_keys_interned() {
        // Test that many different strings can be interned
        let keys: Vec<_> = (0..1000)
            .map(|i| AssetKey::new(&format!("asset_{}.png", i)))
            .collect();

        // Each should be unique
        let unique: HashSet<_> = keys.iter().copied().collect();
        assert_eq!(unique.len(), 1000);
    }

    #[test]
    fn test_key_as_u32() {
        let key = AssetKey::new("test.png");
        let id = key.as_u32();

        // Should be a valid u32
        assert!(id > 0);
    }
}
