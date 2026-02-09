//! Sliver protocol parent data variants - Specialized types for scrollable layouts.

use flui_types::Offset;
use std::hash::{Hash, Hasher};

use super::base::ParentData;
use super::container_mixin::ContainerParentDataMixin;
use super::keep_alive_mixin::KeepAliveParentDataMixin;
use flui_foundation::RenderId;

// ============================================================================
// SLIVER LOGICAL PARENT DATA (Base)
// ============================================================================

/// Parent data for sliver children storing logical scroll offset.
///
/// This is the base for sliver parent data types that track position
/// in the scrollable axis.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverLogicalParentData {
    /// Logical offset in scrollable axis.
    pub layout_offset: f32,
}

impl SliverLogicalParentData {
    /// Create with specific layout offset.
    pub const fn new(layout_offset: f32) -> Self {
        Self { layout_offset }
    }

    /// Create at origin.
    pub const fn zero() -> Self {
        Self::new(0.0)
    }

    /// Builder: set layout offset.
    pub const fn with_layout_offset(mut self, offset: f32) -> Self {
        self.layout_offset = offset;
        self
    }

    /// Check if at origin.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.layout_offset == 0.0
    }

    /// Reset to origin.
    pub fn reset(&mut self) {
        self.layout_offset = 0.0;
    }
}

impl Default for SliverLogicalParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for SliverLogicalParentData {}

impl Hash for SliverLogicalParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
    }
}

impl Eq for SliverLogicalParentData {}

// ============================================================================
// SLIVER MULTI BOX ADAPTOR PARENT DATA
// ============================================================================

/// Parent data for sliver multi-box adaptor children (SliverList, etc).
///
/// Combines logical offset, index, and keep-alive functionality.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverMultiBoxAdaptorParentData {
    /// Logical offset in scrollable axis.
    pub layout_offset: f32,

    /// Index of this child in the list.
    pub index: usize,

    /// Keep-alive mixin for AutomaticKeepAlive support.
    pub keep_alive: KeepAliveParentDataMixin,
}

impl SliverMultiBoxAdaptorParentData {
    /// Create with index.
    pub const fn new(index: usize) -> Self {
        Self {
            layout_offset: 0.0,
            index,
            keep_alive: KeepAliveParentDataMixin::new(),
        }
    }

    /// Create at origin with index 0.
    pub const fn zero() -> Self {
        Self::new(0)
    }

    /// Builder: set layout offset.
    pub const fn with_layout_offset(mut self, offset: f32) -> Self {
        self.layout_offset = offset;
        self
    }

    /// Builder: set index.
    pub const fn with_index(mut self, index: usize) -> Self {
        self.index = index;
        self
    }

    /// Check if at origin.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.layout_offset == 0.0
    }
}

impl Default for SliverMultiBoxAdaptorParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for SliverMultiBoxAdaptorParentData {}

impl Hash for SliverMultiBoxAdaptorParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
        self.index.hash(state);
        self.keep_alive.hash(state);
    }
}

// ============================================================================
// SLIVER GRID PARENT DATA
// ============================================================================

/// Parent data for sliver grid children.
///
/// Extends `SliverMultiBoxAdaptorParentData` with cross-axis offset.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverGridParentData {
    /// Logical offset in scrollable axis.
    pub layout_offset: f32,

    /// Index of this child in the grid.
    pub index: usize,

    /// Keep-alive mixin.
    pub keep_alive: KeepAliveParentDataMixin,

    /// Offset in cross axis (for grid positioning).
    pub cross_axis_offset: f32,
}

impl SliverGridParentData {
    /// Create with index and cross-axis offset.
    pub const fn new(index: usize, cross_axis_offset: f32) -> Self {
        Self {
            layout_offset: 0.0,
            index,
            keep_alive: KeepAliveParentDataMixin::new(),
            cross_axis_offset,
        }
    }

    /// Create at origin.
    pub const fn zero() -> Self {
        Self::new(0, 0.0)
    }

    /// Builder: set layout offset.
    pub const fn with_layout_offset(mut self, offset: f32) -> Self {
        self.layout_offset = offset;
        self
    }

    /// Builder: set cross-axis offset.
    pub const fn with_cross_axis_offset(mut self, offset: f32) -> Self {
        self.cross_axis_offset = offset;
        self
    }
}

impl Default for SliverGridParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for SliverGridParentData {}

impl Hash for SliverGridParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
        self.index.hash(state);
        self.keep_alive.hash(state);
        self.cross_axis_offset.to_bits().hash(state);
    }
}

// ============================================================================
// TREE SLIVER NODE PARENT DATA
// ============================================================================

/// Parent data for tree sliver nodes (expandable tree views).
///
/// Extends `SliverMultiBoxAdaptorParentData` with depth in tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeSliverNodeParentData {
    /// Logical offset in scrollable axis.
    pub layout_offset: f32,

    /// Index of this child in the tree.
    pub index: usize,

    /// Keep-alive mixin.
    pub keep_alive: KeepAliveParentDataMixin,

    /// Depth in tree (0 = root, 1 = child, etc).
    pub depth: usize,
}

impl TreeSliverNodeParentData {
    /// Create with index and depth.
    pub const fn new(index: usize, depth: usize) -> Self {
        Self {
            layout_offset: 0.0,
            index,
            keep_alive: KeepAliveParentDataMixin::new(),
            depth,
        }
    }

    /// Create at origin with depth 0.
    pub const fn zero() -> Self {
        Self::new(0, 0)
    }

    /// Builder: set depth.
    pub const fn with_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Check if this is a root node.
    #[inline]
    pub const fn is_root(&self) -> bool {
        self.depth == 0
    }
}

impl Default for TreeSliverNodeParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for TreeSliverNodeParentData {}

impl Hash for TreeSliverNodeParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
        self.index.hash(state);
        self.keep_alive.hash(state);
        self.depth.hash(state);
    }
}

// ============================================================================
// SLIVER LOGICAL CONTAINER PARENT DATA
// ============================================================================

/// Parent data for sliver containers with logical positioning.
///
/// Combines logical offset with container mixin for sibling pointers.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverLogicalContainerParentData {
    /// Logical offset in scrollable axis.
    pub layout_offset: f32,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,
}

impl SliverLogicalContainerParentData {
    /// Create with layout offset.
    pub const fn new(layout_offset: f32) -> Self {
        Self {
            layout_offset,
            container: ContainerParentDataMixin::new(),
        }
    }

    /// Create at origin.
    pub const fn zero() -> Self {
        Self::new(0.0)
    }

    /// Builder: set layout offset.
    pub const fn with_layout_offset(mut self, offset: f32) -> Self {
        self.layout_offset = offset;
        self
    }
}

impl Default for SliverLogicalContainerParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for SliverLogicalContainerParentData {}

impl Hash for SliverLogicalContainerParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
        self.container.hash(state);
    }
}

// ============================================================================
// SLIVER PHYSICAL PARENT DATA
// ============================================================================

/// Parent data for sliver children with physical paint offset.
///
/// Unlike logical offset, paint offset is the actual position where
/// the child should be painted relative to the viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverPhysicalParentData {
    /// Physical paint offset from viewport origin.
    pub paint_offset: Offset,
}

impl SliverPhysicalParentData {
    /// Create with paint offset.
    pub const fn new(paint_offset: Offset) -> Self {
        Self { paint_offset }
    }

    /// Create at origin.
    pub const fn zero() -> Self {
        Self::new(Offset::ZERO)
    }

    /// Builder: set paint offset.
    pub const fn with_paint_offset(mut self, offset: Offset) -> Self {
        self.paint_offset = offset;
        self
    }

    /// Check if at origin.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.paint_offset == Offset::ZERO
    }
}

impl Default for SliverPhysicalParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for SliverPhysicalParentData {}

impl Hash for SliverPhysicalParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.paint_offset.dx.to_bits().hash(state);
        self.paint_offset.dy.to_bits().hash(state);
    }
}

impl Eq for SliverPhysicalParentData {}

// ============================================================================
// SLIVER PHYSICAL CONTAINER PARENT DATA
// ============================================================================

/// Parent data for sliver containers with physical positioning.
///
/// Combines physical paint offset with container mixin.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverPhysicalContainerParentData {
    /// Physical paint offset from viewport origin.
    pub paint_offset: Offset,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,
}

impl SliverPhysicalContainerParentData {
    /// Create with paint offset.
    pub const fn new(paint_offset: Offset) -> Self {
        Self {
            paint_offset,
            container: ContainerParentDataMixin::new(),
        }
    }

    /// Create at origin.
    pub const fn zero() -> Self {
        Self::new(Offset::ZERO)
    }

    /// Builder: set paint offset.
    pub const fn with_paint_offset(mut self, offset: Offset) -> Self {
        self.paint_offset = offset;
        self
    }
}

impl Default for SliverPhysicalContainerParentData {
    fn default() -> Self {
        Self::zero()
    }
}

impl ParentData for SliverPhysicalContainerParentData {}

impl Hash for SliverPhysicalContainerParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.paint_offset.dx.to_bits().hash(state);
        self.paint_offset.dy.to_bits().hash(state);
        self.container.hash(state);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_logical_parent_data() {
        let data = SliverLogicalParentData::new(100.0);
        assert_eq!(data.layout_offset, 100.0);
    }

    #[test]
    fn test_sliver_multi_box_adaptor_parent_data() {
        let data = SliverMultiBoxAdaptorParentData::new(5).with_layout_offset(100.0);

        assert_eq!(data.index, 5);
        assert_eq!(data.layout_offset, 100.0);
    }

    #[test]
    fn test_sliver_grid_parent_data() {
        let data = SliverGridParentData::new(3, 50.0).with_layout_offset(100.0);

        assert_eq!(data.index, 3);
        assert_eq!(data.cross_axis_offset, 50.0);
    }

    #[test]
    fn test_tree_sliver_node_parent_data() {
        let data = TreeSliverNodeParentData::new(0, 0);
        assert!(data.is_root());

        let child = TreeSliverNodeParentData::new(1, 1);
        assert!(!child.is_root());
    }

    #[test]
    fn test_sliver_physical_parent_data() {
        let data = SliverPhysicalParentData::new(Offset::new(10.0, 20.0));
        assert_eq!(data.paint_offset.dx, 10.0);
    }
}
