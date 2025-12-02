//! Wrapper and proxy types for ergonomic render object composition.
//!
//! This module provides utility wrappers and proxy types that make it easier to
//! compose render objects, create decorators, and implement common patterns.
//! These wrappers leverage the unified arity system and provide zero-cost
//! abstractions for common rendering scenarios.
//!
//! # Design Philosophy
//!
//! - **Composition over inheritance**: Wrappers enable flexible object composition
//! - **Zero-cost abstractions**: All wrappers compile to direct method calls
//! - **Type safety**: Arity validation ensures correct child management
//! - **Ergonomics**: Convenient APIs for common rendering patterns
//! - **Performance**: Optimized for hot rendering paths
//!
//! # Wrapper Categories
//!
//! ## Proxy Wrappers
//!
//! Proxy wrappers delegate most operations to a single child:
//! - [`RenderProxy`] - Generic single-child proxy
//! - [`SingleChildProxy`] - Specialized proxy with transformation hooks
//!
//! ## Utility Wrappers
//!
//! Utility wrappers provide common functionality:
//! - [`BoxRenderWrapper`] - Wraps any RenderBox for type erasure
//! - [`SliverRenderWrapper`] - Wraps any SliverRender for type erasure
//! - [`RenderWrapper`] - Generic wrapper with custom behavior
//!
//! ## Trait Implementations
//!
//! Helper traits for implementing wrappers:
//! - [`ProxyRender`] - Trait for proxy render objects
//! - [`WrapperRender`] - Trait for wrapper render objects
//!
//! # Usage Examples
//!
//! ## Simple Proxy
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderProxy, RenderBox, LayoutContext, Single};
//!
//! #[derive(Debug)]
//! struct RenderTransform {
//!     transform: Transform,
//!     proxy: RenderProxy<Single>,
//! }
//!
//! impl RenderBox<Single> for RenderTransform {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> RenderResult<Size> {
//!         // Delegate to proxy, which handles child layout
//!         self.proxy.layout(ctx)
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Single, BoxProtocol>) {
//!         ctx.with_transform(self.transform, |ctx| {
//!             self.proxy.paint(ctx)
//!         });
//!     }
//!
//!     fn as_render_object(&self) -> &dyn RenderObject {
//!         self
//!     }
//! }
//! ```
//!
//! ## Custom Wrapper
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderWrapper, WrapperRender};
//!
//! #[derive(Debug)]
//! struct RenderDecorator<R> {
//!     decoration: BoxDecoration,
//!     wrapped: R,
//! }
//!
//! impl<R: RenderBox<Single>> WrapperRender<Single> for RenderDecorator<R> {
//!     type Wrapped = R;
//!
//!     fn wrapped(&self) -> &Self::Wrapped {
//!         &self.wrapped
//!     }
//!
//!     fn wrapped_mut(&mut self) -> &mut Self::Wrapped {
//!         &mut self.wrapped
//!     }
//!
//!     fn pre_paint(&self, ctx: &mut PaintContext<'_, Single, BoxProtocol>) {
//!         // Draw decoration before child
//!         self.decoration.paint(ctx.canvas_mut(), ctx.local_bounds());
//!     }
//! }
//! ```
//!
//! ## Type Erasure Wrapper
//!
//! ```rust,ignore
//! use flui_rendering::core::{BoxRenderWrapper, RenderBox, Leaf};
//!
//! // Wrap a concrete render object for storage in collections
//! let render_text = RenderText::new("Hello, World!");
//! let wrapper: BoxRenderWrapper<Leaf> = BoxRenderWrapper::new(render_text);
//!
//! // Use as dyn RenderBox
//! let boxed: Box<dyn RenderBox<Leaf>> = Box::new(wrapper);
//! ```

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_types::{Offset, Size};

use super::arity::{Arity, Single};
use super::contexts::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use super::geometry::BoxConstraints;
use super::render_box::{RenderBox, RenderBoxExt};
use super::render_object::RenderObject;
use super::render_sliver::SliverRender;
use crate::core::RenderResult;

// ============================================================================
// PROXY TRAITS
// ============================================================================

/// Trait for render objects that proxy operations to a child.
///
/// Proxy render objects delegate most or all of their operations to a single
/// child while potentially applying transformations or decorations.
pub trait ProxyRender<A: Arity>: RenderBox<A> {
    /// Gets the child element ID that operations are proxied to.
    ///
    /// This should return the ElementId of the child that receives
    /// layout, paint, and hit test operations.
    fn proxy_child(&self) -> Option<ElementId>;

    /// Applies any transformations needed before proxying layout.
    ///
    /// The default implementation passes constraints through unchanged.
    fn transform_constraints(
        &self,
        ctx: &BoxLayoutContext<'_, A>,
    ) -> super::geometry::BoxConstraints {
        ctx.constraints
    }

    /// Applies any transformations needed after child layout.
    ///
    /// The default implementation passes the child size through unchanged.
    fn transform_size(&self, child_size: Size) -> Size {
        child_size
    }

    /// Applies any transformations needed before proxying paint.
    ///
    /// The default implementation passes the offset through unchanged.
    fn transform_offset(&self, offset: Offset) -> Offset {
        offset
    }

    /// Called before painting the child.
    ///
    /// This can be used to set up clipping, transformations, or draw backgrounds.
    fn pre_paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
        // Default: do nothing
    }

    /// Called after painting the child.
    ///
    /// This can be used to draw overlays, borders, or other decorations.
    fn post_paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
        // Default: do nothing
    }
}

/// Trait for render objects that wrap another render object.
///
/// Wrapper render objects contain another render object and can modify
/// or extend its behavior while maintaining the same arity.
pub trait WrapperRender<A: Arity>: RenderBox<A> {
    /// The type of render object being wrapped.
    type Wrapped: RenderBox<A>;

    /// Gets a reference to the wrapped render object.
    fn wrapped(&self) -> &Self::Wrapped;

    /// Gets a mutable reference to the wrapped render object.
    fn wrapped_mut(&mut self) -> &mut Self::Wrapped;

    /// Called before delegating layout to the wrapped object.
    ///
    /// This can modify the context or constraints before delegation.
    fn pre_layout(&mut self, _ctx: &BoxLayoutContext<'_, A>) {
        // Default: do nothing
    }

    /// Called after the wrapped object completes layout.
    ///
    /// This can modify the returned size or perform additional work.
    fn post_layout(&mut self, _ctx: &BoxLayoutContext<'_, A>, _size: Size) -> Size {
        // Default: return size unchanged
        _size
    }

    /// Called before delegating paint to the wrapped object.
    fn pre_paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
        // Default: do nothing
    }

    /// Called after the wrapped object completes paint.
    fn post_paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
        // Default: do nothing
    }

    /// Called before delegating hit testing to the wrapped object.
    ///
    /// Return `true` to skip delegation and handle hit testing manually.
    fn pre_hit_test(&self, _ctx: &BoxHitTestContext<'_, A>, _result: &mut HitTestResult) -> bool {
        // Default: don't skip delegation
        false
    }

    /// Called after the wrapped object completes hit testing.
    ///
    /// This can modify the result or perform additional hit testing.
    fn post_hit_test(
        &self,
        _ctx: &BoxHitTestContext<'_, A>,
        _result: &mut HitTestResult,
        hit: bool,
    ) -> bool {
        // Default: return hit result unchanged
        hit
    }
}

// ============================================================================
// GENERIC PROXY
// ============================================================================

/// Generic proxy render object that delegates operations to a child.
///
/// This is a utility type for implementing render objects that simply
/// pass through operations to a single child, potentially with transformations.
#[derive(Debug)]
pub struct RenderProxy<A: Arity> {
    child_id: Option<ElementId>,
    _phantom: PhantomData<A>,
}

impl<A: Arity> RenderProxy<A> {
    /// Creates a new proxy with no child.
    pub fn new() -> Self {
        Self {
            child_id: None,
            _phantom: PhantomData,
        }
    }

    /// Creates a new proxy with the specified child.
    pub fn with_child(child_id: ElementId) -> Self {
        Self {
            child_id: Some(child_id),
            _phantom: PhantomData,
        }
    }

    /// Sets the child element ID.
    pub fn set_child(&mut self, child_id: ElementId) {
        self.child_id = Some(child_id);
    }

    /// Gets the child element ID.
    pub fn child(&self) -> Option<ElementId> {
        self.child_id
    }

    /// Clears the child element ID.
    pub fn clear_child(&mut self) {
        self.child_id = None;
    }
}

impl<A: Arity> Default for RenderProxy<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Arity> RenderBox<A> for RenderProxy<A> {
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
        if let Some(child_id) = self.child_id {
            ctx.layout_child(child_id, ctx.constraints)
        } else {
            Ok(ctx.constraints.smallest())
        }
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        if let Some(child_id) = self.child_id {
            let _ = ctx.paint_child(child_id, Offset::ZERO);
        }
    }

    fn as_render_object(&self) -> &dyn RenderObject {
        self
    }
}

impl<A: Arity> RenderObject for RenderProxy<A> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        "RenderProxy"
    }
}

impl<A: Arity> ProxyRender<A> for RenderProxy<A> {
    fn proxy_child(&self) -> Option<ElementId> {
        self.child_id
    }
}

// ============================================================================
// SINGLE CHILD PROXY
// ============================================================================

/// Specialized proxy for single-child render objects with transformation hooks.
///
/// This provides a more ergonomic API for the common case of single-child
/// proxies that need to apply transformations to constraints, sizes, or offsets.
#[derive(Debug)]
pub struct SingleChildProxy<F>
where
    F: Fn(&super::geometry::BoxConstraints) -> super::geometry::BoxConstraints + Send + Sync,
{
    child_id: Option<ElementId>,
    constraint_transform: F,
}

impl<F> SingleChildProxy<F>
where
    F: Fn(&super::geometry::BoxConstraints) -> super::geometry::BoxConstraints + Send + Sync,
{
    /// Creates a new single child proxy with a constraint transformation function.
    pub fn new(constraint_transform: F) -> Self {
        Self {
            child_id: None,
            constraint_transform,
        }
    }

    /// Creates a new single child proxy with a child and constraint transformation.
    pub fn with_child(child_id: ElementId, constraint_transform: F) -> Self {
        Self {
            child_id: Some(child_id),
            constraint_transform,
        }
    }

    /// Sets the child element ID.
    pub fn set_child(&mut self, child_id: ElementId) {
        self.child_id = Some(child_id);
    }

    /// Gets the child element ID.
    pub fn child(&self) -> Option<ElementId> {
        self.child_id
    }
}

impl<F> RenderBox<Single> for SingleChildProxy<F>
where
    F: Fn(&super::geometry::BoxConstraints) -> super::geometry::BoxConstraints
        + Send
        + Sync
        + 'static,
{
    fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        if let Some(child_id) = self.child_id {
            let transformed_constraints = (self.constraint_transform)(&ctx.constraints);
            ctx.layout_child(child_id, transformed_constraints)
        } else {
            Ok(ctx.constraints.smallest())
        }
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        if let Some(child_id) = self.child_id {
            let _ = ctx.paint_child(child_id, Offset::ZERO);
        }
    }

    fn as_render_object(&self) -> &dyn RenderObject {
        self
    }
}

impl<F> RenderObject for SingleChildProxy<F>
where
    F: Fn(&super::geometry::BoxConstraints) -> super::geometry::BoxConstraints
        + Send
        + Sync
        + 'static,
{
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        "SingleChildProxy"
    }
}

// ============================================================================
// TYPE ERASURE WRAPPERS
// ============================================================================

/// Type-erasing wrapper for box protocol render objects.
///
/// This wrapper allows storing different concrete render object types
/// in collections while maintaining type safety for the arity parameter.
pub struct BoxRenderWrapper<A: Arity> {
    inner: Box<dyn RenderBox<A>>,
}

impl<A: Arity> BoxRenderWrapper<A> {
    /// Creates a new wrapper around the given render object.
    pub fn new<R: RenderBox<A> + 'static>(render_object: R) -> Self {
        Self {
            inner: Box::new(render_object),
        }
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
    pub fn downcast_ref<R: RenderBox<A> + 'static>(&self) -> Option<&R> {
        self.inner.as_render_object().as_any().downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    pub fn downcast_mut<R: RenderBox<A> + 'static>(&mut self) -> Option<&mut R> {
        self.inner.as_render_object().as_any().downcast_ref::<R>()
    }
}

impl<A: Arity> fmt::Debug for BoxRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxRenderWrapper")
            .field("inner", &self.inner.debug_name())
            .finish()
    }
}

impl<A: Arity> RenderBox<A> for BoxRenderWrapper<A> {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        self.inner.paint(ctx)
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        self.inner.hit_test(ctx, result)
    }

    fn as_render_object(&self) -> &dyn RenderObject {
        self.inner.as_render_object()
    }
}

/// Type-erasing wrapper for sliver protocol render objects.
///
/// Similar to `BoxRenderWrapper` but for sliver protocol render objects.
pub struct SliverRenderWrapper<A: Arity> {
    inner: Box<dyn SliverRender<A>>,
}

impl<A: Arity> SliverRenderWrapper<A> {
    /// Creates a new wrapper around the given sliver render object.
    pub fn new<R: SliverRender<A> + 'static>(render_object: R) -> Self {
        Self {
            inner: Box::new(render_object),
        }
    }

    /// Gets a reference to the inner render object.
    pub fn inner(&self) -> &dyn SliverRender<A> {
        &*self.inner
    }

    /// Gets a mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut dyn SliverRender<A> {
        &mut *self.inner
    }

    /// Attempts to downcast to a specific render object type.
    pub fn downcast_ref<R: SliverRender<A> + 'static>(&self) -> Option<&R> {
        self.inner.as_render_object().as_any().downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    pub fn downcast_mut<R: SliverRender<A> + 'static>(&mut self) -> Option<&mut R> {
        self.inner.as_render_object().as_any().downcast_ref::<R>()
    }
}

impl<A: Arity> fmt::Debug for SliverRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverRenderWrapper")
            .field("inner", &self.inner.debug_name())
            .finish()
    }
}

impl<A: Arity> SliverRender<A> for SliverRenderWrapper<A> {
    fn layout(
        &mut self,
        ctx: super::contexts::SliverLayoutContext<'_, A>,
    ) -> flui_types::SliverGeometry {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut super::contexts::SliverPaintContext<'_, A>) {
        self.inner.paint(ctx)
    }
}

// ============================================================================
// GENERIC WRAPPER
// ============================================================================

/// Generic wrapper that implements the `WrapperRender` trait.
///
/// This provides a convenient way to create wrapper render objects
/// without implementing the full `RenderBox` trait manually.
pub struct RenderWrapper<A: Arity, R: RenderBox<A>> {
    wrapped: R,
    pre_layout_fn: Option<Box<dyn Fn(&mut R, &BoxLayoutContext<'_, A>) + Send + Sync>>,
    post_layout_fn:
        Option<Box<dyn Fn(&mut R, &BoxLayoutContext<'_, A>, Size) -> Size + Send + Sync>>,
    pre_paint_fn: Option<Box<dyn Fn(&R, &mut BoxPaintContext<'_, A>) + Send + Sync>>,
    post_paint_fn: Option<Box<dyn Fn(&R, &mut BoxPaintContext<'_, A>) + Send + Sync>>,
}

impl<A: Arity, R: RenderBox<A>> RenderWrapper<A, R> {
    /// Creates a new wrapper around the given render object.
    pub fn new(wrapped: R) -> Self {
        Self {
            wrapped,
            pre_layout_fn: None,
            post_layout_fn: None,
            pre_paint_fn: None,
            post_paint_fn: None,
        }
    }

    /// Sets a function to be called before layout.
    pub fn with_pre_layout<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut R, &BoxLayoutContext<'_, A>) + Send + Sync + 'static,
    {
        self.pre_layout_fn = Some(Box::new(f));
        self
    }

    /// Sets a function to be called after layout.
    pub fn with_post_layout<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut R, &BoxLayoutContext<'_, A>, Size) -> Size + Send + Sync + 'static,
    {
        self.post_layout_fn = Some(Box::new(f));
        self
    }

    /// Sets a function to be called before paint.
    pub fn with_pre_paint<F>(mut self, f: F) -> Self
    where
        F: Fn(&R, &mut BoxPaintContext<'_, A>) + Send + Sync + 'static,
    {
        self.pre_paint_fn = Some(Box::new(f));
        self
    }

    /// Sets a function to be called after paint.
    pub fn with_post_paint<F>(mut self, f: F) -> Self
    where
        F: Fn(&R, &mut BoxPaintContext<'_, A>) + Send + Sync + 'static,
    {
        self.post_paint_fn = Some(Box::new(f));
        self
    }
}

impl<A: Arity, R: RenderBox<A>> fmt::Debug for RenderWrapper<A, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderWrapper")
            .field("wrapped", &self.wrapped.debug_name())
            .finish()
    }
}

impl<A: Arity, R: RenderBox<A>> RenderBox<A> for RenderWrapper<A, R> {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
        if let Some(ref pre_layout) = self.pre_layout_fn {
            pre_layout(&mut self.wrapped, &ctx);
        }

        let size = self.wrapped.layout(ctx)?;

        if let Some(ref post_layout) = self.post_layout_fn {
            Ok(post_layout(&mut self.wrapped, &ctx, size))
        } else {
            Ok(size)
        }
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        if let Some(ref pre_paint) = self.pre_paint_fn {
            pre_paint(&self.wrapped, ctx);
        }

        self.wrapped.paint(ctx);

        if let Some(ref post_paint) = self.post_paint_fn {
            post_paint(&self.wrapped, ctx);
        }
    }

    fn as_render_object(&self) -> &dyn RenderObject {
        self.wrapped.as_render_object()
    }
}

impl<A: Arity, R: RenderBox<A>> WrapperRender<A> for RenderWrapper<A, R> {
    type Wrapped = R;

    fn wrapped(&self) -> &Self::Wrapped {
        &self.wrapped
    }

    fn wrapped_mut(&mut self) -> &mut Self::Wrapped {
        &mut self.wrapped
    }

    fn pre_layout(&mut self, ctx: &BoxLayoutContext<'_, A>) {
        if let Some(ref pre_layout) = self.pre_layout_fn {
            pre_layout(&mut self.wrapped, ctx);
        }
    }

    fn post_layout(&mut self, ctx: &BoxLayoutContext<'_, A>, size: Size) -> Size {
        if let Some(ref post_layout) = self.post_layout_fn {
            post_layout(&mut self.wrapped, ctx, size)
        } else {
            size
        }
    }

    fn pre_paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        if let Some(ref pre_paint) = self.pre_paint_fn {
            pre_paint(&self.wrapped, ctx);
        }
    }

    fn post_paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        if let Some(ref post_paint) = self.post_paint_fn {
            post_paint(&self.wrapped, ctx);
        }
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Creates a simple proxy that passes constraints through unchanged.
pub fn create_simple_proxy<A: Arity>() -> RenderProxy<A> {
    RenderProxy::new()
}

/// Creates a proxy with constraint transformation.
pub fn create_constraint_proxy<F>(transform: F) -> SingleChildProxy<F>
where
    F: Fn(&super::geometry::BoxConstraints) -> super::geometry::BoxConstraints + Send + Sync,
{
    SingleChildProxy::new(transform)
}

/// Creates a wrapper with custom behavior.
pub fn create_wrapper<A: Arity, R: RenderBox<A>>(render_object: R) -> RenderWrapper<A, R> {
    RenderWrapper::new(render_object)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single};
    use crate::core::render_box::EmptyRenderBox;

    #[test]
    fn test_render_proxy_creation() {
        let mut proxy = RenderProxy::<Single>::new();
        assert!(proxy.child().is_none());

        let child_id = ElementId::new(42);
        proxy.set_child(child_id);
        assert_eq!(proxy.child(), Some(child_id));

        proxy.clear_child();
        assert!(proxy.child().is_none());
    }

    #[test]
    fn test_proxy_with_child() {
        let child_id = ElementId::new(42);
        let proxy = RenderProxy::<Single>::with_child(child_id);
        assert_eq!(proxy.child(), Some(child_id));
    }

    #[test]
    fn test_single_child_proxy() {
        let transform =
            |constraints: &BoxConstraints| constraints.deflate(&flui_types::EdgeInsets::all(10.0));

        let mut proxy = SingleChildProxy::new(transform);
        assert!(proxy.child().is_none());

        let child_id = ElementId::new(42);
        proxy.set_child(child_id);
        assert_eq!(proxy.child(), Some(child_id));
    }

    #[test]
    fn test_box_render_wrapper() {
        let empty = EmptyRenderBox::<Leaf>::default();
        let wrapper = BoxRenderWrapper::new(empty);

        assert_eq!(wrapper.inner().debug_name(), "EmptyRenderBox");
    }

    #[test]
    fn test_render_wrapper_builder() {
        let empty = EmptyRenderBox::<Single>::default();
        let wrapper = RenderWrapper::new(empty)
            .with_pre_layout(|_render, _ctx| {
                // Pre-layout hook
            })
            .with_post_layout(|_render, _ctx, size| {
                // Post-layout hook - return modified size
                size
            });

        assert_eq!(wrapper.wrapped().debug_name(), "EmptyRenderBox");
    }

    #[test]
    fn test_utility_functions() {
        let proxy = create_simple_proxy::<Single>();
        assert!(proxy.child().is_none());

        let constraint_proxy = create_constraint_proxy(|constraints| *constraints);
        assert!(constraint_proxy.child().is_none());

        let empty = EmptyRenderBox::<Leaf>::default();
        let wrapper = create_wrapper(empty);
        assert_eq!(wrapper.wrapped().debug_name(), "EmptyRenderBox");
    }

    #[test]
    fn test_wrapper_render_trait() {
        let empty = EmptyRenderBox::<Single>::default();
        let wrapper = RenderWrapper::new(empty);

        // Test WrapperRender trait methods
        assert_eq!(wrapper.wrapped().debug_name(), "EmptyRenderBox");
    }
}
