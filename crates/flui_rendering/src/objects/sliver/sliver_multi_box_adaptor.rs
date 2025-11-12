//! RenderSliverMultiBoxAdaptor - Base for slivers with multiple box children

use flui_core::element::ElementId;
use flui_types::Size;

/// Parent data for children of RenderSliverMultiBoxAdaptor
///
/// Tracks the logical index of each child in the data source,
/// enabling efficient lookup and management of lazy-loaded children.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverMultiBoxAdaptorParentData {
    /// Logical index in the data source
    pub index: usize,

    /// Whether this child should be kept alive when scrolled offscreen
    pub keep_alive: bool,
}

impl SliverMultiBoxAdaptorParentData {
    /// Create new parent data with the given index
    pub fn new(index: usize) -> Self {
        Self {
            index,
            keep_alive: false,
        }
    }

    /// Create parent data with keep-alive enabled
    pub fn with_keep_alive(index: usize) -> Self {
        Self {
            index,
            keep_alive: true,
        }
    }
}

impl Default for SliverMultiBoxAdaptorParentData {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Abstract base trait for slivers with multiple box children
///
/// This trait provides common functionality for slivers that:
/// - Contain multiple box (RenderBox) children
/// - Support lazy loading of children
/// - Track logical indices for children
/// - Can keep children alive when scrolled offscreen
///
/// # Implementers
///
/// - `RenderSliverList` - Linear list layout
/// - `RenderSliverFixedExtentList` - List with fixed item heights
/// - `RenderSliverGrid` - Grid layout
///
/// # Lazy Loading
///
/// Children are created on-demand during layout. Only children that are
/// visible or near-visible (within cache extent) are instantiated, enabling
/// efficient scrolling through large datasets.
///
/// # Keep Alive
///
/// Children can be marked as "keep alive" to remain in memory even when
/// scrolled offscreen. This is useful for:
/// - Preserving scroll position in nested scrollables
/// - Maintaining expensive widget state
/// - Avoiding rebuild costs for complex children
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSliverMultiBoxAdaptor, SliverMultiBoxAdaptorParentData};
///
/// struct MyList {
///     // ... fields
/// }
///
/// impl RenderSliverMultiBoxAdaptor for MyList {
///     fn child_count(&self) -> usize {
///         self.items.len()
///     }
///
///     fn estimate_max_scroll_offset(&self, first_index: usize, last_index: usize) -> f32 {
///         (self.child_count() - first_index) as f32 * self.item_height
///     }
/// }
/// ```
pub trait RenderSliverMultiBoxAdaptor {
    /// Get the total number of children
    ///
    /// Returns the logical count of all children that could be created,
    /// not just the number currently instantiated.
    fn child_count(&self) -> usize;

    /// Get the main axis position of a child
    ///
    /// Returns the distance from the leading visible edge of the sliver
    /// to the side of the given child closest to that edge.
    ///
    /// # Arguments
    ///
    /// * `child_id` - Element ID of the child
    /// * `child_index` - Logical index of the child
    fn child_main_axis_position(&self, child_id: ElementId, child_index: usize) -> f32;

    /// Get the main axis extent (height/width) of a child
    ///
    /// Returns the dimension of the child in the main axis direction.
    ///
    /// # Arguments
    ///
    /// * `child_size` - Size of the child from layout
    fn child_main_axis_extent(&self, child_size: Size) -> f32;

    /// Estimate the maximum scroll offset
    ///
    /// Provides an estimate of the total scrollable extent based on the
    /// currently visible range of children. Used for scroll bar sizing
    /// and scroll physics.
    ///
    /// # Arguments
    ///
    /// * `first_index` - Index of first visible child
    /// * `last_index` - Index of last visible child
    ///
    /// # Returns
    ///
    /// Estimated total scroll extent in pixels
    fn estimate_max_scroll_offset(&self, first_index: usize, last_index: usize) -> f32;

    /// Get the cross axis offset for a child
    ///
    /// Returns the offset in the cross axis (perpendicular to scroll direction)
    /// where the child should be positioned. Default implementation returns 0.
    ///
    /// Override this for multi-column layouts like grids.
    fn child_cross_axis_offset(&self, _child_index: usize) -> f32 {
        0.0
    }

    /// Check if a child should be kept alive
    ///
    /// Returns true if the child at the given index should remain in memory
    /// even when scrolled offscreen.
    fn should_keep_alive(&self, _child_index: usize) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parent_data_creation() {
        let data = SliverMultiBoxAdaptorParentData::new(5);
        assert_eq!(data.index, 5);
        assert!(!data.keep_alive);
    }

    #[test]
    fn test_parent_data_with_keep_alive() {
        let data = SliverMultiBoxAdaptorParentData::with_keep_alive(10);
        assert_eq!(data.index, 10);
        assert!(data.keep_alive);
    }

    #[test]
    fn test_parent_data_default() {
        let data = SliverMultiBoxAdaptorParentData::default();
        assert_eq!(data.index, 0);
        assert!(!data.keep_alive);
    }

    struct TestAdaptor {
        count: usize,
        item_height: f32,
    }

    impl RenderSliverMultiBoxAdaptor for TestAdaptor {
        fn child_count(&self) -> usize {
            self.count
        }

        fn child_main_axis_position(&self, _child_id: ElementId, child_index: usize) -> f32 {
            child_index as f32 * self.item_height
        }

        fn child_main_axis_extent(&self, child_size: Size) -> f32 {
            child_size.height
        }

        fn estimate_max_scroll_offset(&self, first_index: usize, _last_index: usize) -> f32 {
            (self.count - first_index) as f32 * self.item_height
        }
    }

    #[test]
    fn test_adaptor_child_count() {
        let adaptor = TestAdaptor {
            count: 100,
            item_height: 50.0,
        };
        assert_eq!(adaptor.child_count(), 100);
    }

    #[test]
    fn test_adaptor_child_position() {
        let adaptor = TestAdaptor {
            count: 100,
            item_height: 50.0,
        };
        let child_id = ElementId::new(1);
        assert_eq!(adaptor.child_main_axis_position(child_id, 0), 0.0);
        assert_eq!(adaptor.child_main_axis_position(child_id, 5), 250.0);
        assert_eq!(adaptor.child_main_axis_position(child_id, 10), 500.0);
    }

    #[test]
    fn test_adaptor_estimate_scroll_offset() {
        let adaptor = TestAdaptor {
            count: 100,
            item_height: 50.0,
        };
        // If we're showing items 10-20, estimate remaining is (100-10) * 50 = 4500
        assert_eq!(adaptor.estimate_max_scroll_offset(10, 20), 4500.0);
    }

    #[test]
    fn test_adaptor_default_cross_axis_offset() {
        let adaptor = TestAdaptor {
            count: 100,
            item_height: 50.0,
        };
        assert_eq!(adaptor.child_cross_axis_offset(5), 0.0);
    }

    #[test]
    fn test_adaptor_default_keep_alive() {
        let adaptor = TestAdaptor {
            count: 100,
            item_height: 50.0,
        };
        assert!(!adaptor.should_keep_alive(5));
    }
}
