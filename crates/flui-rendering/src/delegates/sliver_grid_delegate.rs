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

    /// Returns the cross-axis offset (leading edge) of the child at `index`.
    ///
    /// Flutter parity: `SliverGridRegularTileLayout.getGeometryForChildIndex` /
    /// `_getOffsetFromStartInCrossAxis`
    /// (`.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_grid.dart`).
    /// The cross-axis offset is determined entirely by the layout's own fields
    /// (`cross_axis_count`, `cross_axis_stride`, `child_cross_axis_extent`); the
    /// prior signature took an external `cross_axis_extent` and dropped the
    /// `(stride − child_extent)` spacing term, so reversed grids were off by the
    /// cross-axis spacing.
    pub fn get_cross_axis_offset_of_child(&self, index: usize) -> f32 {
        let column = index % self.cross_axis_count;
        let cross_axis_start = column as f32 * self.cross_axis_stride;

        if self.reverse_cross_axis {
            // Oracle: crossAxisCount*stride − start − childExtent − (stride − childExtent),
            // which simplifies to (crossAxisCount − 1 − column) * stride.
            let total = self.cross_axis_count as f32 * self.cross_axis_stride;
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
    pub fn get_min_child_index_for_scroll_offset(&self, scroll_offset: f32) -> usize {
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
    /// Flutter parity: `SliverGridRegularTileLayout.getMaxChildIndexForScrollOffset`
    /// (`.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_grid.dart`),
    /// `max(0, crossAxisCount * ceil(scrollOffset / mainAxisStride) - 1)`. The
    /// prior `(row + 1) * crossAxisCount - 1` form over-counted by one full row.
    pub fn get_max_child_index_for_scroll_offset(&self, scroll_offset: f32) -> usize {
        if self.main_axis_stride <= 0.0 {
            return 0;
        }
        let main_axis_count = (scroll_offset / self.main_axis_stride).ceil() as usize;
        // `saturating_sub` is the `usize` form of Flutter's `math.max(0, … - 1)`:
        // at `scroll_offset == 0` the count is 0 and the result clamps to 0.
        (self.cross_axis_count * main_axis_count).saturating_sub(1)
    }

    /// Returns the maximum scroll extent for a grid with `child_count` items.
    ///
    /// Flutter parity: `SliverGridRegularTileLayout.computeMaxScrollOffset`
    /// (`.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_grid.dart:257-266`).
    ///
    /// The result is the scroll offset of the trailing edge of the last row:
    /// `main_axis_stride * row_count - main_axis_spacing`, where
    /// `main_axis_spacing = main_axis_stride - child_main_axis_extent`.
    pub fn compute_max_scroll_offset(&self, child_count: usize) -> f32 {
        if child_count == 0 {
            return 0.0;
        }
        let row_count = ((child_count - 1) / self.cross_axis_count) + 1;
        let main_axis_spacing = self.main_axis_stride - self.child_main_axis_extent;
        self.main_axis_stride * row_count as f32 - main_axis_spacing
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
    ///
    /// Ignored when [`main_axis_extent`](Self::main_axis_extent) is set.
    pub child_aspect_ratio: f32,

    /// Explicit main-axis extent per child. When `Some`, it overrides
    /// `child_aspect_ratio`; when `None`, the main-axis extent is derived from
    /// the aspect ratio (Flutter's `mainAxisExtent`).
    pub main_axis_extent: Option<f32>,
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

    /// Sets an explicit main-axis extent per child, overriding the aspect ratio.
    pub fn with_main_axis_extent(mut self, extent: f32) -> Self {
        self.main_axis_extent = Some(extent);
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateWithFixedCrossAxisCount {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        // Flutter SliverGridDelegateWithFixedCrossAxisCount.getLayout
        // (.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_grid.dart:392)
        // clamps the usable cross extent at 0 so heavy cross-axis spacing can't
        // drive the per-child extent negative.
        let used_cross_axis = self.cross_axis_spacing * (self.cross_axis_count - 1) as f32;
        let usable_cross_axis_extent = (constraints.cross_axis_extent - used_cross_axis).max(0.0);
        let child_cross_axis_extent = usable_cross_axis_extent / self.cross_axis_count as f32;
        // Flutter: `mainAxisExtent ?? childCrossAxisExtent / childAspectRatio`.
        let child_main_axis_extent = self
            .main_axis_extent
            .unwrap_or(child_cross_axis_extent / self.child_aspect_ratio);

        SliverGridLayout {
            cross_axis_count: self.cross_axis_count,
            main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
            cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
            child_main_axis_extent,
            child_cross_axis_extent,
            // Flutter derives this from the cross-axis direction
            // (axisDirectionIsReversed), not a hardcoded false.
            reverse_cross_axis: constraints.cross_axis_direction.is_reversed(),
        }
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
            self.cross_axis_count != old.cross_axis_count
                || (self.main_axis_spacing - old.main_axis_spacing).abs() > f32::EPSILON
                || (self.cross_axis_spacing - old.cross_axis_spacing).abs() > f32::EPSILON
                || (self.child_aspect_ratio - old.child_aspect_ratio).abs() > f32::EPSILON
                // Exact bit compare so any change to the explicit override (incl.
                // Some<->None) forces relayout without tripping float-cmp lints.
                || self.main_axis_extent.map(f32::to_bits) != old.main_axis_extent.map(f32::to_bits)
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
    ///
    /// Ignored when [`main_axis_extent`](Self::main_axis_extent) is set.
    pub child_aspect_ratio: f32,

    /// Explicit main-axis extent per child. When `Some`, it overrides
    /// `child_aspect_ratio` (Flutter's `mainAxisExtent`).
    pub main_axis_extent: Option<f32>,
}

impl SliverGridDelegateWithMaxCrossAxisExtent {
    /// Creates a new delegate with the given maximum cross axis extent.
    pub fn new(max_cross_axis_extent: f32) -> Self {
        Self {
            max_cross_axis_extent,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
            child_aspect_ratio: 1.0,
            main_axis_extent: None,
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

    /// Sets an explicit main-axis extent per child, overriding the aspect ratio.
    pub fn with_main_axis_extent(mut self, extent: f32) -> Self {
        self.main_axis_extent = Some(extent);
        self
    }
}

impl SliverGridDelegate for SliverGridDelegateWithMaxCrossAxisExtent {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        // Flutter SliverGridDelegateWithMaxCrossAxisExtent.getLayout
        // (.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_grid.dart:502):
        // count = ceil(crossAxisExtent / (maxCrossAxisExtent + crossAxisSpacing)),
        // floored at 1. The numerator is the bare cross extent — adding the
        // spacing there (as the prior code did) over-counts columns by one when
        // the extent is an exact multiple of the denominator.
        let cross_axis_count = (constraints.cross_axis_extent
            / (self.max_cross_axis_extent + self.cross_axis_spacing))
            .ceil()
            .max(1.0) as usize;

        // Use the fixed-count logic with the calculated count, clamping the
        // usable cross extent at 0 to match the oracle.
        let used_cross_axis = self.cross_axis_spacing * (cross_axis_count - 1) as f32;
        let usable_cross_axis_extent = (constraints.cross_axis_extent - used_cross_axis).max(0.0);
        let child_cross_axis_extent = usable_cross_axis_extent / cross_axis_count as f32;
        // Flutter: `mainAxisExtent ?? childCrossAxisExtent / childAspectRatio`.
        let child_main_axis_extent = self
            .main_axis_extent
            .unwrap_or(child_cross_axis_extent / self.child_aspect_ratio);

        SliverGridLayout {
            cross_axis_count,
            main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
            cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
            child_main_axis_extent,
            child_cross_axis_extent,
            // Flutter derives this from the cross-axis direction
            // (axisDirectionIsReversed), not a hardcoded false.
            reverse_cross_axis: constraints.cross_axis_direction.is_reversed(),
        }
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
            (self.max_cross_axis_extent - old.max_cross_axis_extent).abs() > f32::EPSILON
                || (self.main_axis_spacing - old.main_axis_spacing).abs() > f32::EPSILON
                || (self.cross_axis_spacing - old.cross_axis_spacing).abs() > f32::EPSILON
                || (self.child_aspect_ratio - old.child_aspect_ratio).abs() > f32::EPSILON
                || self.main_axis_extent.map(f32::to_bits) != old.main_axis_extent.map(f32::to_bits)
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

    fn make_constraints(cross_axis_extent: f32) -> SliverConstraints {
        SliverConstraints {
            scroll_offset: 0.0,
            remaining_paint_extent: 1000.0,
            viewport_main_axis_extent: 1000.0,
            cross_axis_extent,
            ..Default::default()
        }
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

        // Oracle: ceil(crossAxisExtent / (max + spacing)) = ceil(320 / 110) = 3.
        assert_eq!(layout.cross_axis_count, 3);
    }

    #[test]
    fn test_max_cross_axis_extent_exact_multiple_does_not_over_count() {
        // Regression: the prior numerator `(extent + spacing)` over-counted by
        // one column when the extent was an exact multiple of (max + spacing).
        // Oracle: ceil(220 / (100 + 10)) = ceil(2.0) = 2 (not 3).
        let delegate =
            SliverGridDelegateWithMaxCrossAxisExtent::new(100.0).with_cross_axis_spacing(10.0);
        let layout = delegate.get_layout(make_constraints(220.0));
        assert_eq!(layout.cross_axis_count, 2);
    }

    #[test]
    fn test_fixed_count_clamps_usable_extent_at_zero() {
        // Cross-axis spacing larger than the available extent must not drive the
        // per-child extent negative; the oracle clamps usable extent at 0.
        let delegate =
            SliverGridDelegateWithFixedCrossAxisCount::new(3).with_cross_axis_spacing(200.0); // used = 200 * 2 = 400 > 100
        let layout = delegate.get_layout(make_constraints(100.0));
        assert_eq!(layout.child_cross_axis_extent, 0.0);
        assert!(layout.child_main_axis_extent >= 0.0);
    }

    #[test]
    fn test_get_layout_wires_reverse_cross_axis_from_direction() {
        use flui_types::layout::AxisDirection;

        let delegate = SliverGridDelegateWithFixedCrossAxisCount::new(3);

        // Default (LeftToRight) cross axis → forward layout.
        let forward = delegate.get_layout(make_constraints(320.0));
        assert!(!forward.reverse_cross_axis);
        assert_eq!(forward.get_cross_axis_offset_of_child(0), 0.0);

        // RightToLeft cross axis → reversed (Flutter axisDirectionIsReversed),
        // which activates the mirrored cross-axis offsets. Before this wiring
        // `get_layout` hardcoded `false`, so the reversed path was unreachable.
        // 330 / 3 = 110 exactly, so the mirror arithmetic stays free of f32
        // rounding noise.
        let constraints = SliverConstraints {
            scroll_offset: 0.0,
            remaining_paint_extent: 1000.0,
            viewport_main_axis_extent: 1000.0,
            cross_axis_extent: 330.0,
            cross_axis_direction: AxisDirection::RightToLeft,
            ..Default::default()
        };
        let reversed = delegate.get_layout(constraints);
        assert!(reversed.reverse_cross_axis);
        assert_eq!(reversed.cross_axis_stride, 110.0);
        // Mirror: column 0 sits at the far cross end, column 2 at the near end.
        assert_eq!(reversed.get_cross_axis_offset_of_child(0), 220.0);
        assert_eq!(reversed.get_cross_axis_offset_of_child(2), 0.0);
    }

    #[test]
    fn test_main_axis_extent_overrides_aspect_ratio() {
        // Flutter `mainAxisExtent`: when set, the per-child main extent is taken
        // verbatim and the aspect ratio is ignored.

        // Fixed-count: aspect 0.5 alone would give main extent 200 (100 / 0.5);
        // the explicit 150 must win.
        let fixed = SliverGridDelegateWithFixedCrossAxisCount::new(2)
            .with_child_aspect_ratio(0.5)
            .with_main_axis_extent(150.0)
            .with_main_axis_spacing(10.0);
        let layout = fixed.get_layout(make_constraints(200.0));
        assert_eq!(layout.child_main_axis_extent, 150.0);
        assert_eq!(layout.main_axis_stride, 160.0); // 150 + 10 spacing

        // Max-extent: aspect 2.0 alone would give 50 (100 / 2.0); the explicit
        // 80 must win.
        let maxed = SliverGridDelegateWithMaxCrossAxisExtent::new(100.0)
            .with_child_aspect_ratio(2.0)
            .with_main_axis_extent(80.0);
        let layout = maxed.get_layout(make_constraints(200.0));
        assert_eq!(layout.child_main_axis_extent, 80.0);
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

        assert_eq!(layout.get_cross_axis_offset_of_child(0), 0.0);
        assert_eq!(layout.get_cross_axis_offset_of_child(1), 110.0);
        assert_eq!(layout.get_cross_axis_offset_of_child(2), 220.0);
        assert_eq!(layout.get_cross_axis_offset_of_child(3), 0.0); // Next row
    }

    #[test]
    fn test_grid_layout_cross_axis_offset_reversed() {
        // reverse_cross_axis mirrors columns: column c sits where column
        // (cross_axis_count - 1 - c) sits in the forward layout. Verified
        // against Flutter's `_getOffsetFromStartInCrossAxis`
        // (.flutter/.../rendering/sliver_grid.dart). With a 10px cross-axis
        // spacing (stride 110, child extent 100), the prior formula was off
        // by that 10px.
        let layout = SliverGridLayout {
            cross_axis_count: 3,
            main_axis_stride: 110.0,
            cross_axis_stride: 110.0,
            child_main_axis_extent: 100.0,
            child_cross_axis_extent: 100.0,
            reverse_cross_axis: true,
        };

        assert_eq!(layout.get_cross_axis_offset_of_child(0), 220.0); // col 0 → far end
        assert_eq!(layout.get_cross_axis_offset_of_child(1), 110.0); // col 1 → middle
        assert_eq!(layout.get_cross_axis_offset_of_child(2), 0.0); // col 2 → near end
        assert_eq!(layout.get_cross_axis_offset_of_child(3), 220.0); // next row, col 0
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

        // `get_min_*` and `get_max_*` are independent formulas; real callers
        // pass different offsets to each (scrollOffset vs targetEndScrollOffset).
        // Values verified against Flutter's SliverGridRegularTileLayout
        // (`.flutter/.../rendering/sliver_grid.dart`):
        //   min = crossAxisCount * (offset ~/ stride)
        //   max = max(0, crossAxisCount * ceil(offset / stride) - 1)

        // min: first child of the row containing the offset.
        assert_eq!(layout.get_min_child_index_for_scroll_offset(0.0), 0);
        assert_eq!(layout.get_min_child_index_for_scroll_offset(110.0), 3);

        // max: ceil(offset/stride) rows fit, so last child is count*ceil - 1.
        // At offset 0 no full row is above it → child 0 (oracle clamps to 0).
        assert_eq!(layout.get_max_child_index_for_scroll_offset(0.0), 0);
        // targetEnd == one stride → exactly row 0 above it → last child 2.
        assert_eq!(layout.get_max_child_index_for_scroll_offset(110.0), 2);
        // targetEnd spanning two full rows → children 0..=5.
        assert_eq!(layout.get_max_child_index_for_scroll_offset(220.0), 5);
        // A partial row rounds up (ceil) → still two rows → children 0..=5.
        assert_eq!(layout.get_max_child_index_for_scroll_offset(165.0), 5);
    }

    #[test]
    fn test_should_relayout() {
        let delegate1 = SliverGridDelegateWithFixedCrossAxisCount::new(3);
        let delegate2 = SliverGridDelegateWithFixedCrossAxisCount::new(3);
        let delegate3 = SliverGridDelegateWithFixedCrossAxisCount::new(4);

        assert!(!delegate1.should_relayout(&delegate2));
        assert!(delegate1.should_relayout(&delegate3));
    }

    // Oracle: `.flutter/.../rendering/sliver_grid.dart:257-266`
    // Layout: stride=100, child_main=100, cross_count=2, no spacing.
    fn two_column_layout() -> SliverGridLayout {
        SliverGridLayout {
            cross_axis_count: 2,
            main_axis_stride: 100.0,
            cross_axis_stride: 100.0,
            child_main_axis_extent: 100.0,
            child_cross_axis_extent: 100.0,
            reverse_cross_axis: false,
        }
    }

    #[test]
    fn compute_max_scroll_offset_zero_children_returns_zero() {
        assert_eq!(two_column_layout().compute_max_scroll_offset(0), 0.0);
    }

    #[test]
    fn compute_max_scroll_offset_one_child_fills_one_row() {
        // 1 child → 1 row; spacing=0 → stride * 1 - 0 = 100
        assert_eq!(two_column_layout().compute_max_scroll_offset(1), 100.0);
    }

    #[test]
    fn compute_max_scroll_offset_six_children_fills_three_rows() {
        // 6 children → rows = (6-1)/2 + 1 = 3; result = 100*3 - 0 = 300
        assert_eq!(two_column_layout().compute_max_scroll_offset(6), 300.0);
    }

    #[test]
    fn compute_max_scroll_offset_seven_children_fills_four_rows() {
        // 7 children → rows = (7-1)/2 + 1 = 4; result = 100*4 - 0 = 400
        assert_eq!(two_column_layout().compute_max_scroll_offset(7), 400.0);
    }

    #[test]
    fn compute_max_scroll_offset_eight_children_same_as_seven() {
        // 8 children → rows = (8-1)/2 + 1 = 4 (still 4 rows); result = 400
        assert_eq!(two_column_layout().compute_max_scroll_offset(8), 400.0);
    }

    #[test]
    fn compute_max_scroll_offset_subtracts_main_axis_spacing() {
        // stride=120, child_main=100, spacing=20; cross=2
        // 4 children → rows=2 → 120*2 - 20 = 220
        let layout = SliverGridLayout {
            cross_axis_count: 2,
            main_axis_stride: 120.0,
            cross_axis_stride: 100.0,
            child_main_axis_extent: 100.0,
            child_cross_axis_extent: 100.0,
            reverse_cross_axis: false,
        };
        assert_eq!(layout.compute_max_scroll_offset(4), 220.0);
    }
}
