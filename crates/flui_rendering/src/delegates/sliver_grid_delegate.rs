//! Sliver grid delegate for grid layout in slivers.
//!
//! [`SliverGridDelegate`] allows users to define grid layout algorithms
//! for slivers, controlling the number of columns, spacing, and child sizes.

use std::any::Any;
use std::fmt::Debug;

use flui_types::SliverConstraints;

/// The layout of a grid in a sliver.
///
/// This struct describes how children are arranged in a grid, including
/// the number of columns, spacing, and child sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliverGridLayout {
    /// The number of children in the cross axis.
    pub cross_axis_count: usize,

    /// The distance between the start of one child and the start of the next
    /// in the main axis (includes child extent and spacing).
    pub main_axis_stride: f32,

    /// The distance between the start of one child and the start of the next
    /// in the cross axis (includes child extent and spacing).
    pub cross_axis_stride: f32,

    /// The extent of children in the main axis.
    pub child_main_axis_extent: f32,

    /// The extent of children in the cross axis.
    pub child_cross_axis_extent: f32,

    /// Whether the cross axis should be laid out in reverse order.
    pub reverse_cross_axis: bool,
}

impl SliverGridLayout {
    /// Returns the scroll offset of the child at the given index.
    pub fn get_scroll_offset_of_child(&self, index: usize) -> f32 {
        let row = index / self.cross_axis_count;
        row as f32 * self.main_axis_stride
    }

    /// Returns the cross axis offset of the child at the given index.
    pub fn get_cross_axis_offset_of_child(&self, index: usize, cross_axis_extent: f32) -> f32 {
        let column = index % self.cross_axis_count;
        let offset = column as f32 * self.cross_axis_stride;

        if self.reverse_cross_axis {
            cross_axis_extent - offset - self.child_cross_axis_extent
        } else {
            offset
        }
    }

    /// Returns the minimum index of children visible at the given scroll offset.
    pub fn get_min_child_index_for_scroll_offset(&self, scroll_offset: f32) -> usize {
        if self.main_axis_stride <= 0.0 {
            return 0;
        }
        let row = (scroll_offset / self.main_axis_stride).floor() as usize;
        row * self.cross_axis_count
    }

    /// Returns the maximum index of children visible at the given scroll offset.
    pub fn get_max_child_index_for_scroll_offset(&self, scroll_offset: f32) -> usize {
        if self.main_axis_stride <= 0.0 {
            return 0;
        }
        let row = (scroll_offset / self.main_axis_stride).ceil() as usize;
        (row + 1) * self.cross_axis_count - 1
    }
}

/// A delegate that defines grid layout in slivers.
///
/// Implement this trait to control how items are arranged in a grid
/// within a scrollable sliver.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::{SliverGridDelegate, SliverGridLayout};
/// use flui_types::SliverConstraints;
///
/// #[derive(Debug)]
/// struct FixedCountGridDelegate {
///     cross_axis_count: usize,
///     main_axis_spacing: f32,
///     cross_axis_spacing: f32,
///     child_aspect_ratio: f32,
/// }
///
/// impl SliverGridDelegate for FixedCountGridDelegate {
///     fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
///         let used_cross_axis = self.cross_axis_spacing * (self.cross_axis_count - 1) as f32;
///         let child_cross_axis_extent =
///             (constraints.cross_axis_extent - used_cross_axis) / self.cross_axis_count as f32;
///         let child_main_axis_extent = child_cross_axis_extent / self.child_aspect_ratio;
///
///         SliverGridLayout {
///             cross_axis_count: self.cross_axis_count,
///             main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
///             cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
///             child_main_axis_extent,
///             child_cross_axis_extent,
///             reverse_cross_axis: false,
///         }
///     }
///
///     fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.cross_axis_count != old.cross_axis_count
///         } else {
///             true
///         }
///     }
/// }
/// ```
pub trait SliverGridDelegate: Send + Sync + Debug {
    /// Get the grid layout for the given constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The sliver constraints from the viewport
    ///
    /// # Returns
    ///
    /// The grid layout configuration.
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout;

    /// Whether to relayout when the delegate changes.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous delegate
    ///
    /// # Returns
    ///
    /// `true` if layout should be recalculated, `false` otherwise.
    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool;

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// A grid delegate with a fixed number of columns.
#[derive(Debug, Clone, Copy)]
pub struct SliverGridDelegateWithFixedCrossAxisCount {
    /// The number of children in the cross axis.
    pub cross_axis_count: usize,

    /// The spacing between children in the main axis.
    pub main_axis_spacing: f32,

    /// The spacing between children in the cross axis.
    pub cross_axis_spacing: f32,

    /// The ratio of the cross-axis to the main-axis extent of each child.
    pub child_aspect_ratio: f32,
}

impl SliverGridDelegateWithFixedCrossAxisCount {
    /// Creates a new delegate with the given cross axis count.
    pub fn new(cross_axis_count: usize) -> Self {
        Self {
            cross_axis_count,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
            child_aspect_ratio: 1.0,
        }
    }

    /// Sets the main axis spacing.
    pub fn with_main_axis_spacing(mut self, spacing: f32) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Sets the cross axis spacing.
    pub fn with_cross_axis_spacing(mut self, spacing: f32) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }

    /// Sets the child aspect ratio.
    pub fn with_child_aspect_ratio(mut self, ratio: f32) -> Self {
        self.child_aspect_ratio = ratio;
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateWithFixedCrossAxisCount {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        let used_cross_axis = self.cross_axis_spacing * (self.cross_axis_count - 1) as f32;
        let child_cross_axis_extent =
            (constraints.cross_axis_extent - used_cross_axis) / self.cross_axis_count as f32;
        let child_main_axis_extent = child_cross_axis_extent / self.child_aspect_ratio;

        SliverGridLayout {
            cross_axis_count: self.cross_axis_count,
            main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
            cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
            child_main_axis_extent,
            child_cross_axis_extent,
            reverse_cross_axis: false,
        }
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
            self.cross_axis_count != old.cross_axis_count
                || (self.main_axis_spacing - old.main_axis_spacing).abs() > f32::EPSILON
                || (self.cross_axis_spacing - old.cross_axis_spacing).abs() > f32::EPSILON
                || (self.child_aspect_ratio - old.child_aspect_ratio).abs() > f32::EPSILON
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A grid delegate with a maximum cross axis extent for each child.
#[derive(Debug, Clone, Copy)]
pub struct SliverGridDelegateWithMaxCrossAxisExtent {
    /// The maximum extent of children in the cross axis.
    pub max_cross_axis_extent: f32,

    /// The spacing between children in the main axis.
    pub main_axis_spacing: f32,

    /// The spacing between children in the cross axis.
    pub cross_axis_spacing: f32,

    /// The ratio of the cross-axis to the main-axis extent of each child.
    pub child_aspect_ratio: f32,
}

impl SliverGridDelegateWithMaxCrossAxisExtent {
    /// Creates a new delegate with the given maximum cross axis extent.
    pub fn new(max_cross_axis_extent: f32) -> Self {
        Self {
            max_cross_axis_extent,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
            child_aspect_ratio: 1.0,
        }
    }

    /// Sets the main axis spacing.
    pub fn with_main_axis_spacing(mut self, spacing: f32) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Sets the cross axis spacing.
    pub fn with_cross_axis_spacing(mut self, spacing: f32) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }

    /// Sets the child aspect ratio.
    pub fn with_child_aspect_ratio(mut self, ratio: f32) -> Self {
        self.child_aspect_ratio = ratio;
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateWithMaxCrossAxisExtent {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        // Calculate the number of columns that fit
        let cross_axis_count = ((constraints.cross_axis_extent + self.cross_axis_spacing)
            / (self.max_cross_axis_extent + self.cross_axis_spacing))
            .ceil()
            .max(1.0) as usize;

        // Use the fixed count logic with calculated count
        let used_cross_axis = self.cross_axis_spacing * (cross_axis_count - 1) as f32;
        let child_cross_axis_extent =
            (constraints.cross_axis_extent - used_cross_axis) / cross_axis_count as f32;
        let child_main_axis_extent = child_cross_axis_extent / self.child_aspect_ratio;

        SliverGridLayout {
            cross_axis_count,
            main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
            cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
            child_main_axis_extent,
            child_cross_axis_extent,
            reverse_cross_axis: false,
        }
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
            (self.max_cross_axis_extent - old.max_cross_axis_extent).abs() > f32::EPSILON
                || (self.main_axis_spacing - old.main_axis_spacing).abs() > f32::EPSILON
                || (self.cross_axis_spacing - old.cross_axis_spacing).abs() > f32::EPSILON
                || (self.child_aspect_ratio - old.child_aspect_ratio).abs() > f32::EPSILON
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::GrowthDirection;
    use flui_types::layout::{Axis, AxisDirection};

    fn make_constraints(cross_axis_extent: f32) -> SliverConstraints {
        SliverConstraints::new(
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Axis::Vertical,
            0.0,               // scroll_offset
            1000.0,            // remaining_paint_extent
            1000.0,            // viewport_main_axis_extent
            cross_axis_extent, // cross_axis_extent
        )
    }

    #[test]
    fn test_fixed_cross_axis_count() {
        let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(3)
            .with_cross_axis_spacing(10.0)
            .with_main_axis_spacing(10.0);

        let constraints = make_constraints(320.0); // 320 - 20 (2 spacings) = 300, 300 / 3 = 100
        let layout = delegate.get_layout(constraints);

        assert_eq!(layout.cross_axis_count, 3);
        assert_eq!(layout.child_cross_axis_extent, 100.0);
        assert_eq!(layout.child_main_axis_extent, 100.0);
        assert_eq!(layout.cross_axis_stride, 110.0);
        assert_eq!(layout.main_axis_stride, 110.0);
    }

    #[test]
    fn test_max_cross_axis_extent() {
        let delegate = SliverGridDelegateWithMaxCrossAxisExtent::new(100.0)
            .with_cross_axis_spacing(10.0)
            .with_main_axis_spacing(10.0);

        let constraints = make_constraints(320.0);
        let layout = delegate.get_layout(constraints);

        // Should fit 3 columns: (320 + 10) / (100 + 10) = 3
        assert_eq!(layout.cross_axis_count, 3);
    }

    #[test]
    fn test_grid_layout_scroll_offset() {
        let layout = SliverGridLayout {
            cross_axis_count: 3,
            main_axis_stride: 110.0,
            cross_axis_stride: 110.0,
            child_main_axis_extent: 100.0,
            child_cross_axis_extent: 100.0,
            reverse_cross_axis: false,
        };

        // Row 0: indices 0, 1, 2
        assert_eq!(layout.get_scroll_offset_of_child(0), 0.0);
        assert_eq!(layout.get_scroll_offset_of_child(1), 0.0);
        assert_eq!(layout.get_scroll_offset_of_child(2), 0.0);

        // Row 1: indices 3, 4, 5
        assert_eq!(layout.get_scroll_offset_of_child(3), 110.0);
        assert_eq!(layout.get_scroll_offset_of_child(4), 110.0);
        assert_eq!(layout.get_scroll_offset_of_child(5), 110.0);

        // Row 2: indices 6, 7, 8
        assert_eq!(layout.get_scroll_offset_of_child(6), 220.0);
    }

    #[test]
    fn test_grid_layout_cross_axis_offset() {
        let layout = SliverGridLayout {
            cross_axis_count: 3,
            main_axis_stride: 110.0,
            cross_axis_stride: 110.0,
            child_main_axis_extent: 100.0,
            child_cross_axis_extent: 100.0,
            reverse_cross_axis: false,
        };

        assert_eq!(layout.get_cross_axis_offset_of_child(0, 320.0), 0.0);
        assert_eq!(layout.get_cross_axis_offset_of_child(1, 320.0), 110.0);
        assert_eq!(layout.get_cross_axis_offset_of_child(2, 320.0), 220.0);
        assert_eq!(layout.get_cross_axis_offset_of_child(3, 320.0), 0.0); // Next row
    }

    #[test]
    fn test_grid_layout_min_max_index() {
        let layout = SliverGridLayout {
            cross_axis_count: 3,
            main_axis_stride: 110.0,
            cross_axis_stride: 110.0,
            child_main_axis_extent: 100.0,
            child_cross_axis_extent: 100.0,
            reverse_cross_axis: false,
        };

        // At scroll offset 0, first visible row is 0
        assert_eq!(layout.get_min_child_index_for_scroll_offset(0.0), 0);
        assert_eq!(layout.get_max_child_index_for_scroll_offset(0.0), 2);

        // At scroll offset 110, first visible row is 1
        assert_eq!(layout.get_min_child_index_for_scroll_offset(110.0), 3);
        assert_eq!(layout.get_max_child_index_for_scroll_offset(110.0), 5);
    }

    #[test]
    fn test_should_relayout() {
        let delegate1 = SliverGridDelegateWithFixedCrossAxisCount::new(3);
        let delegate2 = SliverGridDelegateWithFixedCrossAxisCount::new(3);
        let delegate3 = SliverGridDelegateWithFixedCrossAxisCount::new(4);

        assert!(!delegate1.should_relayout(&delegate2));
        assert!(delegate1.should_relayout(&delegate3));
    }
}
