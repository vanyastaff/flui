//! Hit test context for pointer event detection.
//!
//! This module provides [`HitTestContext`] - a GAT-based context for hit testing
//! operations with compile-time arity validation and protocol-specific geometry.
//!
//! # Type Aliases
//!
//! - [`BoxHitTestContext`] - Hit test context for Box protocol (Size geometry)
//! - [`SliverHitTestContext`] - Hit test context for Sliver protocol (SliverGeometry)

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_types::{Offset, Rect, Size, SliverGeometry};

use crate::arity::{Arity, ChildrenAccess, Single};
use crate::hit_test_tree::HitTestTree;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};

// ============================================================================
// TYPE ALIASES FOR ERGONOMICS
// ============================================================================

/// Box hit test context with dynamic dispatch (convenience alias).
///
/// Equivalent to `HitTestContext<'a, A, BoxProtocol, Box<dyn HitTestTree + Send + Sync>>`.
pub type BoxHitTestContext<'a, A, T = Box<dyn HitTestTree + Send + Sync>> =
    HitTestContext<'a, A, BoxProtocol, T>;

/// Sliver hit test context with dynamic dispatch (convenience alias).
///
/// Equivalent to `HitTestContext<'a, A, SliverProtocol, Box<dyn HitTestTree + Send + Sync>>`.
pub type SliverHitTestContext<'a, A, T = Box<dyn HitTestTree + Send + Sync>> =
    HitTestContext<'a, A, SliverProtocol, T>;

// ============================================================================
// HIT TEST CONTEXT
// ============================================================================

/// GAT-based hit test context for pointer event detection.
///
/// This context provides efficient hit testing with:
/// - Spatial optimizations
/// - Protocol-specific geometry
/// - Type-safe children access
/// - Optional static dispatch
///
/// # Type Parameters
///
/// - `'a`: Lifetime of tree reference and children access
/// - `A`: Arity type constraining the number of children
/// - `P`: Layout protocol (defaults to `BoxProtocol`)
/// - `T`: Tree implementation (defaults to dynamic dispatch)
///
/// # Protocol-Specific Geometry
///
/// The `geometry` field type depends on the protocol:
/// - `BoxProtocol`: `Size` for rectangular hit testing
/// - `SliverProtocol`: `SliverGeometry` for scroll-aware hit testing
///
/// # Examples
///
/// ## Minimal
///
/// ```rust,ignore
/// fn hit_test(&self, ctx: &HitTestContext<'_, Variable>, result: &mut HitTestResult) -> bool {
///     // Test children in reverse z-order
///     for child_id in ctx.children_reverse() {
///         if ctx.hit_test_child(child_id, ctx.position, result) {
///             return true;
///         }
///     }
///     ctx.hit_test_self(result)
/// }
/// ```
pub struct HitTestContext<
    'a,
    A: Arity,
    P: Protocol = BoxProtocol,
    T: HitTestTree = Box<dyn HitTestTree + Send + Sync>,
> where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    tree: &'a T,
    element_id: ElementId,
    /// The position to test (in parent coordinates).
    pub position: Offset,
    /// The computed geometry from layout (protocol-specific).
    pub geometry: P::Geometry,
    /// Children accessor for compile-time arity-checked access.
    pub children: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol, T: HitTestTree> fmt::Debug for HitTestContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HitTestContext")
            .field("element_id", &self.element_id)
            .field("position", &self.position)
            .field("geometry", &self.geometry)
            .field("children_count", &self.children.len())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// HIT TEST CONTEXT - COMMON METHODS
// ============================================================================

impl<'a, A: Arity, P: Protocol, T: HitTestTree> HitTestContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Creates a new hit test context.
    pub fn new(
        tree: &'a T,
        element_id: ElementId,
        position: Offset,
        geometry: P::Geometry,
        children: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            position,
            geometry,
            children,
            _phantom: PhantomData,
        }
    }

    /// Gets the element ID this context is testing.
    #[inline]
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns a GAT-based iterator over child ElementIds.
    #[inline]
    pub fn children(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.children.iter().copied()
    }

    /// Returns children in reverse order (for z-order hit testing).
    pub fn children_reverse(&self) -> impl DoubleEndedIterator<Item = ElementId> + 'a {
        self.children.as_slice().iter().copied().rev()
    }

    /// Hit tests a child element.
    pub fn hit_test_child(
        &self,
        child_id: ElementId,
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool {
        self.tree.hit_test(child_id, position, result)
    }

    /// Creates a new context with a transformed position.
    ///
    /// This is used by render objects that apply transformations (like
    /// `RenderTransform`) to convert the hit test position into child
    /// coordinate space.
    pub fn with_position(&self, new_position: Offset) -> Self
    where
        A::Accessor<'a, ElementId>: Clone,
    {
        Self {
            tree: self.tree,
            element_id: self.element_id,
            position: new_position,
            geometry: self.geometry.clone(),
            children: self.children,
            _phantom: PhantomData,
        }
    }

    /// Hit tests all children in reverse z-order (front to back).
    ///
    /// This is the standard pattern for hit testing children - test from
    /// front to back and return on first hit.
    ///
    /// # Returns
    ///
    /// `true` if any child was hit, `false` otherwise.
    pub fn hit_test_children(&self, result: &mut HitTestResult) -> bool {
        for child_id in self.children_reverse() {
            // Get child offset from tree
            let child_offset = self.tree.get_offset(child_id).unwrap_or(Offset::ZERO);
            let child_position = self.position - child_offset;

            if self.tree.hit_test(child_id, child_position, result) {
                return true;
            }
        }
        false
    }
}

// ============================================================================
// HIT TEST CONTEXT - BOX PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: HitTestTree> HitTestContext<'a, A, BoxProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Returns the size of this element (convenience for Box protocol).
    #[inline]
    pub fn size(&self) -> Size {
        self.geometry
    }

    /// Adds this element to the hit test result.
    pub fn hit_test_self(&self, result: &mut HitTestResult) -> bool {
        let bounds = Rect::from_min_size(Offset::ZERO, self.geometry);
        let entry = HitTestEntry::new(self.element_id, self.position, bounds);
        result.add(entry);
        true
    }

    /// Checks if the position is within this element's bounds.
    pub fn contains_position(&self) -> bool {
        let local_bounds = Rect::from_min_size(Offset::ZERO, self.geometry);
        local_bounds.contains(self.position)
    }

    /// Checks if the position is within a specific rectangle.
    pub fn position_in_rect(&self, rect: Rect) -> bool {
        rect.contains(self.position)
    }
}

// ============================================================================
// HIT TEST CONTEXT - SINGLE CHILD BOX PROTOCOL
// ============================================================================

impl<'a, T: HitTestTree> HitTestContext<'a, Single, BoxProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children.single()
    }

    /// Hit tests the single child at the given position.
    pub fn hit_test_single_child(&self, position: Offset, result: &mut HitTestResult) -> bool {
        let child_id = self.single_child();
        self.hit_test_child(child_id, position, result)
    }
}

// ============================================================================
// HIT TEST CONTEXT - SLIVER PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: HitTestTree> HitTestContext<'a, A, SliverProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Returns the sliver geometry (convenience for Sliver protocol).
    #[inline]
    pub fn sliver_geometry(&self) -> SliverGeometry {
        self.geometry
    }

    /// Adds this element to the hit test result.
    ///
    /// For slivers, uses the paint extent for bounds.
    pub fn hit_test_self(&self, result: &mut HitTestResult) -> bool {
        // For slivers, use paint extent for bounds calculation
        let bounds = Rect::from_ltwh(0.0, 0.0, 0.0, self.geometry.paint_extent);
        let entry = HitTestEntry::new(self.element_id, self.position, bounds);
        result.add(entry);
        true
    }
}

// ============================================================================
// HIT TEST CONTEXT - SINGLE CHILD SLIVER PROTOCOL
// ============================================================================

impl<'a, T: HitTestTree> HitTestContext<'a, Single, SliverProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children.single()
    }

    /// Hit tests the single child at the given position.
    pub fn hit_test_single_child(&self, position: Offset, result: &mut HitTestResult) -> bool {
        let child_id = self.single_child();
        self.hit_test_child(child_id, position, result)
    }
}
