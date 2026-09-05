//! Sliver protocol parent data variants - Specialized types for scrollable
//! layouts.

use std::hash::{Hash, Hasher};

use flui_foundation::RenderId;
use flui_types::Offset;

use super::{base::ParentData, container_mixin::ContainerParentDataMixin};

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

/// The pair a lazy sliver hands down to each materialised child.
///
/// Both halves travel together by construction. Keeping them in one value is
/// the point: the failure this exists to prevent is a semantic position that
/// drifts from the row it describes, which is exactly what happens when the two
/// are threaded, stamped, or defaulted independently.
///
/// Minted by the sparse host, inherited through however many component
/// elements sit between it and the child's first render descendant, and
/// consumed once at adopt time — Flutter's `didAdoptChild` slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliverSlot {
    /// The logical index: what layout and the band walk key on.
    pub logical: usize,
    /// The position within the semantic set, or `None` for a child that is not
    /// a member of it — see
    /// [`SliverMultiBoxAdaptorParentData::semantic_index`].
    pub semantic: Option<i32>,
}

/// A logical index as a semantic position, or `None` if it does not fit.
///
/// The platform property is `i32`. Casting past its range wraps, and a wrapped
/// position is a *wrong* announcement — "item 3 of 100" on the four-billionth
/// row — where `None` is an honest "item ? of 100". A list that long is not one
/// a screen reader can navigate anyway, so nothing is lost by declining.
const fn semantic_position(logical: usize) -> Option<i32> {
    if logical <= i32::MAX as usize {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_possible_wrap,
            reason = "guarded by the bound immediately above"
        )]
        Some(logical as i32)
    } else {
        None
    }
}

impl SliverSlot {
    /// A slot whose semantic position is its logical index.
    ///
    /// The 1:1 case, which is every delegate FLUI ships today.
    #[must_use]
    pub const fn identity(logical: usize) -> Self {
        Self {
            logical,
            semantic: semantic_position(logical),
        }
    }

    /// A slot for a child that occupies a logical index without being a member
    /// of the semantic set — a separator, a header the reader should not count.
    #[must_use]
    pub const fn unindexed(logical: usize) -> Self {
        Self {
            logical,
            semantic: None,
        }
    }
}

/// Parent data for sliver multi-box adaptor children (SliverList, etc).
///
/// Combines logical offset, index, and keep-alive functionality.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverMultiBoxAdaptorParentData {
    /// Logical offset in scrollable axis.
    pub layout_offset: f32,

    /// Index of this child in the list.
    pub index: usize,

    /// This child's position within the *semantic* set, when it is a member.
    ///
    /// Distinct from [`Self::index`], which is the LOGICAL index layout and the
    /// band walk key on. The two coincide for a delegate that materialises one
    /// set member per logical index — every delegate FLUI ships today — and
    /// diverge for any that interleaves non-members, the way Flutter's
    /// `ListView.separated` puts separators at odd logical indices. `None`
    /// means "not a member of the set": a separator has a logical index and no
    /// position to announce.
    ///
    /// Carried beside the logical index rather than derived from it, because
    /// the delegate is the only thing that knows which is which and the
    /// semantics assembler that publishes the position never sees the delegate.
    pub semantic_index: Option<i32>,
}

impl SliverMultiBoxAdaptorParentData {
    /// Create with a logical index that is also its semantic position.
    pub const fn new(index: usize) -> Self {
        Self {
            layout_offset: 0.0,
            index,
            semantic_index: semantic_position(index),
        }
    }

    /// Create with a logical index and an explicit semantic position.
    ///
    /// `None` marks a child that is not a member of the set.
    #[must_use]
    pub const fn with_semantic_index(index: usize, semantic_index: Option<i32>) -> Self {
        Self {
            layout_offset: 0.0,
            index,
            semantic_index,
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
        // Both halves, or neither. Moving the logical index while leaving the
        // semantic one behind is exactly the drift these two travel together to
        // prevent -- `zero().with_index(9)` would otherwise keep announcing the
        // position it held at index 0. A caller that needs them to differ says
        // so through [`Self::with_semantic_index`].
        self.semantic_index = semantic_position(index);
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
    }
}

impl crate::parent_data::base::ParentData for SliverMultiBoxAdaptorParentData {}

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

    /// Depth in tree (0 = root, 1 = child, etc).
    pub depth: usize,
}

impl TreeSliverNodeParentData {
    /// Create with index and depth.
    pub const fn new(index: usize, depth: usize) -> Self {
        Self {
            layout_offset: 0.0,
            index,
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
        self.depth.hash(state);
    }
}

impl crate::parent_data::base::ParentData for TreeSliverNodeParentData {}

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
    /// `with_index` moves BOTH halves.
    ///
    /// Moving the logical index while leaving the semantic one behind is the
    /// drift the pair exists to prevent, and this builder is where it can
    /// happen — the constructors derive both from one value and cannot
    /// disagree with themselves.
    #[test]
    fn with_index_moves_the_semantic_position_too() {
        let moved = SliverMultiBoxAdaptorParentData::zero().with_index(9);
        assert_eq!(
            (moved.index, moved.semantic_index),
            (9, Some(9)),
            "`Some(0)` here is the position the child held before the move, \
             which would announce it as item 1 rather than item 10"
        );
    }

    /// A caller that wants the two to differ has to say so.
    #[test]
    fn with_semantic_index_is_how_the_two_are_allowed_to_differ() {
        let separator = SliverMultiBoxAdaptorParentData::with_semantic_index(7, None);
        assert_eq!((separator.index, separator.semantic_index), (7, None));
    }

    /// A logical index past `i32::MAX` declines a position rather than wrapping.
    ///
    /// The platform property is `i32`, and a wrapped cast announces a *wrong*
    /// position — the honest answer is none at all.
    #[test]
    fn an_index_too_large_for_the_platform_property_declines() {
        let representable = SliverSlot::identity(i32::MAX as usize);
        assert_eq!(representable.semantic, Some(i32::MAX));

        let past_the_end = SliverSlot::identity(i32::MAX as usize + 1);
        assert_eq!(
            past_the_end.semantic, None,
            "wrapping would announce a small, plausible, and wrong position"
        );
        assert_eq!(
            past_the_end.logical,
            i32::MAX as usize + 1,
            "the logical index is untouched -- layout still keys on it"
        );
    }
}
