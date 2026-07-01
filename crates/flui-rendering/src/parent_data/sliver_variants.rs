//! Sliver protocol parent data variants - Specialized types for scrollable
//! layouts.

use std::hash::{Hash, Hasher};

use flui_foundation::RenderId;
use flui_types::Offset;

use super::{
    base::ParentData, container_mixin::ContainerParentDataMixin,
    keep_alive_mixin::KeepAliveParentDataMixin,
};

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

impl Hash for SliverMultiBoxAdaptorParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
        self.index.hash(state);
        self.keep_alive.hash(state);
    }
}

impl crate::parent_data::base::LogicalIndexParentData for SliverMultiBoxAdaptorParentData {
    fn set_logical_index(&mut self, index: usize) {
        self.index = index;
    }
}

impl crate::parent_data::base::ParentData for SliverMultiBoxAdaptorParentData {
    // `private_interfaces`: `LogicalIndexParentData` is `pub(crate)` by design;
    // this method is pipeline-internal plumbing, not part of the public contract.
    #[allow(private_interfaces)]
    fn as_logical_index_mut(
        &mut self,
    ) -> Option<&mut dyn crate::parent_data::base::LogicalIndexParentData> {
        Some(self)
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

impl Hash for SliverGridParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
        self.index.hash(state);
        self.keep_alive.hash(state);
        self.cross_axis_offset.to_bits().hash(state);
    }
}

impl crate::parent_data::base::LogicalIndexParentData for SliverGridParentData {
    fn set_logical_index(&mut self, index: usize) {
        self.index = index;
    }
}

impl crate::parent_data::base::ParentData for SliverGridParentData {
    // `private_interfaces`: `LogicalIndexParentData` is `pub(crate)` by design;
    // this method is pipeline-internal plumbing, not part of the public contract.
    #[allow(private_interfaces)]
    fn as_logical_index_mut(
        &mut self,
    ) -> Option<&mut dyn crate::parent_data::base::LogicalIndexParentData> {
        Some(self)
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

impl Hash for TreeSliverNodeParentData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout_offset.to_bits().hash(state);
        self.index.hash(state);
        self.keep_alive.hash(state);
        self.depth.hash(state);
    }
}

impl crate::parent_data::base::LogicalIndexParentData for TreeSliverNodeParentData {
    fn set_logical_index(&mut self, index: usize) {
        self.index = index;
    }
}

impl crate::parent_data::base::ParentData for TreeSliverNodeParentData {
    // `private_interfaces`: `LogicalIndexParentData` is `pub(crate)` by design;
    // this method is pipeline-internal plumbing, not part of the public contract.
    #[allow(private_interfaces)]
    fn as_logical_index_mut(
        &mut self,
    ) -> Option<&mut dyn crate::parent_data::base::LogicalIndexParentData> {
        Some(self)
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
    use std::hash::{DefaultHasher, Hash, Hasher};

    use flui_types::geometry::px;

    use super::*;
    use crate::parent_data::base::LogicalIndexParentData;

    fn hash_of<T: Hash>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn test_sliver_logical_parent_data() {
        let data = SliverLogicalParentData::new(100.0);
        assert_eq!(data.layout_offset, 100.0);
    }

    #[test]
    fn sliver_logical_parent_data_zero_reset_and_default() {
        assert_eq!(
            SliverLogicalParentData::default(),
            SliverLogicalParentData::zero()
        );
        assert!(SliverLogicalParentData::zero().is_zero());

        let mut data = SliverLogicalParentData::new(42.0).with_layout_offset(7.0);
        assert_eq!(data.layout_offset, 7.0);
        assert!(!data.is_zero());

        data.reset();
        assert!(data.is_zero());
        assert_eq!(data, SliverLogicalParentData::zero());
    }

    #[test]
    fn sliver_logical_parent_data_hash_matches_for_equal_values() {
        let a = SliverLogicalParentData::new(12.5);
        let b = SliverLogicalParentData::new(12.5);
        let c = SliverLogicalParentData::new(3.0);

        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&c));
    }

    #[test]
    fn test_sliver_multi_box_adaptor_parent_data() {
        let data = SliverMultiBoxAdaptorParentData::new(5).with_layout_offset(100.0);

        assert_eq!(data.index, 5);
        assert_eq!(data.layout_offset, 100.0);
    }

    #[test]
    fn sliver_multi_box_adaptor_parent_data_zero_default_and_index_builder() {
        assert_eq!(
            SliverMultiBoxAdaptorParentData::default(),
            SliverMultiBoxAdaptorParentData::zero()
        );
        assert!(SliverMultiBoxAdaptorParentData::zero().is_zero());
        assert_eq!(SliverMultiBoxAdaptorParentData::zero().index, 0);

        let data = SliverMultiBoxAdaptorParentData::zero().with_index(9);
        assert_eq!(data.index, 9);
        assert!(data.is_zero());

        let moved = data.with_layout_offset(3.0);
        assert!(!moved.is_zero());
    }

    #[test]
    fn sliver_multi_box_adaptor_parent_data_hash_matches_for_equal_values() {
        let a = SliverMultiBoxAdaptorParentData::new(2).with_layout_offset(1.0);
        let b = SliverMultiBoxAdaptorParentData::new(2).with_layout_offset(1.0);
        let c = SliverMultiBoxAdaptorParentData::new(3).with_layout_offset(1.0);

        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&c));
    }

    #[test]
    fn sliver_multi_box_adaptor_parent_data_logical_index_channel() {
        let mut data = SliverMultiBoxAdaptorParentData::new(1);

        // Exercise the concrete inherent setter directly.
        data.set_logical_index(5);
        assert_eq!(data.index, 5);

        // Exercise the type-erased `ParentData::as_logical_index_mut` channel
        // the pipeline uses to reach `set_logical_index` through `dyn ParentData`.
        let erased: &mut dyn ParentData = &mut data;
        let logical = erased
            .as_logical_index_mut()
            .expect("multi-box adaptor parent data must expose the logical-index channel");
        logical.set_logical_index(11);
        assert_eq!(data.index, 11);
    }

    #[test]
    fn test_sliver_grid_parent_data() {
        let data = SliverGridParentData::new(3, 50.0).with_layout_offset(100.0);

        assert_eq!(data.index, 3);
        assert_eq!(data.cross_axis_offset, 50.0);
    }

    #[test]
    fn sliver_grid_parent_data_zero_default_and_cross_axis_builder() {
        assert_eq!(
            SliverGridParentData::default(),
            SliverGridParentData::zero()
        );
        assert_eq!(SliverGridParentData::zero().index, 0);
        assert_eq!(SliverGridParentData::zero().cross_axis_offset, 0.0);

        let data = SliverGridParentData::zero().with_cross_axis_offset(25.0);
        assert_eq!(data.cross_axis_offset, 25.0);
        assert_eq!(data.layout_offset, 0.0);
    }

    #[test]
    fn sliver_grid_parent_data_hash_matches_for_equal_values() {
        let a = SliverGridParentData::new(1, 2.0);
        let b = SliverGridParentData::new(1, 2.0);
        let c = SliverGridParentData::new(1, 3.0);

        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&c));
    }

    #[test]
    fn sliver_grid_parent_data_logical_index_channel() {
        let mut data = SliverGridParentData::new(0, 0.0);
        let erased: &mut dyn ParentData = &mut data;
        let logical = erased
            .as_logical_index_mut()
            .expect("grid parent data must expose the logical-index channel");
        logical.set_logical_index(7);
        assert_eq!(data.index, 7);
    }

    #[test]
    fn test_tree_sliver_node_parent_data() {
        let data = TreeSliverNodeParentData::new(0, 0);
        assert!(data.is_root());

        let child = TreeSliverNodeParentData::new(1, 1);
        assert!(!child.is_root());
    }

    #[test]
    fn tree_sliver_node_parent_data_zero_default_and_depth_builder() {
        assert_eq!(
            TreeSliverNodeParentData::default(),
            TreeSliverNodeParentData::zero()
        );
        assert!(TreeSliverNodeParentData::zero().is_root());

        let data = TreeSliverNodeParentData::new(2, 0).with_depth(4);
        assert_eq!(data.depth, 4);
        assert!(!data.is_root());
    }

    #[test]
    fn tree_sliver_node_parent_data_hash_matches_for_equal_values() {
        let a = TreeSliverNodeParentData::new(1, 2);
        let b = TreeSliverNodeParentData::new(1, 2);
        let c = TreeSliverNodeParentData::new(1, 3);

        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&c));
    }

    #[test]
    fn tree_sliver_node_parent_data_logical_index_channel() {
        let mut data = TreeSliverNodeParentData::new(0, 3);
        let erased: &mut dyn ParentData = &mut data;
        let logical = erased
            .as_logical_index_mut()
            .expect("tree sliver node parent data must expose the logical-index channel");
        logical.set_logical_index(9);
        assert_eq!(data.index, 9);
        // Setting the logical index must not disturb unrelated fields.
        assert_eq!(data.depth, 3);
    }

    #[test]
    fn sliver_logical_container_parent_data_construction_and_builders() {
        assert_eq!(
            SliverLogicalContainerParentData::default(),
            SliverLogicalContainerParentData::zero()
        );

        let zero = SliverLogicalContainerParentData::zero();
        assert_eq!(zero.layout_offset, 0.0);
        assert!(zero.container.is_first_child());
        assert!(zero.container.is_last_child());
        assert!(!zero.container.has_previous_sibling());
        assert!(!zero.container.has_next_sibling());

        let data = SliverLogicalContainerParentData::new(10.0).with_layout_offset(20.0);
        assert_eq!(data.layout_offset, 20.0);
    }

    #[test]
    fn sliver_logical_container_parent_data_hash_matches_for_equal_values() {
        let a = SliverLogicalContainerParentData::new(1.0);
        let b = SliverLogicalContainerParentData::new(1.0);
        let c = SliverLogicalContainerParentData::new(2.0);

        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&c));
    }

    #[test]
    fn test_sliver_physical_parent_data() {
        let data = SliverPhysicalParentData::new(Offset::new(px(10.0), px(20.0)));
        assert_eq!(data.paint_offset.dx, px(10.0));
    }

    #[test]
    fn sliver_physical_parent_data_zero_default_and_builder() {
        assert_eq!(
            SliverPhysicalParentData::default(),
            SliverPhysicalParentData::zero()
        );
        assert!(SliverPhysicalParentData::zero().is_zero());

        let data =
            SliverPhysicalParentData::zero().with_paint_offset(Offset::new(px(5.0), px(6.0)));
        assert!(!data.is_zero());
        assert_eq!(data.paint_offset, Offset::new(px(5.0), px(6.0)));
    }

    #[test]
    fn sliver_physical_parent_data_hash_matches_for_equal_values() {
        let a = SliverPhysicalParentData::new(Offset::new(px(1.0), px(2.0)));
        let b = SliverPhysicalParentData::new(Offset::new(px(1.0), px(2.0)));
        let c = SliverPhysicalParentData::new(Offset::new(px(1.0), px(3.0)));

        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&c));
    }

    #[test]
    fn sliver_physical_container_parent_data_construction_and_builders() {
        assert_eq!(
            SliverPhysicalContainerParentData::default(),
            SliverPhysicalContainerParentData::zero()
        );

        let zero = SliverPhysicalContainerParentData::zero();
        assert_eq!(zero.paint_offset, Offset::ZERO);
        assert!(zero.container.is_first_child());
        assert!(zero.container.is_last_child());

        let data = SliverPhysicalContainerParentData::new(Offset::new(px(1.0), px(2.0)))
            .with_paint_offset(Offset::new(px(3.0), px(4.0)));
        assert_eq!(data.paint_offset, Offset::new(px(3.0), px(4.0)));
    }

    #[test]
    fn sliver_physical_container_parent_data_hash_matches_for_equal_values() {
        let a = SliverPhysicalContainerParentData::new(Offset::new(px(1.0), px(2.0)));
        let b = SliverPhysicalContainerParentData::new(Offset::new(px(1.0), px(2.0)));
        let c = SliverPhysicalContainerParentData::new(Offset::new(px(9.0), px(2.0)));

        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&c));
    }
}
