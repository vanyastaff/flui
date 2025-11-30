//! Type-safe identifiers using newtype pattern
//!
//! This module provides strongly-typed identifiers that prevent mixing up
//! different ID types at compile time.
//!
//! # Features
//!
//! - **Type safety**: Cannot mix `PointerId` with `FocusNodeId`
//! - **Zero-cost**: No runtime overhead (same size as underlying type)
//! - **Niche optimization**: `Option<NonZeroId>` is same size as `Id`
//! - **Debug/Display**: Nice formatting for debugging
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::ids::{PointerId, FocusNodeId};
//!
//! let pointer = PointerId::new(1);
//! let focus = FocusNodeId::new(42);
//!
//! // These are different types - cannot mix!
//! // fn process(id: PointerId) { ... }
//! // process(focus); // Compile error!
//! ```

use std::fmt;
use std::num::NonZeroU64;

// ============================================================================
// PointerId - Identifier for pointer devices
// ============================================================================

/// Unique identifier for a pointer device (mouse, touch, stylus).
///
/// Uses `i32` to match platform APIs (winit, etc.).
///
/// # Example
///
/// ```rust
/// use flui_interaction::ids::PointerId;
///
/// let mouse = PointerId::new(0);
/// let touch1 = PointerId::new(1);
///
/// assert_ne!(mouse, touch1);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct PointerId(i32);

impl PointerId {
    /// Creates a new pointer ID.
    #[inline]
    pub const fn new(id: i32) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
    #[inline]
    pub const fn get(self) -> i32 {
        self.0
    }

    /// Returns the raw ID value (alias for compatibility).
    #[inline]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Mouse pointer ID (typically 0).
    pub const MOUSE: Self = Self(0);
}

impl fmt::Debug for PointerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PointerId({})", self.0)
    }
}

impl fmt::Display for PointerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pointer:{}", self.0)
    }
}

impl From<i32> for PointerId {
    #[inline]
    fn from(id: i32) -> Self {
        Self::new(id)
    }
}

impl From<PointerId> for i32 {
    #[inline]
    fn from(id: PointerId) -> Self {
        id.0
    }
}

// ============================================================================
// FocusNodeId - Identifier for focusable UI elements
// ============================================================================

/// Unique identifier for a focusable UI element.
///
/// Uses `NonZeroU64` for niche optimization: `Option<FocusNodeId>` is same size.
///
/// # Example
///
/// ```rust
/// use flui_interaction::ids::FocusNodeId;
///
/// let text_field = FocusNodeId::new(1);
/// let button = FocusNodeId::new(2);
///
/// // Option<FocusNodeId> is still 8 bytes due to niche optimization
/// assert_eq!(
///     std::mem::size_of::<Option<FocusNodeId>>(),
///     std::mem::size_of::<FocusNodeId>()
/// );
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct FocusNodeId(NonZeroU64);

impl FocusNodeId {
    /// Creates a new focus node ID.
    ///
    /// # Panics
    ///
    /// Panics if `id` is 0. Use `try_new` for fallible construction.
    #[inline]
    pub fn new(id: u64) -> Self {
        Self(NonZeroU64::new(id).expect("FocusNodeId cannot be 0"))
    }

    /// Creates a new focus node ID, returning `None` if `id` is 0.
    #[inline]
    pub const fn try_new(id: u64) -> Option<Self> {
        match NonZeroU64::new(id) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Returns the raw ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Creates a FocusNodeId from a NonZeroU64.
    #[inline]
    pub const fn from_non_zero(nz: NonZeroU64) -> Self {
        Self(nz)
    }
}

impl fmt::Debug for FocusNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FocusNodeId({})", self.0)
    }
}

impl fmt::Display for FocusNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "focus:{}", self.0)
    }
}

impl From<NonZeroU64> for FocusNodeId {
    #[inline]
    fn from(nz: NonZeroU64) -> Self {
        Self(nz)
    }
}

impl From<FocusNodeId> for NonZeroU64 {
    #[inline]
    fn from(id: FocusNodeId) -> Self {
        id.0
    }
}

// ============================================================================
// HandlerId - Identifier for registered handlers
// ============================================================================

/// Unique identifier for a registered event handler.
///
/// Used by signal resolver and other registration systems.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct HandlerId(NonZeroU64);

impl HandlerId {
    /// Creates a new handler ID.
    ///
    /// # Panics
    ///
    /// Panics if `id` is 0.
    #[inline]
    pub fn new(id: u64) -> Self {
        Self(NonZeroU64::new(id).expect("HandlerId cannot be 0"))
    }

    /// Returns the raw ID value.
    #[inline]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Debug for HandlerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HandlerId({})", self.0)
    }
}

impl fmt::Display for HandlerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "handler:{}", self.0)
    }
}

impl From<NonZeroU64> for HandlerId {
    #[inline]
    fn from(nz: NonZeroU64) -> Self {
        Self(nz)
    }
}

impl From<HandlerId> for NonZeroU64 {
    #[inline]
    fn from(id: HandlerId) -> Self {
        id.0
    }
}

// ============================================================================
// DeviceId - Identifier for input devices (mouse tracker)
// ============================================================================

/// Unique identifier for an input device.
///
/// Alias for mouse tracker compatibility.
pub type DeviceId = i32;

// ============================================================================
// RegionId - Identifier for mouse regions
// ============================================================================

/// Unique identifier for a mouse-sensitive region.
///
/// Re-exported from `flui_foundation::ElementId`.
pub use flui_foundation::ElementId as RegionId;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_id() {
        let id = PointerId::new(42);
        assert_eq!(id.get(), 42);
        assert_eq!(id.raw(), 42);
        assert_eq!(format!("{:?}", id), "PointerId(42)");
        assert_eq!(format!("{}", id), "pointer:42");
    }

    #[test]
    fn test_focus_node_id() {
        let id = FocusNodeId::new(123);
        assert_eq!(id.get(), 123);
        assert_eq!(format!("{:?}", id), "FocusNodeId(123)");
        assert_eq!(format!("{}", id), "focus:123");
    }

    #[test]
    fn test_focus_node_id_niche_optimization() {
        // Option<FocusNodeId> should be same size as FocusNodeId
        // due to NonZeroU64 niche optimization
        assert_eq!(
            std::mem::size_of::<Option<FocusNodeId>>(),
            std::mem::size_of::<FocusNodeId>()
        );
    }

    #[test]
    fn test_focus_node_id_try_new() {
        assert!(FocusNodeId::try_new(0).is_none());
        assert!(FocusNodeId::try_new(1).is_some());
    }

    #[test]
    #[should_panic(expected = "FocusNodeId cannot be 0")]
    fn test_focus_node_id_zero_panics() {
        let _ = FocusNodeId::new(0);
    }

    #[test]
    fn test_handler_id() {
        let id = HandlerId::new(999);
        assert_eq!(id.get(), 999);
        assert_eq!(format!("{:?}", id), "HandlerId(999)");
    }

    #[test]
    fn test_pointer_id_equality() {
        let a = PointerId::new(1);
        let b = PointerId::new(1);
        let c = PointerId::new(2);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_pointer_id_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(PointerId::new(1));
        set.insert(PointerId::new(2));
        set.insert(PointerId::new(1)); // duplicate

        assert_eq!(set.len(), 2);
    }
}
