//! BoxParentData - Cartesian positioning metadata for box layout children.

use flui_types::Offset;
use std::hash::{Hash, Hasher};

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
/// use flui_types::Offset;
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
        // Hash offset components as bits to avoid float precision issues
        self.offset.dx.to_bits().hash(state);
        self.offset.dy.to_bits().hash(state);
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

impl From<(f32, f32)> for BoxParentData {
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(Offset::new(flui_types::Pixels(x), flui_types::Pixels(y)))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_new() {
        let offset = Offset::new(px(10.0), px(20.0));
        let data = BoxParentData::new(offset);

        assert_eq!(data.offset, offset);
    }

    #[test]
    fn test_zero() {
        let data = BoxParentData::zero();

        assert_eq!(data.offset, Offset::ZERO);
        assert!(data.is_zero());
    }

    #[test]
    fn test_default() {
        let data = BoxParentData::default();

        assert_eq!(data.offset, Offset::ZERO);
        assert!(data.is_zero());
    }

    #[test]
    fn test_builder() {
        let data = BoxParentData::zero().with_offset(Offset::new(px(5.0), px(10.0)));

        assert_eq!(data.offset.dx, 5.0);
        assert_eq!(data.offset.dy, 10.0);
        assert!(!data.is_zero());
    }

    #[test]
    fn test_reset() {
        let mut data = BoxParentData::new(Offset::new(px(10.0), px(20.0)));
        assert!(!data.is_zero());

        data.reset();
        assert!(data.is_zero());
    }

    #[test]
    fn test_hash() {
        use std::collections::hash_map::DefaultHasher;

        let data1 = BoxParentData::new(Offset::new(px(10.0), px(20.0)));
        let data2 = BoxParentData::new(Offset::new(px(10.0), px(20.0)));
        let data3 = BoxParentData::new(Offset::new(px(10.0), px(20.1)));

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
        let data1 = BoxParentData::new(Offset::new(px(10.0), px(20.0)));
        let data2 = BoxParentData::new(Offset::new(px(10.0), px(20.0)));
        let data3 = BoxParentData::new(Offset::new(px(10.0), px(20.1)));

        assert_eq!(data1, data2);
        assert_ne!(data1, data3);
    }

    #[test]
    fn test_clone() {
        let data1 = BoxParentData::new(Offset::new(px(10.0), px(20.0)));
        let data2 = data1.clone();

        assert_eq!(data1, data2);
    }

    #[test]
    fn test_from_offset() {
        let offset = Offset::new(px(15.0), px(25.0));
        let data: BoxParentData = offset.into();

        assert_eq!(data.offset, offset);
    }

    #[test]
    fn test_from_tuple() {
        let data: BoxParentData = (15.0, 25.0).into();

        assert_eq!(data.offset.dx, 15.0);
        assert_eq!(data.offset.dy, 25.0);
    }

    #[test]
    fn test_parent_data_trait() {
        let mut data = BoxParentData::new(Offset::new(px(10.0), px(20.0)));

        // ParentData::detach should not panic
        data.detach();
    }

    #[test]
    fn test_downcast() {
        let data = BoxParentData::new(Offset::new(px(10.0), px(20.0)));
        let trait_obj: &dyn ParentData = &data;

        let downcasted = trait_obj.downcast_ref::<BoxParentData>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().offset.dx, 10.0);
    }
}
