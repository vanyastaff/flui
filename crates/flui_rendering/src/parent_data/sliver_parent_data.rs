//! SliverParentData - Logical positioning metadata for sliver layout children.

use std::hash::{Hash, Hasher};

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
    pub layout_offset: f32,
}

impl SliverParentData {
    /// Create parent data with specific layout offset.
    #[inline]
    pub const fn new(layout_offset: f32) -> Self {
        Self { layout_offset }
    }

    /// Create parent data with zero offset (at parent's start).
    #[inline]
    pub const fn zero() -> Self {
        Self { layout_offset: 0.0 }
    }

    /// Builder: set layout offset (consumes self).
    #[inline]
    pub const fn with_layout_offset(mut self, offset: f32) -> Self {
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
        self.layout_offset.to_bits().hash(state);
    }
}

impl Eq for SliverParentData {}

// ============================================================================
// CONVERSIONS
// ============================================================================

impl From<f32> for SliverParentData {
    fn from(offset: f32) -> Self {
        Self::new(offset)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let data = SliverParentData::new(100.0);

        assert_eq!(data.layout_offset, 100.0);
    }

    #[test]
    fn test_zero() {
        let data = SliverParentData::zero();

        assert_eq!(data.layout_offset, 0.0);
        assert!(data.is_zero());
    }

    #[test]
    fn test_default() {
        let data = SliverParentData::default();

        assert_eq!(data.layout_offset, 0.0);
        assert!(data.is_zero());
    }

    #[test]
    fn test_builder() {
        let data = SliverParentData::zero().with_layout_offset(150.0);

        assert_eq!(data.layout_offset, 150.0);
        assert!(!data.is_zero());
    }

    #[test]
    fn test_reset() {
        let mut data = SliverParentData::new(200.0);
        assert!(!data.is_zero());

        data.reset();
        assert!(data.is_zero());
    }

    #[test]
    fn test_is_valid() {
        let valid = SliverParentData::new(100.0);
        assert!(valid.is_valid());

        let zero = SliverParentData::zero();
        assert!(zero.is_valid());

        let invalid = SliverParentData::new(-50.0);
        assert!(!invalid.is_valid());
    }

    #[test]
    fn test_hash() {
        use std::collections::hash_map::DefaultHasher;

        let data1 = SliverParentData::new(100.0);
        let data2 = SliverParentData::new(100.0);
        let data3 = SliverParentData::new(100.1);

        let mut hasher1 = DefaultHasher::new();
        data1.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        data2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        let mut hasher3 = DefaultHasher::new();
        data3.hash(&mut hasher3);
        let hash3 = hasher3.finish();

        assert_eq!(hash1, hash2); // Same values = same hash
        assert_ne!(hash1, hash3); // Different values = different hash
    }

    #[test]
    fn test_eq() {
        let data1 = SliverParentData::new(100.0);
        let data2 = SliverParentData::new(100.0);
        let data3 = SliverParentData::new(100.1);

        assert_eq!(data1, data2);
        assert_ne!(data1, data3);
    }

    #[test]
    fn test_clone() {
        let data1 = SliverParentData::new(100.0);
        let data2 = data1.clone();

        assert_eq!(data1, data2);
    }

    #[test]
    fn test_from_f32() {
        let data: SliverParentData = 150.0.into();

        assert_eq!(data.layout_offset, 150.0);
    }

    #[test]
    fn test_parent_data_trait() {
        let mut data = SliverParentData::new(100.0);

        // ParentData::detach should not panic
        data.detach();
    }

    #[test]
    fn test_downcast() {
        let data = SliverParentData::new(100.0);
        let trait_obj: &dyn ParentData = &data;

        let downcasted = trait_obj.downcast_ref::<SliverParentData>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().layout_offset, 100.0);
    }
}
