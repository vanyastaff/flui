//! Element ID - stable index into element tree
//!
//! Provides a type-safe, memory-efficient identifier for elements in the UI tree.
//!
//! # Design
//!
//! Uses `NonZeroUsize` for niche optimization:
//! - `Option<ElementId>` is same size as `ElementId` (no extra byte needed)
//! - Prevents 0 from being a valid ID (reserved for sentinel)
//! - Enables pattern matching without branching overhead
//!
//! # Example
//!
//! ```rust
//! use flui_core::foundation::ElementId;
//!
//! // Option<ElementId> is same size as ElementId (8 bytes on 64-bit)
//! assert_eq!(
//!     std::mem::size_of::<ElementId>(),
//!     std::mem::size_of::<Option<ElementId>>()
//! );
//!
//! // Create from usize (panics if 0)
//! let id = ElementId::new(1);
//!
//! // Safe creation that returns Option
//! let maybe_id = ElementId::new_checked(0); // None
//! let valid_id = ElementId::new_checked(1); // Some(ElementId)
//! ```

use std::num::NonZeroUsize;

/// Element ID - stable index into the ElementTree slab
///
/// Uses `NonZeroUsize` internally for niche optimization:
/// - `Option<ElementId>` is same size as `ElementId` (no extra byte)
/// - Prevents 0 from being a valid ID (0 reserved for sentinel)
/// - Enables pattern matching on Option without branching overhead
///
/// This is a handle to an element that remains valid until the element is removed.
/// ElementIds are reused after removal (slab behavior), so don't store them long-term
/// without verifying the element still exists.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::ElementId;
///
/// // Option<ElementId> is same size as ElementId (8 bytes on 64-bit)
/// assert_eq!(
///     std::mem::size_of::<ElementId>(),
///     std::mem::size_of::<Option<ElementId>>()
/// );
///
/// // Create from usize (panics if 0)
/// let id = ElementId::new(1);
///
/// // Safe creation that returns Option
/// let maybe_id = ElementId::new_checked(0); // None
/// let valid_id = ElementId::new_checked(1); // Some(ElementId)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ElementId(NonZeroUsize);

impl ElementId {
    /// Create a new ElementId from a non-zero usize.
    ///
    /// # Panics
    ///
    /// Panics if `id` is 0. Zero is reserved for sentinel values
    /// and cannot be used as a valid ElementId.
    ///
    /// If you need to handle 0, use [`new_checked()`](ElementId::new_checked) instead.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::ElementId;
    ///
    /// let id = ElementId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    #[inline]
    #[track_caller]
    pub fn new(id: usize) -> Self {
        Self(NonZeroUsize::new(id).unwrap_or_else(|| {
            panic!(
                "ElementId::new() called with 0, which is not a valid ElementId.\n\
                \n\
                ElementId uses NonZeroUsize internally, so 0 is reserved for sentinel values.\n\
                \n\
                To handle potentially-zero values, use ElementId::new_checked() instead:\n\
                ```\n\
                match ElementId::new_checked(id) {{\n\
                    Some(element_id) => /* use element_id */,\n\
                    None => /* handle zero case */,\n\
                }}\n\
                ```"
            )
        }))
    }

    /// Create a new ElementId from a usize, returning None if 0.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::ElementId;
    ///
    /// assert_eq!(ElementId::new_checked(0), None);
    /// assert_eq!(ElementId::new_checked(1).map(|id| id.get()), Some(1));
    /// ```
    #[inline]
    pub const fn new_checked(id: usize) -> Option<Self> {
        match NonZeroUsize::new(id) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Get the inner usize value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::ElementId;
    ///
    /// let id = ElementId::new(42);
    /// assert_eq!(id.get(), 42);
    /// ```
    #[inline]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// Create an ElementId without checking if the value is non-zero.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `id` is not 0.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::foundation::ElementId;
    ///
    /// // Safe because 1 is non-zero
    /// let id = unsafe { ElementId::new_unchecked(1) };
    /// assert_eq!(id.get(), 1);
    /// ```
    #[inline]
    pub const unsafe fn new_unchecked(id: usize) -> Self {
        // SAFETY: Caller must ensure id is non-zero
        unsafe { Self(NonZeroUsize::new_unchecked(id)) }
    }
}

// =========================================================================
// Conversions for ergonomics
// =========================================================================

impl From<NonZeroUsize> for ElementId {
    #[inline]
    fn from(id: NonZeroUsize) -> Self {
        Self(id)
    }
}

impl From<ElementId> for usize {
    #[inline]
    fn from(id: ElementId) -> usize {
        id.get()
    }
}

// Backward compatibility: Allow using ElementId as usize in tests
#[cfg(test)]
impl From<usize> for ElementId {
    fn from(id: usize) -> Self {
        Self::new(id)
    }
}

// =========================================================================
// Display for debugging
// =========================================================================

impl std::fmt::Display for ElementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ElementId({})", self.get())
    }
}

// =========================================================================
// Arithmetic operations (for bitmap indexing in dirty tracking)
// =========================================================================

impl std::ops::Sub<usize> for ElementId {
    type Output = usize;

    #[inline]
    fn sub(self, rhs: usize) -> usize {
        self.get() - rhs
    }
}

impl std::ops::Add<usize> for ElementId {
    type Output = ElementId;

    #[inline]
    fn add(self, rhs: usize) -> ElementId {
        ElementId::new(self.get() + rhs)
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_id_new() {
        let id = ElementId::new(42);
        assert_eq!(id.get(), 42);
    }

    #[test]
    #[should_panic(expected = "ElementId::new() called with 0")]
    fn test_element_id_new_zero_panics() {
        let _ = ElementId::new(0);
    }

    #[test]
    fn test_element_id_new_checked() {
        assert_eq!(ElementId::new_checked(0), None);
        assert_eq!(ElementId::new_checked(1).map(|id| id.get()), Some(1));
        assert_eq!(ElementId::new_checked(42).map(|id| id.get()), Some(42));
    }

    #[test]
    fn test_element_id_new_unchecked() {
        let id = unsafe { ElementId::new_unchecked(1) };
        assert_eq!(id.get(), 1);
    }

    #[test]
    fn test_element_id_niche_optimization() {
        // Option<ElementId> should be same size as ElementId (niche optimization)
        assert_eq!(
            std::mem::size_of::<ElementId>(),
            std::mem::size_of::<Option<ElementId>>()
        );
    }

    #[test]
    fn test_element_id_from_non_zero() {
        let nz = NonZeroUsize::new(42).unwrap();
        let id = ElementId::from(nz);
        assert_eq!(id.get(), 42);
    }

    #[test]
    fn test_element_id_into_usize() {
        let id = ElementId::new(42);
        let value: usize = id.into();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_element_id_display() {
        let id = ElementId::new(42);
        assert_eq!(format!("{}", id), "ElementId(42)");
    }

    #[test]
    fn test_element_id_arithmetic() {
        let id = ElementId::new(10);
        assert_eq!(id - 5, 5);
        assert_eq!((id + 5).get(), 15);
    }

    #[test]
    fn test_element_id_equality() {
        let id1 = ElementId::new(42);
        let id2 = ElementId::new(42);
        let id3 = ElementId::new(43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_element_id_ordering() {
        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        assert!(id1 < id2);
        assert!(id2 < id3);
        assert!(id1 < id3);
    }

    #[test]
    fn test_element_id_from_usize_in_tests() {
        let id: ElementId = 42.into();
        assert_eq!(id.get(), 42);
    }
}
