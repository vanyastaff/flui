//! BoxParentData - Cartesian positioning metadata for box layout children.

use std::hash::{Hash, Hasher};

use flui_foundation::geometry::{Offset, canonical_bits_f64};

use super::base::ParentData;

// ============================================================================
// BOX PARENT DATA
// ============================================================================

/// Parent data for box protocol children storing 2D offset.
///
/// Used by parent render objects to position children in Cartesian space.
/// The offset is relative to the parent's top-left corner.
///
/// # Usage
///
/// ```ignore
/// use flui_rendering::parent_data::BoxParentData;
/// use flui_foundation::geometry::Offset;
///
/// // Create with builder
/// let data = BoxParentData::new(Offset::new(10.0, 20.0));
///
/// // Or use default (zero offset)
/// let data = BoxParentData::default();
///
/// // Builder pattern
/// let data = BoxParentData::zero()
///     .with_offset(Offset::new(10.0, 20.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BoxParentData {
    /// Offset of child relative to parent's top-left corner.
    pub offset: Offset,
}

impl BoxParentData {
    /// Create parent data with specific offset.
    #[inline]
    pub const fn new(offset: Offset) -> Self {
        Self { offset }
    }

    /// Create parent data with zero offset (at parent's origin).
    #[inline]
    pub const fn zero() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }

    /// Builder: set offset (consumes self).
    #[inline]
    pub const fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    /// Check if offset is at origin.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.offset == Offset::ZERO
    }

    /// Set offset to zero (mutating).
    #[inline]
    pub fn reset(&mut self) {
        self.offset = Offset::ZERO;
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Default for BoxParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for BoxParentData {}

// Hash implementation for caching layout results
impl Hash for BoxParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Canonical bits, so offsets equal under `PartialEq` (`0.0` and
        // `-0.0`) hash equal.
        canonical_bits_f64(self.offset.dx).hash(state);
        canonical_bits_f64(self.offset.dy).hash(state);
    }
}

impl Eq for BoxParentData {}

// ============================================================================
// CONVERSIONS
// ============================================================================

impl From<Offset> for BoxParentData {
    fn from(offset: Offset) -> Self {
        Self::new(offset)
    }
}

impl From<(f64, f64)> for BoxParentData {
    fn from((x, y): (f64, f64)) -> Self {
        Self::new(Offset::new(x, y))
    }
}

// ============================================================================
// TESTS
// ============================================================================
