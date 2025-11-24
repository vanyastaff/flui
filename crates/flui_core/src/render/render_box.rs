//! Box protocol render trait and extensions.
//!
//! This module provides the `RenderBox<A>` trait for implementing render objects
//! that use the standard 2D box layout protocol, along with ergonomic builder
//! extensions for creating elements.
//!
//! # Architecture
//!
//! ```text
//! RenderBox<A> trait
//! ├── layout() → Size
//! ├── paint() → Canvas
//! └── hit_test() → bool
//!
//! RenderBoxExt builder
//! ├── leaf() → WithLeaf
//! ├── child() → WithChild
//! ├── maybe_child() → WithOptionalChild
//! └── children() → WithChildren
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! // Using the builder API
//! let element = RenderPadding::new(padding)
//!     .child(RenderText::new("Hello").leaf());
//! ```

use crate::element::hit_test::BoxHitTestResult;
use crate::element::Element;
use crate::render::arity::{Arity, ChildrenAccess, Leaf, Optional, Single, Variable};
use crate::render::contexts::{HitTestContext, LayoutContext, PaintContext};
use crate::render::protocol::BoxProtocol;
use crate::render::render_element::RenderElement;
use crate::view::IntoElement;
use flui_types::{Offset, Size};
use std::fmt::Debug;

// ============================================================================
// RENDER TRAIT
// ============================================================================

/// Box protocol render trait
///
/// Implement this trait for render objects that use the standard 2D box layout.
///
/// # Type Parameters
///
/// - `A`: Arity - compile-time child count (Leaf, Single, Variable, etc.)
///
/// # Example
///
/// ```rust,ignore
/// impl RenderBox<Leaf> for RenderText {
///     fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
///         // Compute text size
///         ctx.constraints.constrain(self.measured_size)
///     }
///
///     fn paint(&self, ctx: &mut PaintContext<'_, Leaf>) {
///         ctx.canvas().draw_text(&self.text, self.offset);
///     }
/// }
/// ```
pub trait RenderBox<A: Arity>: Send + Sync + Debug + 'static {
    /// Computes the size of this render object given constraints.
    ///
    /// This method must:
    /// 1. Layout any children using `ctx.layout_child()`
    /// 2. Return a size that satisfies the constraints
    ///
    /// # Contract
    ///
    /// - Must return a size within `ctx.constraints`
    /// - Should cache expensive computations for use in `paint()`
    fn layout(&mut self, ctx: LayoutContext<'_, A, BoxProtocol>) -> Size;

    /// Paints this render object to the canvas.
    ///
    /// Called after layout. Use `ctx.canvas()` for drawing operations
    /// and `ctx.paint_child()` to paint children.
    fn paint(&self, ctx: &mut PaintContext<'_, A>);

    /// Performs hit testing for pointer events.
    ///
    /// Default implementation tests children first (front-to-back), then self.
    /// Override for custom hit test behavior (e.g., clipping, transforms).
    ///
    /// Returns `true` if this element or any child was hit.
    fn hit_test(
        &self,
        ctx: HitTestContext<'_, A, BoxProtocol>,
        result: &mut BoxHitTestResult,
    ) -> bool {
        let hit_children = self.hit_test_children(&ctx, result);
        if hit_children || self.hit_test_self(ctx.position) {
            result.add(
                ctx.element_id,
                crate::element::hit_test_entry::BoxHitTestEntry::new(ctx.position, ctx.size()),
            );
            return true;
        }
        false
    }

    /// Tests if the position hits this render object (excluding children).
    ///
    /// Override for opaque hit testing (e.g., buttons, interactive areas).
    /// Default returns `false` (transparent to hit testing).
    fn hit_test_self(&self, _position: Offset) -> bool {
        false
    }

    /// Tests if the position hits any children.
    ///
    /// Default iterates children and tests each. Override for custom
    /// traversal (e.g., z-order, clipping regions).
    fn hit_test_children(
        &self,
        ctx: &HitTestContext<'_, A, BoxProtocol>,
        result: &mut BoxHitTestResult,
    ) -> bool {
        let mut hit = false;
        for &child in ctx.children.as_slice().iter() {
            if ctx.hit_test_child(child, ctx.position, result) {
                hit = true;
            }
        }
        hit
    }

    /// Returns a debug name for this render object.
    ///
    /// Used for debugging and error messages. Default returns the type name.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for ergonomic render object construction.
///
/// Provides builder-style methods to attach children to render objects,
/// automatically creating the appropriate `IntoElement` wrapper.
///
/// # Example
///
/// ```rust,ignore
/// // Leaf (no children)
/// RenderColoredBox::new(color).leaf()
///
/// // Single child
/// RenderPadding::new(padding).child(child_element)
///
/// // Multiple children
/// RenderFlex::new(axis).children(vec![child1, child2])
/// ```
pub trait RenderBoxExt: Sized {
    /// Wraps this render as a leaf element (no children).
    fn leaf(self) -> WithLeaf<Self>
    where
        Self: RenderBox<Leaf>,
    {
        WithLeaf { render: self }
    }

    /// Wraps this render with a single child.
    fn child<C: IntoElement>(self, child: C) -> WithChild<Self, C>
    where
        Self: RenderBox<Single>,
    {
        WithChild {
            render: self,
            child,
        }
    }

    /// Wraps this render with an optional child (for Single arity).
    fn child_opt(self, child: impl Into<Option<Element>>) -> WithOptionalChild<Self>
    where
        Self: RenderBox<Single>,
    {
        WithOptionalChild {
            render: self,
            child: child.into(),
        }
    }

    /// Wraps this render with an optional child (for Optional arity).
    ///
    /// Use this for render objects that implement `RenderBox<Optional>`.
    fn maybe_child(self, child: impl Into<Option<Element>>) -> WithMaybeChild<Self>
    where
        Self: RenderBox<Optional>,
    {
        WithMaybeChild {
            render: self,
            child: child.into(),
        }
    }

    /// Wraps this render with multiple children.
    fn children(self, children: impl Into<Vec<Element>>) -> WithChildren<Self>
    where
        Self: RenderBox<Variable>,
    {
        WithChildren {
            render: self,
            children: children.into(),
        }
    }
}

impl<R> RenderBoxExt for R {}

// ============================================================================
// BUILDER WRAPPERS
// ============================================================================

/// Builder wrapper for leaf render objects (no children).
///
/// Created by [`RenderBoxExt::leaf()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct WithLeaf<R> {
    /// The render object.
    pub render: R,
}

/// Builder wrapper for single-child render objects.
///
/// Created by [`RenderBoxExt::child()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct WithChild<R, C> {
    /// The render object.
    pub render: R,
    /// The child element.
    pub child: C,
}

/// Builder wrapper for render objects with optional child (Single arity).
///
/// Created by [`RenderBoxExt::child_opt()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct WithOptionalChild<R> {
    /// The render object.
    pub render: R,
    /// The optional child element.
    pub child: Option<Element>,
}

/// Builder wrapper for render objects with optional child (Optional arity).
///
/// Created by [`RenderBoxExt::maybe_child()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct WithMaybeChild<R> {
    /// The render object.
    pub render: R,
    /// The optional child element.
    pub child: Option<Element>,
}

/// Builder wrapper for multi-child render objects.
///
/// Created by [`RenderBoxExt::children()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct WithChildren<R> {
    /// The render object.
    pub render: R,
    /// The child elements.
    pub children: Vec<Element>,
}

// ============================================================================
// SEALED TRAIT IMPLEMENTATIONS
// ============================================================================

impl<R> crate::element::into_element::sealed::Sealed for WithLeaf<R> {}

impl<R, C> crate::element::into_element::sealed::Sealed for WithChild<R, C> {}

impl<R> crate::element::into_element::sealed::Sealed for WithOptionalChild<R> {}

impl<R> crate::element::into_element::sealed::Sealed for WithMaybeChild<R> {}

impl<R> crate::element::into_element::sealed::Sealed for WithChildren<R> {}

// ============================================================================
// INTO ELEMENT IMPLEMENTATIONS
// ============================================================================

impl<R: RenderBox<Leaf>> IntoElement for WithLeaf<R> {
    fn into_element(self) -> Element {
        Element::from_render_element(RenderElement::r#box::<Leaf, _>(self.render))
    }
}

impl<R: RenderBox<Single>, C: IntoElement> IntoElement for WithChild<R, C> {
    fn into_element(self) -> Element {
        let child = self.child.into_element();

        let mut re = RenderElement::r#box::<Single, _>(self.render);

        re.set_unmounted_children(vec![child]);

        Element::from_render_element(re)
    }
}

impl<R: RenderBox<Single>> IntoElement for WithOptionalChild<R> {
    fn into_element(self) -> Element {
        let mut re = RenderElement::r#box::<Single, _>(self.render);

        if let Some(child) = self.child {
            re.set_unmounted_children(vec![child]);
        }

        Element::from_render_element(re)
    }
}

impl<R: RenderBox<Optional>> IntoElement for WithMaybeChild<R> {
    fn into_element(self) -> Element {
        let mut re = RenderElement::r#box::<Optional, _>(self.render);

        if let Some(child) = self.child {
            re.set_unmounted_children(vec![child]);
        }

        Element::from_render_element(re)
    }
}

impl<R: RenderBox<Variable>> IntoElement for WithChildren<R> {
    fn into_element(self) -> Element {
        let mut re = RenderElement::r#box::<Variable, _>(self.render);

        if !RenderElement::children(&re).is_empty() {
            // children were already mounted elsewhere; only attach unmounted if provided
        }
        if !self.children.is_empty() {
            re.set_unmounted_children(self.children);
        }

        Element::from_render_element(re)
    }
}

// ============================================================================
// EMPTY RENDER
// ============================================================================

/// Empty render object with zero size.
///
/// Used for `Option::None` and placeholder elements.
#[derive(Debug)]
pub struct EmptyRender;

impl RenderBox<Leaf> for EmptyRender {
    fn layout(
        &mut self,
        _ctx: LayoutContext<'_, Leaf, crate::render::BoxProtocol>,
    ) -> flui_types::Size {
        flui_types::Size::ZERO
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {
        // Empty - nothing to paint
    }
}
