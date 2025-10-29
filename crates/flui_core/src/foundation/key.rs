//! Widget keys with niche optimization and compile-time constant support
//!
//! This module provides the `Key` type for widget identity tracking with:
//! - Compile-time constant keys via FNV-1a hash
//! - Runtime unique key generation via atomic counter
//! - Explicit keys from external IDs
//! - Memory-efficient `Option<Key>` (8 bytes instead of 16)
//!
//! # Examples
//!
//! ```
//! use flui_core::Key;
//!
//! // Compile-time constant key
//! const HEADER_KEY: Key = Key::from_str("app_header");
//!
//! // Runtime unique key
//! let dynamic_key = Key::new();
//!
//! // Explicit key from database ID
//! let user_key = Key::from_u64(user.id).unwrap();
//! ```

use std::fmt;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

/// Widget key with niche optimization
///
/// Thanks to `NonZeroU64`, `Option<Key>` is only 8 bytes instead of 16.
/// This saves memory and improves cache locality when storing many widgets.
///
/// # Memory Layout
///
/// ```text
/// Option<u64>:      [8 bytes data] + [8 bytes discriminant] = 16 bytes
/// Option<Key>:      [8 bytes NonZeroU64] = 8 bytes (0 means None)
/// ```
///
/// # Creation Methods
///
/// 1. **Compile-time constant** - `Key::from_str("name")`
/// 2. **Runtime unique** - `Key::new()`
/// 3. **Explicit ID** - `Key::from_u64(id)`
///
/// # Performance
///
/// - Key comparison: ~1ns (u64 compare)
/// - Hash computation: O(1) (already hashed)
/// - Creation: 0ns (compile-time) or ~5ns (runtime counter)
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Key(NonZeroU64);

impl Key {
    /// Create compile-time constant key from string
    ///
    /// Uses FNV-1a hash algorithm which is const-evaluatable.
    /// The hash is computed at compile time with zero runtime cost.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_core::Key;
    ///
    /// const HEADER: Key = Key::from_str("app_header");
    /// const FOOTER: Key = Key::from_str("app_footer");
    ///
    /// // These are compile-time constants:
    /// assert_eq!(HEADER, Key::from_str("app_header"));
    /// ```
    ///
    /// # Panics
    ///
    /// Never panics - if hash is 0, uses 1 instead.
    #[inline]
    pub const fn from_str(s: &str) -> Self {
        let hash = const_fnv1a_hash(s.as_bytes());
        // Ensure non-zero (use 1 if hash is 0, which is extremely rare)
        let non_zero = if hash == 0 { 1 } else { hash };
        // SAFETY: We just ensured non_zero != 0
        Self(unsafe { NonZeroU64::new_unchecked(non_zero) })
    }

    /// Generate unique runtime key
    ///
    /// Uses thread-safe atomic counter for guaranteed uniqueness.
    /// Each call returns a new unique key.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_core::Key;
    ///
    /// let key1 = Key::new();
    /// let key2 = Key::new();
    /// assert_ne!(key1, key2);
    /// ```
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and lock-free.
    /// Uses `Ordering::Relaxed` for maximum performance.
    #[inline]
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        // SAFETY: Counter starts at 1, so id is always non-zero
        Self(unsafe { NonZeroU64::new_unchecked(id) })
    }

    /// Create key from existing u64 ID
    ///
    /// Returns `None` if `n` is 0 (invalid for `NonZeroU64`).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_core::Key;
    ///
    /// // From database ID
    /// let key = Key::from_u64(user_id).expect("Invalid ID");
    ///
    /// // Check for 0
    /// assert_eq!(Key::from_u64(0), None);
    /// ```
    #[inline]
    pub const fn from_u64(n: u64) -> Option<Self> {
        match NonZeroU64::new(n) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Convert key to raw u64
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_core::Key;
    ///
    /// let key = Key::from_u64(42).unwrap();
    /// assert_eq!(key.as_u64(), 42);
    /// ```
    #[inline]
    pub const fn as_u64(&self) -> u64 {
        self.0.get()
    }

    /// Get the inner NonZeroU64
    #[inline]
    pub(crate) const fn inner(&self) -> NonZeroU64 {
        self.0
    }
}

impl Default for Key {
    /// Default key is generated uniquely
    ///
    /// Same as calling `Key::new()`.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({})", self.0)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Hash for Key {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Key reference for DynWidget trait
///
/// This is a lightweight wrapper around `Key` that can be used
/// in the object-safe `DynWidget` trait. It's essentially the same
/// as `Key` but semantically represents a reference.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyRef(Key);

impl KeyRef {
    /// Create from Key
    #[inline]
    pub const fn new(key: Key) -> Self {
        Self(key)
    }

    /// Convert to raw u64
    #[inline]
    pub const fn as_u64(&self) -> u64 {
        self.0.as_u64()
    }

    /// Get the underlying Key
    #[inline]
    pub const fn key(&self) -> Key {
        self.0
    }
}

impl From<Key> for KeyRef {
    #[inline]
    fn from(k: Key) -> Self {
        Self(k)
    }
}

impl fmt::Debug for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyRef({})", self.0.as_u64())
    }
}

impl fmt::Display for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.as_u64())
    }
}

/// FNV-1a hash for compile-time evaluation
///
/// This is a simple, fast hash function that can be evaluated at compile time.
/// Used for creating constant keys from string literals.
///
/// # Algorithm
///
/// FNV-1a (Fowler-Noll-Vo) is a non-cryptographic hash function:
/// - Fast and simple
/// - Good distribution
/// - Const-evaluatable in Rust
///
/// # References
///
/// - http://www.isthe.com/chongo/tech/comp/fnv/
const fn const_fnv1a_hash(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let mut hash = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::mem::size_of;

    #[test]
    fn test_niche_optimization() {
        // Key uses niche optimization
        assert_eq!(size_of::<Option<Key>>(), size_of::<Key>());
        assert_eq!(size_of::<Option<Key>>(), 8);

        // Regular u64 doesn't
        assert_eq!(size_of::<Option<u64>>(), 16);
    }

    #[test]
    fn test_compile_time_keys() {
        const K1: Key = Key::from_str("test");
        const K2: Key = Key::from_str("test");
        const K3: Key = Key::from_str("other");

        // Same string = same key
        assert_eq!(K1, K2);

        // Different string = different key
        assert_ne!(K1, K3);

        // Runtime matches compile-time
        assert_eq!(K1, Key::from_str("test"));
    }

    #[test]
    fn test_runtime_keys() {
        let k1 = Key::new();
        let k2 = Key::new();
        let k3 = Key::new();

        // All unique
        assert_ne!(k1, k2);
        assert_ne!(k2, k3);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_explicit_keys() {
        assert_eq!(Key::from_u64(0), None);
        assert!(Key::from_u64(1).is_some());
        assert!(Key::from_u64(u64::MAX).is_some());

        let key = Key::from_u64(42).unwrap();
        assert_eq!(key.as_u64(), 42);
    }

    #[test]
    fn test_key_ref() {
        let key = Key::new();
        let key_ref = KeyRef::from(key);

        assert_eq!(key_ref.as_u64(), key.as_u64());
        assert_eq!(key_ref.key(), key);
    }

    #[test]
    fn test_hash_consistency() {
        let key = Key::new();
        let mut set = HashSet::new();

        set.insert(key);
        assert!(set.contains(&key));

        // Same key hashes the same
        let key_copy = key;
        assert!(set.contains(&key_copy));
    }

    #[test]
    fn test_default() {
        let k1 = Key::default();
        let k2 = Key::default();

        // Default creates unique keys
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_debug_display() {
        let key = Key::from_u64(42).unwrap();

        assert_eq!(format!("{:?}", key), "Key(42)");
        assert_eq!(format!("{}", key), "42");

        let key_ref = KeyRef::from(key);
        assert_eq!(format!("{:?}", key_ref), "KeyRef(42)");
        assert_eq!(format!("{}", key_ref), "42");
    }

    #[test]
    fn test_fnv1a_hash() {
        // Known FNV-1a hash values
        const EMPTY: u64 = const_fnv1a_hash(b"");
        const HELLO: u64 = const_fnv1a_hash(b"hello");

        assert_ne!(EMPTY, HELLO);
        assert_ne!(EMPTY, 0); // Empty string should not hash to 0
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        // Generate keys in parallel
        let handles: Vec<_> = (0..10)
            .map(|_| {
                thread::spawn(|| {
                    let keys: Vec<_> = (0..100).map(|_| Key::new()).collect();
                    keys
                })
            })
            .collect();

        let mut all_keys = Vec::new();
        for handle in handles {
            all_keys.extend(handle.join().unwrap());
        }

        // All keys should be unique
        let unique: HashSet<_> = all_keys.iter().collect();
        assert_eq!(unique.len(), all_keys.len());
    }

    #[test]
    fn test_const_evaluation() {
        // This compiles if Key::from_str is truly const
        const _: Key = Key::from_str("compile_time_test");
        const KEYS: [Key; 3] = [
            Key::from_str("one"),
            Key::from_str("two"),
            Key::from_str("three"),
        ];

        assert_eq!(KEYS[0], Key::from_str("one"));
        assert_eq!(KEYS[1], Key::from_str("two"));
        assert_eq!(KEYS[2], Key::from_str("three"));
    }
}
