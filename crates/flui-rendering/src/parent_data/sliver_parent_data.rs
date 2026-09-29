//! SliverParentData - Logical positioning metadata for sliver layout children.

use std::hash::{Hash, Hasher};

use flui_foundation::geometry::canonical_bits_f64;

use super::base::ParentData;

// ============================================================================
// SLIVER PARENT DATA
// ============================================================================

/// Parent data for sliver protocol children storing logical scroll offset.
///
/// Used by parent sliver render objects (like SliverList) to track
/// each child's logical position in the scrollable axis. This differs from
/// physical painting position and represents the child's position in the
/// overall scroll extent.
///
/// # Usage
///
/// ```ignore
/// use flui_rendering::parent_data::SliverParentData;
///
/// // Create with specific layout offset
/// let data = SliverParentData::new(100.0);
///
/// // Or use default (zero offset)
/// let data = SliverParentData::default();
///
/// // Builder pattern
/// let data = SliverParentData::zero()
///     .with_layout_offset(100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SliverParentData {
    /// Logical offset in scrollable axis (not paint offset).
    ///
    /// This is the distance from the start of the parent sliver's
    /// scroll extent to the start of this child's scroll extent.
    pub layout_offset: f64,
}

impl SliverParentData {
    /// Create parent data with specific layout offset.
    #[inline]
    pub const fn new(layout_offset: f64) -> Self {
        Self { layout_offset }
    }

    /// Create parent data with zero offset (at parent's start).
    #[inline]
    pub const fn zero() -> Self {
        Self { layout_offset: 0.0 }
    }

    /// Builder: set layout offset (consumes self).
    #[inline]
    pub const fn with_layout_offset(mut self, offset: f64) -> Self {
        self.layout_offset = offset;
        self
    }

    /// Check if offset is at origin.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.layout_offset == 0.0
    }

    /// Set offset to zero (mutating).
    #[inline]
    pub fn reset(&mut self) {
        self.layout_offset = 0.0;
    }

    /// Check if offset is valid (non-negative).
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.layout_offset >= 0.0
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Default for SliverParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for SliverParentData {}

// Hash implementation for caching layout results
impl Hash for SliverParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash offset as bits to avoid float precision issues
        canonical_bits_f64(self.layout_offset).hash(state);
    }
}

impl Eq for SliverParentData {}

// ============================================================================
// CONVERSIONS
// ============================================================================

impl From<f64> for SliverParentData {
    fn from(offset: f64) -> Self {
        Self::new(offset)
    }
}

// ============================================================================
// TESTS
// ============================================================================
