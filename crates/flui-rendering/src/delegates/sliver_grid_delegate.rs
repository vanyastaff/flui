//! Sliver grid delegate for grid layout in slivers.
//!
//! [`SliverGridDelegate`] allows users to define grid layout algorithms
//! for slivers, controlling the number of columns, spacing, and child sizes.

use std::{any::Any, fmt::Debug};

use crate::constraints::SliverConstraints;

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
    pub main_axis_stride: f64,

    /// The distance between the start of one child and the start of the next
    /// in the cross axis (includes child extent and spacing).
    pub cross_axis_stride: f64,

    /// The extent of children in the main axis.
    pub child_main_axis_extent: f64,

    /// The extent of children in the cross axis.
    pub child_cross_axis_extent: f64,

    /// Whether the cross axis should be laid out in reverse order.
    pub reverse_cross_axis: bool,
}

impl SliverGridLayout {
    /// Returns the scroll offset of the child at the given index.
    pub fn get_scroll_offset_of_child(&self, index: usize) -> f64 {
        let row = index / self.cross_axis_count;
        row as f64 * self.main_axis_stride
    }

    /// Returns the cross-axis offset (leading edge) of the child at `index`.
    ///
    /// The cross-axis offset is determined entirely by the layout's own fields
    /// (`cross_axis_count`, `cross_axis_stride`, `child_cross_axis_extent`); an
    /// external `cross_axis_extent` argument would drop the
    /// `(stride − child_extent)` spacing term, leaving reversed grids off by the
    /// cross-axis spacing.
    pub fn get_cross_axis_offset_of_child(&self, index: usize) -> f64 {
        let column = index % self.cross_axis_count;
        let cross_axis_start = column as f64 * self.cross_axis_stride;

        if self.reverse_cross_axis {
            // crossAxisCount*stride − start − childExtent − (stride − childExtent),
            // which simplifies to (crossAxisCount − 1 − column) * stride.
            let total = self.cross_axis_count as f64 * self.cross_axis_stride;
            total
                - cross_axis_start
                - self.child_cross_axis_extent
                - (self.cross_axis_stride - self.child_cross_axis_extent)
        } else {
            cross_axis_start
        }
    }

    /// Returns the minimum index of children visible at the given scroll
    /// offset.
    pub fn get_min_child_index_for_scroll_offset(&self, scroll_offset: f64) -> usize {
        if self.main_axis_stride <= 0.0 {
            return 0;
        }
        let row = (scroll_offset / self.main_axis_stride).floor() as usize;
        row * self.cross_axis_count
    }

    /// Returns the maximum child index reachable by the given scroll offset.
    ///
    /// Callers pass the *trailing* edge of the visible-plus-cache region
    /// (`targetEndScrollOffset`); the result is the last child of the rows whose
    /// top lies strictly above that offset.
    ///
    /// The index is `max(0, crossAxisCount * ceil(scrollOffset / mainAxisStride) - 1)`;
    /// a `(row + 1) * crossAxisCount - 1` form would over-count by one full row.
    pub fn get_max_child_index_for_scroll_offset(&self, scroll_offset: f64) -> usize {
        if self.main_axis_stride <= 0.0 {
            return 0;
        }
        let main_axis_count = (scroll_offset / self.main_axis_stride).ceil() as usize;
        // `saturating_sub` is the `usize` form of `max(0, … - 1)`:
        // at `scroll_offset == 0` the count is 0 and the result clamps to 0.
        (self.cross_axis_count * main_axis_count).saturating_sub(1)
    }

    /// Returns the maximum scroll extent for a grid with `child_count` items.
    ///
    /// The result is the scroll offset of the trailing edge of the last row:
    /// `main_axis_stride * row_count - main_axis_spacing`, where
    /// `main_axis_spacing = main_axis_stride - child_main_axis_extent`.
    pub fn compute_max_scroll_offset(&self, child_count: usize) -> f64 {
        if child_count == 0 {
            return 0.0;
        }
        let row_count = ((child_count - 1) / self.cross_axis_count) + 1;
        let main_axis_spacing = self.main_axis_stride - self.child_main_axis_extent;
        self.main_axis_stride * row_count as f64 - main_axis_spacing
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
/// use flui_rendering::constraints::SliverConstraints;
///
/// #[derive(Debug)]
/// struct FixedCountGridDelegate {
///     cross_axis_count: usize,
///     main_axis_spacing: f64,
///     cross_axis_spacing: f64,
///     child_aspect_ratio: f64,
/// }
///
/// impl SliverGridDelegate for FixedCountGridDelegate {
///     fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
///         let used_cross_axis = self.cross_axis_spacing * (self.cross_axis_count - 1) as f64;
///         let child_cross_axis_extent =
///             (constraints.cross_axis_extent - used_cross_axis) / self.cross_axis_count as f64;
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
    pub main_axis_spacing: f64,

    /// The spacing between children in the cross axis.
    pub cross_axis_spacing: f64,

    /// The ratio of the cross-axis to the main-axis extent of each child.
    ///
    /// Ignored when [`main_axis_extent`](Self::main_axis_extent) is set.
    pub child_aspect_ratio: f64,

    /// Explicit main-axis extent per child. When `Some`, it overrides
    /// `child_aspect_ratio`; when `None`, the main-axis extent is derived from
    /// the aspect ratio.
    pub main_axis_extent: Option<f64>,
}

impl SliverGridDelegateWithFixedCrossAxisCount {
    /// Creates a new delegate with the given cross axis count.
    pub fn new(cross_axis_count: usize) -> Self {
        Self {
            cross_axis_count,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
            child_aspect_ratio: 1.0,
            main_axis_extent: None,
        }
    }

    /// Sets the main axis spacing.
    pub fn with_main_axis_spacing(mut self, spacing: f64) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Sets the cross axis spacing.
    pub fn with_cross_axis_spacing(mut self, spacing: f64) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }

    /// Sets the child aspect ratio.
    pub fn with_child_aspect_ratio(mut self, ratio: f64) -> Self {
        self.child_aspect_ratio = ratio;
        self
    }

    /// Sets an explicit main-axis extent per child, overriding the aspect ratio.
    pub fn with_main_axis_extent(mut self, extent: f64) -> Self {
        self.main_axis_extent = Some(extent);
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateWithFixedCrossAxisCount {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        // The usable cross extent is clamped at 0 so heavy cross-axis spacing
        // can't drive the per-child extent negative.
        let used_cross_axis = self.cross_axis_spacing * (self.cross_axis_count - 1) as f64;
        let usable_cross_axis_extent = (constraints.cross_axis_extent - used_cross_axis).max(0.0);
        let child_cross_axis_extent = usable_cross_axis_extent / self.cross_axis_count as f64;
        // `main_axis_extent`, else `child_cross_axis_extent / child_aspect_ratio`.
        let child_main_axis_extent = self
            .main_axis_extent
            .unwrap_or(child_cross_axis_extent / self.child_aspect_ratio);

        SliverGridLayout {
            cross_axis_count: self.cross_axis_count,
            main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
            cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
            child_main_axis_extent,
            child_cross_axis_extent,
            // Derived from the cross-axis direction, not a hardcoded false.
            reverse_cross_axis: constraints.cross_axis_direction.is_reversed(),
        }
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
            self.cross_axis_count != old.cross_axis_count
                || (self.main_axis_spacing - old.main_axis_spacing).abs() > f64::EPSILON
                || (self.cross_axis_spacing - old.cross_axis_spacing).abs() > f64::EPSILON
                || (self.child_aspect_ratio - old.child_aspect_ratio).abs() > f64::EPSILON
                // Exact bit compare so any change to the explicit override (incl.
                // Some<->None) forces relayout without tripping float-cmp lints.
                || self.main_axis_extent.map(f64::to_bits) != old.main_axis_extent.map(f64::to_bits)
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
    pub max_cross_axis_extent: f64,

    /// The spacing between children in the main axis.
    pub main_axis_spacing: f64,

    /// The spacing between children in the cross axis.
    pub cross_axis_spacing: f64,

    /// The ratio of the cross-axis to the main-axis extent of each child.
    ///
    /// Ignored when [`main_axis_extent`](Self::main_axis_extent) is set.
    pub child_aspect_ratio: f64,

    /// Explicit main-axis extent per child. When `Some`, it overrides
    /// `child_aspect_ratio`.
    pub main_axis_extent: Option<f64>,
}

impl SliverGridDelegateWithMaxCrossAxisExtent {
    /// Creates a new delegate with the given maximum cross axis extent.
    pub fn new(max_cross_axis_extent: f64) -> Self {
        Self {
            max_cross_axis_extent,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
            child_aspect_ratio: 1.0,
            main_axis_extent: None,
        }
    }

    /// Sets the main axis spacing.
    pub fn with_main_axis_spacing(mut self, spacing: f64) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Sets the cross axis spacing.
    pub fn with_cross_axis_spacing(mut self, spacing: f64) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }

    /// Sets the child aspect ratio.
    pub fn with_child_aspect_ratio(mut self, ratio: f64) -> Self {
        self.child_aspect_ratio = ratio;
        self
    }

    /// Sets an explicit main-axis extent per child, overriding the aspect ratio.
    pub fn with_main_axis_extent(mut self, extent: f64) -> Self {
        self.main_axis_extent = Some(extent);
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateWithMaxCrossAxisExtent {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        // count = ceil(crossAxisExtent / (maxCrossAxisExtent + crossAxisSpacing)),
        // floored at 1. The numerator is the bare cross extent — adding the
        // spacing there would over-count columns by one when the extent is an
        // exact multiple of the denominator.
        let cross_axis_count = (constraints.cross_axis_extent
            / (self.max_cross_axis_extent + self.cross_axis_spacing))
            .ceil()
            .max(1.0) as usize;

        // Use the fixed-count logic with the calculated count, clamping the
        // usable cross extent at 0.
        let used_cross_axis = self.cross_axis_spacing * (cross_axis_count - 1) as f64;
        let usable_cross_axis_extent = (constraints.cross_axis_extent - used_cross_axis).max(0.0);
        let child_cross_axis_extent = usable_cross_axis_extent / cross_axis_count as f64;
        // `main_axis_extent`, else `child_cross_axis_extent / child_aspect_ratio`.
        let child_main_axis_extent = self
            .main_axis_extent
            .unwrap_or(child_cross_axis_extent / self.child_aspect_ratio);

        SliverGridLayout {
            cross_axis_count,
            main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
            cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
            child_main_axis_extent,
            child_cross_axis_extent,
            // Derived from the cross-axis direction, not a hardcoded false.
            reverse_cross_axis: constraints.cross_axis_direction.is_reversed(),
        }
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
            (self.max_cross_axis_extent - old.max_cross_axis_extent).abs() > f64::EPSILON
                || (self.main_axis_spacing - old.main_axis_spacing).abs() > f64::EPSILON
                || (self.cross_axis_spacing - old.cross_axis_spacing).abs() > f64::EPSILON
                || (self.child_aspect_ratio - old.child_aspect_ratio).abs() > f64::EPSILON
                || self.main_axis_extent.map(f64::to_bits) != old.main_axis_extent.map(f64::to_bits)
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {}
