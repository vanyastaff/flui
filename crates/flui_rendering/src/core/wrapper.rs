//! Type-erasure wrappers and utility types for render objects.
//!
//! This module provides wrapper types for working with render objects in
//! type-erased contexts, such as storing them in collections or passing
//! them across API boundaries.
//!
//! # Design Philosophy
//!
//! - **Type erasure**: Store concrete render objects as trait objects
//! - **Arity preservation**: Wrappers maintain arity information
//! - **Protocol preservation**: Box/Sliver protocol is maintained
//! - **Two-level API**: Typed (RenderBox<A>) + dyn-compatible (RenderObject)
//!
//! # Wrapper Types
//!
//! ## BoxRenderWrapper
//!
//! Type-erased wrapper for box protocol render objects:
//! - Stores any `RenderBox<A>` as `Box<dyn RenderBox<A>>`
//! - Preserves arity at compile time
//! - Implements both typed and dyn-compatible APIs
//!
//! ## SliverRenderWrapper
//!
//! Type-erased wrapper for sliver protocol render objects:
//! - Stores any `RenderSliver<A>` as `Box<dyn RenderSliver<A>>`
//! - Preserves arity at compile time
//!
//! # Use Cases
//!
//! - **Collections**: Store heterogeneous render objects
//! - **Dynamic dispatch**: Switch between different implementations
//! - **API boundaries**: Pass render objects without exposing concrete types
//! - **Type-erased rendering**: Use with `Box<dyn RenderObject>`

use std::fmt;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_types::{Rect, Size, SliverGeometry};

use super::arity::Arity;
use super::box_render::RenderBox;
use super::context::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, LayoutContext, SliverHitTestContext,
    SliverLayoutContext, SliverPaintContext,
};
use super::object::RenderObject;
use super::sliver::RenderSliver;
use super::{BoxConstraints, BoxProtocol, LayoutTree};
use crate::RenderResult;

// ============================================================================
// BOX RENDER WRAPPER
// ============================================================================

/// Type-erased wrapper for box protocol render objects.
///
/// This wrapper allows storing any concrete `RenderBox<A>` implementation as a
/// trait object while preserving arity information at compile time.
///
/// # Type Parameters
///
/// - `A`: Arity type (preserved at compile time)
///
/// # Use Cases
///
/// - Store heterogeneous render objects in collections
/// - Pass render objects across API boundaries
/// - Dynamic dispatch based on runtime conditions
/// - Plugin systems with external implementations
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use flui_rendering::core::{BoxRenderWrapper, Single};
///
/// let padding = RenderPadding::new(EdgeInsets::all(10.0));
/// let wrapper: BoxRenderWrapper<Single> = BoxRenderWrapper::new(padding);
///
/// // Use as RenderBox<Single>
/// let size = wrapper.layout(ctx);
/// ```
///
/// ## Collections
///
/// ```rust,ignore
/// use flui_rendering::core::{BoxRenderWrapper, Variable};
///
/// let children: Vec<BoxRenderWrapper<Variable>> = vec![
///     BoxRenderWrapper::new(RenderText::new("Title")),
///     BoxRenderWrapper::new(RenderImage::new("icon.png")),
///     BoxRenderWrapper::new(RenderButton::new()),
/// ];
///
/// // All stored with same type, different implementations
/// for child in &children {
///     child.layout(ctx);
/// }
/// ```
pub struct BoxRenderWrapper<A: Arity> {
    inner: Box<dyn RenderBox<A>>,
}

impl<A: Arity> BoxRenderWrapper<A> {
    /// Creates a new wrapper around a render object.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let padding = RenderPadding::new(EdgeInsets::all(10.0));
    /// let wrapper = BoxRenderWrapper::new(padding);
    /// ```
    pub fn new<R: RenderBox<A> + 'static>(render: R) -> Self {
        Self {
            inner: Box::new(render),
        }
    }

    /// Creates a wrapper from a boxed trait object.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let boxed: Box<dyn RenderBox<Single>> = Box::new(RenderPadding::default());
    /// let wrapper = BoxRenderWrapper::from_box(boxed);
    /// ```
    pub fn from_box(inner: Box<dyn RenderBox<A>>) -> Self {
        Self { inner }
    }

    /// Gets a reference to the inner render object.
    pub fn inner(&self) -> &dyn RenderBox<A> {
        &*self.inner
    }

    /// Gets a mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut dyn RenderBox<A> {
        &mut *self.inner
    }

    /// Attempts to downcast to a specific render object type.
    ///
    /// Returns `Some(&R)` if the inner object is of type `R`, `None` otherwise.
    pub fn downcast_ref<R: RenderBox<A> + 'static>(&self) -> Option<&R> {
        (self.inner.as_ref() as &dyn RenderObject)
            .as_any()
            .downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    ///
    /// Returns `Some(&mut R)` if the inner object is of type `R`, `None` otherwise.
    pub fn downcast_mut<R: RenderBox<A> + 'static>(&mut self) -> Option<&mut R> {
        (self.inner.as_mut() as &mut dyn RenderObject)
            .as_any_mut()
            .downcast_mut::<R>()
    }

    /// Unwraps the wrapper, returning the inner boxed trait object.
    pub fn into_inner(self) -> Box<dyn RenderBox<A>> {
        self.inner
    }
}

impl<A: Arity> fmt::Debug for BoxRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxRenderWrapper")
            .field("inner", &self.inner.as_ref().debug_name())
            .finish()
    }
}

// ============================================================================
// TYPED API (RenderBox<A>)
// ============================================================================

// Implement RenderBox by delegating to inner
impl<A: Arity> RenderBox<A> for BoxRenderWrapper<A> {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        RenderBox::paint(&*self.inner, ctx)
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        RenderBox::hit_test(&*self.inner, ctx, result)
    }

    fn intrinsic_width(&self, height: f32) -> Option<f32> {
        self.inner.intrinsic_width(height)
    }

    fn intrinsic_height(&self, width: f32) -> Option<f32> {
        self.inner.intrinsic_height(width)
    }

    fn baseline_offset(&self) -> Option<f32> {
        RenderBox::baseline_offset(self.inner.as_ref())
    }

    fn local_bounds(&self) -> Rect {
        RenderBox::local_bounds(self.inner.as_ref())
    }
}

// ============================================================================
// DYN-COMPATIBLE API (RenderObject)
// ============================================================================

impl<A: Arity> RenderObject for BoxRenderWrapper<A> {
    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }

    /// Performs layout using box protocol.
    ///
    /// This overrides the default implementation to call the typed
    /// `RenderBox::layout()` method with proper context.
    fn perform_layout(
        &mut self,
        element_id: ElementId,
        constraints: BoxConstraints,
        tree: &mut dyn LayoutTree,
    ) -> RenderResult<Size> {
        // Get children from tree
        // We need to use render_object to access the tree navigation
        let children_vec: Vec<ElementId> = if let Some(_render_obj) = tree.render_object(element_id) {
            // Try to get children - for now, return empty vec if not available
            // TODO: Add a proper method to LayoutTree for getting children
            Vec::new()
        } else {
            Vec::new()
        };

        // Create children accessor using Arity::from_slice
        let children_accessor = A::from_slice(&children_vec);

        // Create tree adapter that implements Send + Sync
        // This is safe because LayoutTree operations don't require thread-safety at runtime
        let mut tree_adapter = TreeAdapter(tree);

        //  Create layout context for RenderBox::layout()
        // Use TreeAdapter directly as T instead of boxing
        let ctx = LayoutContext::<'_, A, BoxProtocol, TreeAdapter<'_>>::new(
            &mut tree_adapter,
            element_id,
            constraints,
            children_accessor,
        );

        // Call typed layout method
        self.inner.layout(ctx)
    }
}

/// Adapter to make &mut dyn LayoutTree work with BoxLayoutContext
///
/// This wrapper provides Send + Sync bounds required by BoxLayoutContext
/// while delegating all operations to the underlying LayoutTree.
struct TreeAdapter<'a>(&'a mut dyn LayoutTree);

// SAFETY: LayoutTree operations are single-threaded at runtime
// These markers are needed for type system compatibility
unsafe impl Send for TreeAdapter<'_> {}
unsafe impl Sync for TreeAdapter<'_> {}

impl LayoutTree for TreeAdapter<'_> {
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<flui_types::Size, crate::RenderError> {
        self.0.perform_layout(id, constraints)
    }

    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: crate::SliverConstraints,
    ) -> Result<crate::SliverGeometry, crate::RenderError> {
        self.0.perform_sliver_layout(id, constraints)
    }

    fn set_offset(&mut self, id: ElementId, offset: flui_types::Offset) {
        self.0.set_offset(id, offset)
    }

    fn get_offset(&self, id: ElementId) -> Option<flui_types::Offset> {
        self.0.get_offset(id)
    }

    fn mark_needs_layout(&mut self, id: ElementId) {
        self.0.mark_needs_layout(id)
    }

    fn needs_layout(&self, id: ElementId) -> bool {
        self.0.needs_layout(id)
    }

    fn render_object(&self, id: ElementId) -> Option<&dyn std::any::Any> {
        self.0.render_object(id)
    }

    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn std::any::Any> {
        self.0.render_object_mut(id)
    }

    fn setup_child_parent_data(&mut self, parent_id: ElementId, child_id: ElementId) {
        self.0.setup_child_parent_data(parent_id, child_id)
    }
}

// ============================================================================
// SLIVER RENDER WRAPPER
// ============================================================================

/// Type-erased wrapper for sliver protocol render objects.
///
/// This wrapper allows storing any concrete `RenderSliver<A>` implementation as a
/// trait object while preserving arity information at compile time.
///
/// # Type Parameters
///
/// - `A`: Arity type (preserved at compile time)
pub struct SliverRenderWrapper<A: Arity> {
    inner: Box<dyn RenderSliver<A>>,
}

impl<A: Arity> SliverRenderWrapper<A> {
    /// Creates a new wrapper around a sliver render object.
    pub fn new<R: RenderSliver<A> + 'static>(render: R) -> Self {
        Self {
            inner: Box::new(render),
        }
    }

    /// Creates a wrapper from a boxed trait object.
    pub fn from_box(inner: Box<dyn RenderSliver<A>>) -> Self {
        Self { inner }
    }

    /// Gets a reference to the inner render object.
    pub fn inner(&self) -> &dyn RenderSliver<A> {
        &*self.inner
    }

    /// Gets a mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut dyn RenderSliver<A> {
        &mut *self.inner
    }

    /// Attempts to downcast to a specific render object type.
    pub fn downcast_ref<R: RenderSliver<A> + 'static>(&self) -> Option<&R> {
        (self.inner.as_ref() as &dyn RenderObject)
            .as_any()
            .downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    pub fn downcast_mut<R: RenderSliver<A> + 'static>(&mut self) -> Option<&mut R> {
        (self.inner.as_mut() as &mut dyn RenderObject)
            .as_any_mut()
            .downcast_mut::<R>()
    }

    /// Unwraps the wrapper, returning the inner boxed trait object.
    pub fn into_inner(self) -> Box<dyn RenderSliver<A>> {
        self.inner
    }
}

impl<A: Arity> fmt::Debug for SliverRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverRenderWrapper")
            .field("inner", &self.inner.as_ref().debug_name())
            .finish()
    }
}

// ============================================================================
// TYPED API (RenderSliver<A>)
// ============================================================================

impl<A: Arity> RenderSliver<A> for SliverRenderWrapper<A> {
    fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> RenderResult<SliverGeometry> {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, A>) {
        RenderSliver::paint(&*self.inner, ctx)
    }

    fn hit_test(&self, ctx: &SliverHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        RenderSliver::hit_test(&*self.inner, ctx, result)
    }
}

// ============================================================================
// DYN-COMPATIBLE API (RenderObject) for Sliver
// ============================================================================

impl<A: Arity> RenderObject for SliverRenderWrapper<A> {
    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }

    // Sliver protocol uses different constraints (SliverConstraints, not BoxConstraints)
    // So perform_layout returns an error for box constraints
    fn perform_layout(
        &mut self,
        _element_id: ElementId,
        _constraints: BoxConstraints,
        _tree: &mut dyn LayoutTree,
    ) -> RenderResult<Size> {
        Err(crate::RenderError::UnsupportedProtocol {
            expected: "SliverProtocol",
            found: "BoxProtocol (use perform_sliver_layout instead)",
        })
    }

    // TODO: Add perform_sliver_layout when we extend RenderObject trait
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Leaf;

    #[derive(Debug)]
    struct MockRenderBox {
        value: i32,
    }

    impl RenderObject for MockRenderBox {}

    impl RenderBox<Leaf> for MockRenderBox {
        fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
            Ok(ctx.constraints.smallest())
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<'_, Leaf>) {}
    }

    #[test]
    fn test_wrapper_creation() {
        let mock = MockRenderBox { value: 42 };
        let wrapper = BoxRenderWrapper::new(mock);

        assert_eq!(
            wrapper.inner().debug_name(),
            "flui_rendering::core::wrapper::tests::MockRenderBox"
        );
    }

    #[test]
    fn test_wrapper_downcast() {
        let mock = MockRenderBox { value: 42 };
        let mut wrapper = BoxRenderWrapper::new(mock);

        // Downcast to correct type
        let downcast = wrapper.downcast_ref::<MockRenderBox>();
        assert!(downcast.is_some());
        assert_eq!(downcast.unwrap().value, 42);

        // Mutable downcast
        let downcast_mut = wrapper.downcast_mut::<MockRenderBox>();
        assert!(downcast_mut.is_some());
        downcast_mut.unwrap().value = 100;

        assert_eq!(wrapper.downcast_ref::<MockRenderBox>().unwrap().value, 100);
    }
}
