//! Sliver parent data types.

use flui_types::Offset;

// ============================================================================
// SliverParentData (Logical)
// ============================================================================

/// Parent data for slivers that position children using layout offsets.
///
/// Optimized for fast layout. Best used by parents that expect many children
/// whose relative positions don't change even when scroll offset does.
///
/// # Flutter Equivalence
///
/// ```dart
/// class SliverLogicalParentData extends ParentData {
///   double? layoutOffset;
/// }
/// ```
///
/// # Example
///
/// ```ignore
/// use flui_rendering::parent_data::SliverParentData;
///
/// let mut data = SliverParentData::default();
/// data.layout_offset = Some(100.0);
/// ```
#[derive(Debug, Clone, Default)]
pub struct SliverParentData {
    /// The position of the child relative to the zero scroll offset.
    ///
    /// The number of pixels from the zero scroll offset of the parent sliver
    /// to the side of the child closest to that offset.
    ///
    /// In a typical list, this does not change as the parent is scrolled.
    pub layout_offset: Option<f32>,
}

impl SliverParentData {
    /// Creates SliverParentData with the given layout offset.
    #[inline]
    pub fn new(layout_offset: f32) -> Self {
        Self {
            layout_offset: Some(layout_offset),
        }
    }
}

crate::impl_parent_data!(SliverParentData);

// ============================================================================
// SliverPhysicalParentData
// ============================================================================

/// Parent data for slivers that position children using absolute coordinates.
///
/// Optimized for fast painting at the cost of additional work during layout.
/// Best used by parents with few children that are tall relative to parent.
///
/// # Flutter Equivalence
///
/// ```dart
/// class SliverPhysicalParentData extends ParentData {
///   Offset paintOffset = Offset.zero;
///   int? crossAxisFlex;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct SliverPhysicalParentData {
    /// The position of the child relative to the parent.
    ///
    /// Distance from top-left visible corner of parent to top-left
    /// visible corner of the sliver.
    pub paint_offset: Offset,

    /// The cross-axis flex factor for this sliver child.
    ///
    /// Used by SliverCrossAxisGroup to determine how to allocate
    /// cross-axis extent to children.
    ///
    /// If None or zero, child is inflexible and determines its own
    /// size in the cross axis.
    pub cross_axis_flex: Option<u32>,
}

impl SliverPhysicalParentData {
    /// Creates SliverPhysicalParentData with the given paint offset.
    #[inline]
    pub fn new(paint_offset: Offset) -> Self {
        Self {
            paint_offset,
            cross_axis_flex: None,
        }
    }
}

crate::impl_parent_data!(SliverPhysicalParentData);

// ============================================================================
// SliverMultiBoxAdaptorParentData
// ============================================================================

/// Parent data for children of sliver multi-box adaptors (lists, grids).
///
/// Combines logical parent data with an index for efficient child management.
///
/// # Flutter Equivalence
///
/// ```dart
/// class SliverMultiBoxAdaptorParentData extends SliverLogicalParentData
///     with ContainerParentDataMixin<RenderBox>, KeepAliveParentDataMixin {
///   int? index;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct SliverMultiBoxAdaptorParentData {
    /// The position of the child relative to the zero scroll offset.
    pub layout_offset: Option<f32>,

    /// The index of this child in the parent's child list.
    pub index: Option<usize>,

    /// Whether to keep this child alive even when scrolled out of view.
    pub keep_alive: bool,
}

impl SliverMultiBoxAdaptorParentData {
    /// Creates new parent data with the given index.
    #[inline]
    pub fn new(index: usize) -> Self {
        Self {
            layout_offset: None,
            index: Some(index),
            keep_alive: false,
        }
    }
}

crate::impl_parent_data!(SliverMultiBoxAdaptorParentData);

// ============================================================================
// SliverGridParentData
// ============================================================================

/// Parent data for children of sliver grids.
///
/// Extends [`SliverMultiBoxAdaptorParentData`] with cross-axis offset.
///
/// # Flutter Equivalence
///
/// ```dart
/// class SliverGridParentData extends SliverMultiBoxAdaptorParentData {
///   double? crossAxisOffset;
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct SliverGridParentData {
    /// The position along the main axis.
    pub layout_offset: Option<f32>,

    /// The index of this child.
    pub index: Option<usize>,

    /// Whether to keep this child alive.
    pub keep_alive: bool,

    /// The offset of the child in the cross axis direction.
    pub cross_axis_offset: Option<f32>,
}

crate::impl_parent_data!(SliverGridParentData);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_parent_data_default() {
        let data = SliverParentData::default();
        assert_eq!(data.layout_offset, None);
    }

    #[test]
    fn test_sliver_physical_parent_data() {
        let data = SliverPhysicalParentData::new(Offset::new(10.0, 20.0));
        assert_eq!(data.paint_offset, Offset::new(10.0, 20.0));
        assert_eq!(data.cross_axis_flex, None);
    }

    #[test]
    fn test_sliver_multi_box_adaptor_parent_data() {
        let data = SliverMultiBoxAdaptorParentData::new(5);
        assert_eq!(data.index, Some(5));
        assert!(!data.keep_alive);
    }
}
