//! String interning for O(1) comparison
//!
//! This module provides zero-cost string interning using the `lasso` crate.
//! Interned strings are small (4-8 bytes), `Copy`, and can be compared in O(1).
//!
//! # Performance
//!
//! - **Interning**: O(1) amortized, uses hash table lookup
//! - **Comparison**: O(1) integer comparison (not string comparison!)
//! - **Resolving**: O(1) array lookup
//! - **Memory**: ~4 bytes per interned string handle
//!
//! # Use Cases
//!
//! String interning is ideal for:
//! - Widget type names (e.g., "Container", "Row", "Column")
//! - Style class names that are compared frequently
//! - Property names in serialization
//! - Any strings that are:
//!   - Compared frequently
//!   - Small set of unique values
//!   - Immutable
//!
//! # Examples
//!
//! ```rust
//! use flui_core::foundation::string_cache::{intern, resolve, InternedString};
//!
//! // Intern strings for fast comparison
//! let s1 = intern("Container");
//! let s2 = intern("Container");
//! assert_eq!(s1, s2); // O(1) integer comparison!
//!
//! // Resolve back to &str when needed
//! let text = resolve(s1);
//! assert_eq!(text, "Container");
//!
//! // Check if already interned without creating
//! use flui_core::foundation::string_cache::get;
//! if let Some(handle) = get("Container") {
//!     println!("Already interned: {}", resolve(handle));
//! }
//! ```
//!
//! # Thread Safety
//!
//! All operations are thread-safe. Multiple threads can intern and resolve
//! strings concurrently without synchronization.

use lasso::{Spur, ThreadedRodeo};
use once_cell::sync::Lazy;
use std::fmt;

/// Global thread-safe string interner
///
/// This is a static interner that lives for the entire program lifetime.
/// All interned strings remain in memory until program exit.
static INTERNER: Lazy<ThreadedRodeo> = Lazy::new(ThreadedRodeo::default);

/// Interned string handle for O(1) comparison
///
/// This is a lightweight, `Copy` handle to an interned string. It's typically
/// 4 bytes in size and can be compared in O(1) time using integer comparison.
///
/// # Type Safety
///
/// This is a newtype wrapper around `lasso::Spur` for better type safety.
/// You cannot accidentally mix interned strings with other integer types.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::string_cache::{intern, resolve, InternedString};
///
/// let handle: InternedString = intern("Widget");
/// let text: &str = resolve(handle);
/// assert_eq!(text, "Widget");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InternedString(Spur);

impl InternedString {
    /// Creates an interned string from a raw `Spur`
    ///
    /// This is primarily for internal use and interop with `lasso` APIs.
    #[must_use]
    #[inline]
    pub const fn from_raw(spur: Spur) -> Self {
        Self(spur)
    }

    /// Returns the underlying `Spur` handle
    ///
    /// This is primarily for internal use and interop with `lasso` APIs.
    #[must_use]
    #[inline]
    pub const fn into_raw(self) -> Spur {
        self.0
    }

    /// Resolves this handle to its string value
    ///
    /// This is a convenience method equivalent to calling `resolve(handle)`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::string_cache::intern;
    ///
    /// let handle = intern("Container");
    /// assert_eq!(handle.as_str(), "Container");
    /// ```
    #[must_use]
    #[inline]
    pub fn as_str(self) -> &'static str {
        resolve(self)
    }
}

impl fmt::Display for InternedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for InternedString {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for InternedString {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

/// Interns a string, returning a handle for O(1) comparison
///
/// This operation is O(1) amortized. If the string is already interned,
/// the existing handle is returned immediately without allocation.
///
/// # Performance
///
/// - First intern: O(1) amortized (hash table insert + string allocation)
/// - Subsequent interns: O(1) hash table lookup
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::string_cache::intern;
///
/// let handle1 = intern("MyWidget");
/// let handle2 = intern("MyWidget");
/// assert_eq!(handle1, handle2); // Same handle for same string
///
/// // Different strings get different handles
/// let handle3 = intern("OtherWidget");
/// assert_ne!(handle1, handle3);
/// ```
#[must_use = "interning a string without using the handle is pointless"]
#[inline]
pub fn intern(s: &str) -> InternedString {
    InternedString(INTERNER.get_or_intern(s))
}

/// Resolves an interned string handle back to `&'static str`
///
/// This is an O(1) array lookup operation. The returned string is guaranteed
/// to be valid for the entire program lifetime.
///
/// # Lifetime Safety
///
/// The returned `&'static str` is safe because:
/// 1. `INTERNER` is a static `Lazy<ThreadedRodeo>` with `'static` lifetime
/// 2. `ThreadedRodeo` guarantees interned strings are never dropped
/// 3. Strings remain in memory until the interner is dropped (at program exit)
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::string_cache::{intern, resolve};
///
/// let handle = intern("Container");
/// let text = resolve(handle);
/// assert_eq!(text, "Container");
///
/// // The string has 'static lifetime
/// let static_str: &'static str = resolve(handle);
/// ```
#[must_use]
#[inline]
pub fn resolve(key: InternedString) -> &'static str {
    // SAFETY: This is safe because:
    // 1. INTERNER is 'static Lazy<ThreadedRodeo>
    // 2. ThreadedRodeo never drops interned strings (they live for its lifetime)
    // 3. Since INTERNER is 'static, all interned strings are effectively 'static
    // 4. The Spur key is valid because it came from our interner
    //
    // The transmute extends the lifetime from the borrow of INTERNER to 'static.
    // This is sound because INTERNER is never dropped during program execution.
    unsafe {
        let s = INTERNER.resolve(&key.0);
        // Extend lifetime to 'static (safe because interner is static)
        std::mem::transmute::<&str, &'static str>(s)
    }
}

/// Retrieves the handle for an already-interned string
///
/// Returns `Some(handle)` if the string was previously interned,
/// `None` otherwise. This does **not** intern the string if not found.
///
/// Use this when you want to check if a string is interned without
/// adding it to the interner.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::string_cache::{intern, get};
///
/// // Intern a string first
/// let handle = intern("Container");
///
/// // Now we can look it up
/// assert_eq!(get("Container"), Some(handle));
/// assert_eq!(get("NotInterned"), None);
/// ```
#[must_use]
#[inline]
pub fn get(s: &str) -> Option<InternedString> {
    INTERNER.get(s).map(InternedString)
}

/// Returns the number of unique strings currently interned
///
/// This can be useful for debugging and monitoring memory usage.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::string_cache::{intern, len};
///
/// let before = len();
/// intern("NewString");
/// let after = len();
/// assert_eq!(after, before + 1);
/// ```
#[must_use]
#[inline]
pub fn len() -> usize {
    INTERNER.len()
}

/// Checks if no strings have been interned yet
///
/// Returns `true` if the interner is empty.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::string_cache::{is_empty, intern};
///
/// // Note: In real code, other parts of the program may have
/// // already interned strings, so is_empty() might be false
/// ```
#[must_use]
#[inline]
pub fn is_empty() -> bool {
    INTERNER.is_empty()
}

/// Returns the memory capacity of the interner
///
/// This is the total capacity allocated, not the number of strings.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::string_cache::capacity;
///
/// let cap = capacity();
/// assert!(cap > 0);
/// ```
#[must_use]
#[inline]
pub fn capacity() -> usize {
    INTERNER.capacity()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_same_string_twice() {
        let s1 = intern("test_same");
        let s2 = intern("test_same");
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_intern_different_strings() {
        let s1 = intern("foo_test");
        let s2 = intern("bar_test");
        assert_ne!(s1, s2);
    }

    #[test]
    fn test_resolve() {
        let handle = intern("MyWidget_test");
        let s = resolve(handle);
        assert_eq!(s, "MyWidget_test");
    }

    #[test]
    fn test_resolve_method() {
        let handle = intern("Container_test");
        assert_eq!(handle.as_str(), "Container_test");
    }

    #[test]
    fn test_get_existing() {
        let handle = intern("existing_test");
        assert_eq!(get("existing_test"), Some(handle));
    }

    #[test]
    fn test_get_non_existing() {
        // Use UUID to ensure uniqueness
        let unique = format!("non_existing_{:x}", rand::random::<u64>());
        assert_eq!(get(&unique), None);
    }

    #[test]
    fn test_len_and_is_empty() {
        let before = len();

        // Intern unique string
        let unique = format!("len_test_{:x}", rand::random::<u64>());
        intern(&unique);

        let after = len();
        assert_eq!(after, before + 1);
        assert!(!is_empty());
    }

    #[test]
    fn test_capacity() {
        let cap = capacity();
        assert!(cap > 0);
        assert!(cap >= len());
    }

    #[test]
    fn test_comparison_performance() {
        // Intern some widget type names
        let container = intern("Container_perf");
        let row = intern("Row_perf");
        let column = intern("Column_perf");
        let text = intern("Text_perf");

        // Fast O(1) comparisons
        assert_eq!(container, intern("Container_perf"));
        assert_ne!(container, row);
        assert_ne!(row, column);
        assert_eq!(text, intern("Text_perf"));
    }

    #[test]
    fn test_widget_type_names() {
        // Simulate widget type name interning
        let widget_types = vec![
            "Container_w", "Row_w", "Column_w", "Stack_w", "Text_w",
            "Image_w", "Button_w", "TextField_w", "Checkbox_w", "Radio_w"
        ];

        use itertools::Itertools;
        let interned = widget_types.iter().map(|s| intern(s)).collect_vec();

        // Verify all strings are interned with zip_eq for equal-length assertion
        for (&s, &interned_handle) in itertools::zip_eq(&widget_types, &interned) {
            assert_eq!(interned_handle, intern(s));
        }
    }

    #[test]
    fn test_display() {
        let handle = intern("DisplayTest");
        assert_eq!(format!("{}", handle), "DisplayTest");
    }

    #[test]
    fn test_as_ref() {
        let handle = intern("AsRefTest");
        let s: &str = handle.as_ref();
        assert_eq!(s, "AsRefTest");
    }

    #[test]
    fn test_borrow() {
        use std::borrow::Borrow;

        let handle = intern("BorrowTest");
        let s: &str = handle.borrow();
        assert_eq!(s, "BorrowTest");
    }

    #[test]
    fn test_hash() {
        use std::collections::HashMap;

        let handle1 = intern("HashTest1");
        let handle2 = intern("HashTest2");

        let mut map = HashMap::new();
        map.insert(handle1, "value1");
        map.insert(handle2, "value2");

        assert_eq!(map.get(&handle1), Some(&"value1"));
        assert_eq!(map.get(&handle2), Some(&"value2"));
    }

    #[test]
    fn test_ord() {
        let a = intern("aaa");
        let b = intern("bbb");

        // Ordering is based on internal Spur ordering, not string ordering
        assert!(a != b);
        assert!(a < b || a > b);
    }

    #[test]
    fn test_copy_semantics() {
        let handle = intern("CopyTest");
        let copy = handle; // Copy, not move

        assert_eq!(handle, copy);
        assert_eq!(handle.as_str(), copy.as_str());
    }
}