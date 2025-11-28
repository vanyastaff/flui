//! Box protocol render trait.
//!
//! This module provides the `RenderBox<T, A>` trait for implementing render objects
//! that use the standard 2D box layout protocol.
//!
//! # Architecture
//!
//! ```text
//! RenderBox<T, A> trait
//! ├── layout() → Size
//! ├── paint()
//! └── hit_test() → bool
//! ```
//!
//! # Design
//!
//! `RenderBox` is generic over:
//! - `T`: Tree type implementing `FullRenderTree` (LayoutTree + PaintTree + HitTestTree)
//! - `A`: Arity - compile-time child count (Leaf, Single, Optional, Variable)
//!
//! Having `T` at trait level (not method level) provides:
//! - dyn-compatibility for concrete tree types
//! - Better IDE support and error messages
//! - Consistent tree type across all methods
//!
//! The trait uses context-based API where `LayoutContext` and `PaintContext`
//! provide access to children and tree operations.

use flui_element::Element;
use flui_interaction::HitTestResult;
use flui_types::{Offset, Size};
use std::fmt::Debug;

use super::arity::{Arity, Leaf, Optional, Single, Variable};
use super::contexts::{HitTestContext, LayoutContext, PaintContext};
use super::protocol::BoxProtocol;

// ============================================================================
// RENDER BOX TRAIT
// ============================================================================

/// Box protocol render trait.
///
/// Implement this trait for render objects that use the standard 2D box layout.
///
/// # Type Parameters
///
/// - `T`: Tree type implementing `FullRenderTree`
/// - `A`: Arity - compile-time child count (Leaf, Single, Optional, Variable)
///
/// # dyn-compatibility
///
/// By having `T` at trait level (not method level), this trait is dyn-compatible
/// for a concrete tree type. This enables `Box<dyn RenderBox<MyTree, Leaf>>`.
///
/// # Example
///
/// ```rust,ignore
/// impl<T: FullRenderTree> RenderBox<T, Leaf> for RenderColoredBox {
///     fn layout(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size {
///         ctx.constraints.constrain(Size::new(100.0, 100.0))
///     }
///
///     fn paint(&self, ctx: &mut PaintContext<'_, T, Leaf>) {
///         ctx.canvas().rect(Rect::from_size(self.size), &self.paint);
///     }
/// }
/// ```
pub trait RenderBox<T: super::render_tree::FullRenderTree, A: Arity>:
    Send + Sync + Debug + 'static
{
    /// Computes the size of this render object given constraints.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Layout context with constraints, children access, and tree
    ///
    /// # Returns
    ///
    /// The computed size that satisfies the constraints.
    fn layout(&mut self, ctx: LayoutContext<'_, T, A, BoxProtocol>) -> Size;

    /// Paints this render object to a canvas.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Paint context with offset, children access, canvas, and tree
    fn paint(&self, ctx: &mut PaintContext<'_, T, A>);

    /// Performs hit testing for pointer events.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Hit test context with position, geometry, children access
    /// * `result` - Hit test result to add entries to
    ///
    /// # Returns
    ///
    /// `true` if this element or any child was hit.
    fn hit_test(
        &self,
        ctx: &HitTestContext<'_, T, A, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool {
        // Default: test children first, then self
        let hit_children = self.hit_test_children(ctx, result);
        if hit_children || self.hit_test_self(ctx.position, ctx.size()) {
            ctx.add_to_result(result);
            return true;
        }
        false
    }

    /// Tests if the position hits this render object (excluding children).
    ///
    /// Override for opaque hit testing (e.g., buttons, interactive areas).
    /// Default returns `false` (transparent to hit testing).
    fn hit_test_self(&self, _position: Offset, _size: Size) -> bool {
        false
    }

    /// Tests if the position hits any children.
    ///
    /// Default iterates children and tests each. Override for custom
    /// traversal (e.g., z-order, clipping regions).
    fn hit_test_children(
        &self,
        _ctx: &HitTestContext<'_, T, A, BoxProtocol>,
        _result: &mut HitTestResult,
    ) -> bool {
        // Default: no children hit (override for non-leaf)
        false
    }

    /// Returns a debug name for this render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcasts to concrete type for inspection.
    ///
    /// Default implementation provides automatic downcasting for all types.
    fn as_any(&self) -> &dyn std::any::Any
    where
        Self: Sized,
    {
        self
    }

    /// Downcasts to mutable concrete type for mutation.
    ///
    /// Default implementation provides automatic downcasting for all types.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any
    where
        Self: Sized,
    {
        self
    }
}

// ============================================================================
// BUILDER WRAPPERS FOR RENDERBOX -> ELEMENT
// ============================================================================

// Import Child and Children from flui-view
// Note: These are re-exported through flui_core::view::children

/// Wrapper for RenderBox with no children (Leaf arity).
///
/// Returned by `RenderBoxExt::leaf()` for render objects with no children.
pub struct RenderBoxLeaf<R> {
    /// The render object
    pub(crate) render: R,
}

/// Wrapper for RenderBox with single child (Single arity).
///
/// Returned by `RenderBoxExt::with_child()` for render objects with exactly one child.
///
/// # Note
///
/// The child is stored as `flui_view::Child` which will be extracted during
/// element inflation by the BuildPipeline.
pub struct RenderBoxWithChild<R> {
    /// The render object
    pub(crate) render: R,
    /// The single child (from flui-view)
    pub(crate) child: flui_element::Element,
}

/// Wrapper for RenderBox with optional child (Optional arity).
///
/// Returned by `RenderBoxExt::maybe_child()` for render objects with 0 or 1 child.
///
/// # Note
///
/// The child is stored as `flui_view::Child` (which wraps `Option<Element>`).
pub struct RenderBoxWithOptionalChild<R> {
    /// The render object
    pub(crate) render: R,
    /// Optional child (from flui-view)
    pub(crate) child: Option<flui_element::Element>,
}

/// Wrapper for RenderBox with multiple children (Variable arity).
///
/// Returned by `RenderBoxExt::with_children()` for render objects with N children.
///
/// # Note
///
/// The children are stored as `flui_view::Children` which will be extracted
/// during element inflation by the BuildPipeline.
pub struct RenderBoxWithChildren<R> {
    /// The render object
    pub(crate) render: R,
    /// Child elements (from flui-view)
    pub(crate) children: Vec<flui_element::Element>,
}

// ============================================================================
// IntoElement IMPLEMENTATIONS
// ============================================================================

use crate::core::RuntimeArity;
use crate::view::RenderObjectWrapper;
use flui_element::IntoElement;
use flui_foundation::ViewMode;

// Note: IntoElement implementations don't require RenderBox<T, A> bound because:
// 1. RenderObjectWrapper only requires Send + Sync + Debug + 'static on struct
// 2. The RenderBox<T, A> bound is checked later when RenderViewObject<T> is used
// 3. This allows creating Elements without knowing the concrete tree type T

impl<R> IntoElement for RenderBoxLeaf<R>
where
    R: Send + Sync + std::fmt::Debug + 'static,
{
    fn into_element(self) -> Element {
        let wrapper = RenderObjectWrapper::<Leaf, _>::new(self.render, RuntimeArity::Exact(0));
        Element::with_mode(wrapper, ViewMode::RenderBox)
        // No children for Leaf
    }
}

impl<R> IntoElement for RenderBoxWithChild<R>
where
    R: Send + Sync + std::fmt::Debug + 'static,
{
    fn into_element(self) -> Element {
        let wrapper = RenderObjectWrapper::<Single, _>::new(self.render, RuntimeArity::Exact(1));
        Element::with_mode(wrapper, ViewMode::RenderBox).with_pending_children(vec![self.child])
    }
}

impl<R> IntoElement for RenderBoxWithChildren<R>
where
    R: Send + Sync + std::fmt::Debug + 'static,
{
    fn into_element(self) -> Element {
        let wrapper = RenderObjectWrapper::<Variable, _>::new(self.render, RuntimeArity::Variable);
        Element::with_mode(wrapper, ViewMode::RenderBox).with_pending_children(self.children)
    }
}

impl<R> IntoElement for RenderBoxWithOptionalChild<R>
where
    R: Send + Sync + std::fmt::Debug + 'static,
{
    fn into_element(self) -> Element {
        let has_child = self.child.is_some();
        let arity = if has_child {
            RuntimeArity::Exact(1)
        } else {
            RuntimeArity::Exact(0)
        };
        let wrapper = RenderObjectWrapper::<Optional, _>::new(self.render, arity);
        let mut element = Element::with_mode(wrapper, ViewMode::RenderBox);

        // Set pending children if child is present
        if let Some(child) = self.child {
            element = element.with_pending_children(vec![child]);
        }

        element
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for ergonomic render box operations.
///
/// Provides convenience methods for converting RenderBox to Element,
/// enabling widgets to use builder-style API like `RenderPadding::new().child(...)`.
///
/// Note: This trait is independent of tree type T since these are builder methods
/// that don't require tree access.
pub trait RenderBoxExt: Sized {
    /// Checks if position is within the given size bounds.
    fn contains(&self, position: Offset, size: Size) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
    }

    /// Convert leaf render object to element (no children).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::prelude::*;
    ///
    /// impl StatelessView for Text {
    ///     fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
    ///         RenderParagraph::new(self.data).leaf()
    ///     }
    /// }
    /// ```
    fn leaf(self) -> RenderBoxLeaf<Self>
    where
        Self: 'static,
    {
        RenderBoxLeaf { render: self }
    }

    /// Add single child to render object.
    ///
    /// Accepts anything that can be converted to `Element` (including `Child` from flui-view).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::prelude::*;
    /// use flui_view::Child;
    ///
    /// impl StatelessView for Padding {
    ///     fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
    ///         RenderPadding::new(self.padding).with_child(self.child)  // child: Child
    ///     }
    /// }
    /// ```
    fn with_child<C>(self, child: C) -> RenderBoxWithChild<Self>
    where
        Self: 'static,
        C: flui_element::IntoElement,
    {
        RenderBoxWithChild {
            render: self,
            child: child.into_element(),
        }
    }

    /// Add multiple children to render object.
    ///
    /// Accepts `Vec<Element>`, `Vec<impl IntoElement>`, or iterators.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::prelude::*;
    /// use flui_view::Children;
    ///
    /// impl StatelessView for Row {
    ///     fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
    ///         RenderFlex::row().with_children(self.children)  // children: Children
    ///     }
    /// }
    /// ```
    fn with_children<I>(self, children: I) -> RenderBoxWithChildren<Self>
    where
        Self: 'static,
        I: IntoIterator,
        I::Item: flui_element::IntoElement,
    {
        RenderBoxWithChildren {
            render: self,
            children: children.into_iter().map(|c| c.into_element()).collect(),
        }
    }

    /// Add optional child to render object.
    ///
    /// Accepts either `Option<Element>` or `Child` from flui-view.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_rendering::prelude::*;
    /// use flui_view::Child;
    ///
    /// impl StatelessView for Container {
    ///     fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
    ///         RenderSizedBox::new(self.width, self.height)
    ///             .maybe_child(self.child)  // self.child: Child
    ///     }
    /// }
    /// ```
    fn maybe_child<C>(self, child: C) -> RenderBoxWithOptionalChild<Self>
    where
        Self: 'static,
        C: Into<Option<Element>>,
    {
        RenderBoxWithOptionalChild {
            render: self,
            child: child.into(),
        }
    }
}

/// Blanket implementation for all types that can be render boxes.
/// The actual RenderBox<T, A> bound is checked at IntoElement impl site.
impl<R> RenderBoxExt for R {}

// ============================================================================
// EMPTY RENDER
// ============================================================================

/// Empty render object with zero size.
///
/// Used for `Option::None` and placeholder elements.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyRender;

impl<T: super::render_tree::FullRenderTree> RenderBox<T, Leaf> for EmptyRender {
    fn layout(&mut self, _ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size {
        Size::ZERO
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, T, Leaf>) {
        // Nothing to paint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_render_default() {
        let empty = EmptyRender::default();
        assert_eq!(
            empty.debug_name(),
            "flui_rendering::core::render_box::EmptyRender"
        );
    }

    // Note: Testing RenderBoxExt API requires RenderBox + RenderObject implementations.
    // EmptyRender only implements RenderBox<Leaf>, not RenderObject.
    // The API is tested in actual widget implementations (e.g., Padding, Container).
}
