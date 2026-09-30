//! Sliver protocol parent data variants - Specialized types for scrollable
//! layouts.

use std::hash::{Hash, Hasher};

use flui_foundation::RenderId;
use flui_foundation::geometry::{Offset, canonical_bits_f64};

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
    pub layout_offset: f64,
}

impl SliverLogicalParentData {
    /// Create with specific layout offset.
    pub const fn new(layout_offset: f64) -> Self {
        Self { layout_offset }
    }

    /// Create at origin.
    pub const fn zero() -> Self {
        Self::new(0.0)
    }

    /// Builder: set layout offset.
    pub const fn with_layout_offset(mut self, offset: f64) -> Self {
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
        canonical_bits_f64(self.layout_offset).hash(state);
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
/// consumed once at adopt time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SliverSlot {
    /// The logical index: what layout and the band walk key on.
    pub logical: usize,
    /// The position within the semantic set, or `None` for a child that is not
    /// a member of it — see
    /// [`SliverMultiBoxAdaptorParentData::semantic_index`].
    pub semantic: Option<i32>,
    /// How many members the set has, or `None` while that is not yet known.
    ///
    /// Rides with the position because a screen reader announces them as one
    /// phrase — "item 12 of 100" — and AccessKit's `position_in_set` and
    /// `size_of_set` describe the SAME node, so a total held anywhere else is
    /// invisible to a reader querying the focused row.
    ///
    /// `None` under an unresolved `ItemCount::Unknown`, which degrades to
    /// "item 12 of ?". A missing total is honest; a wrong one misleads.
    pub set_size: Option<i32>,
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
            set_size: None,
        }
    }

    /// This slot with the set's member count attached.
    #[must_use]
    pub const fn with_set_size(mut self, set_size: Option<i32>) -> Self {
        self.set_size = set_size;
        self
    }

    /// A slot for a child that occupies a logical index without being a member
    /// of the semantic set — a separator, a header the reader should not count.
    #[must_use]
    pub const fn unindexed(logical: usize) -> Self {
        Self {
            logical,
            semantic: None,
            set_size: None,
        }
    }
}

/// Parent data for sliver multi-box adaptor children (SliverList, etc).
///
/// Combines logical offset, index, and keep-alive functionality.
#[derive(Debug, Clone, PartialEq)]
pub struct SliverMultiBoxAdaptorParentData {
    /// Logical offset in scrollable axis.
    pub layout_offset: f64,

    /// Index of this child in the list.
    pub index: usize,

    /// This child's position within the *semantic* set, when it is a member.
    ///
    /// Distinct from [`Self::index`], which is the LOGICAL index layout and the
    /// band walk key on. The two coincide for a delegate that materialises one
    /// set member per logical index — every delegate FLUI ships today — and
    /// diverge for any that interleaves non-members, such as a separated
    /// list that puts separators at odd logical indices. `None`
    /// means "not a member of the set": a separator has a logical index and no
    /// position to announce.
    ///
    /// Carried beside the logical index rather than derived from it, because
    /// the delegate is the only thing that knows which is which and the
    /// semantics assembler that publishes the position never sees the delegate.
    pub semantic_index: Option<i32>,

    /// How many members the semantic set has, published beside
    /// [`Self::semantic_index`] — see [`SliverSlot::set_size`].
    pub semantic_set_size: Option<i32>,
}

impl SliverMultiBoxAdaptorParentData {
    /// Create with a logical index that is also its semantic position.
    pub const fn new(index: usize) -> Self {
        Self {
            layout_offset: 0.0,
            index,
            semantic_index: semantic_position(index),
            semantic_set_size: None,
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
            semantic_set_size: None,
        }
    }

    /// Create at origin with index 0.
    pub const fn zero() -> Self {
        Self::new(0)
    }

    /// Builder: set layout offset.
    pub const fn with_layout_offset(mut self, offset: f64) -> Self {
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
        canonical_bits_f64(self.layout_offset).hash(state);
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
    pub layout_offset: f64,

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
        canonical_bits_f64(self.layout_offset).hash(state);
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
    pub layout_offset: f64,

    /// Container mixin for sibling pointers.
    pub container: ContainerParentDataMixin<RenderId>,
}

impl SliverLogicalContainerParentData {
    /// Create with layout offset.
    pub const fn new(layout_offset: f64) -> Self {
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
    pub const fn with_layout_offset(mut self, offset: f64) -> Self {
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
        canonical_bits_f64(self.layout_offset).hash(state);
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
        canonical_bits_f64(self.paint_offset.dx).hash(state);
        canonical_bits_f64(self.paint_offset.dy).hash(state);
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
        canonical_bits_f64(self.paint_offset.dx).hash(state);
        canonical_bits_f64(self.paint_offset.dy).hash(state);
        self.container.hash(state);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
}
