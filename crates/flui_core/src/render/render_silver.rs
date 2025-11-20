//! Sliver protocol render trait and extensions.
//!
//! This module provides the `SliverRender<A>` trait for implementing render objects
//! that participate in scrollable layouts (viewports), along with ergonomic builder
//! extensions for creating elements.
//!
//! # Sliver vs Box
//!
//! - **Box**: Fixed 2D layout with `BoxConstraints` → `Size`
//! - **Sliver**: Scrollable layout with `SliverConstraints` → `SliverGeometry`
//!
//! # Architecture
//!
//! ```text
//! SliverRender<A> trait
//! ├── layout() → SliverGeometry
//! ├── paint() → Canvas
//! └── hit_test() → bool
//!
//! SliverExt builder
//! ├── leaf() → SliverWithLeaf
//! ├── child() → SliverWithChild
//! ├── maybe_child() → SliverWithOptionalChild
//! └── children() → SliverWithChildren
//! ```

use crate::element::hit_test::SliverHitTestResult;
use crate::element::Element;
use crate::render::arity::{Arity, ChildrenAccess, Leaf, Single, Variable};
use crate::render::contexts::{HitTestContext, LayoutContext, PaintContext};
use crate::render::protocol::SliverProtocol;
use crate::render::render_element::RenderElement;
use crate::view::IntoElement;
use flui_types::SliverGeometry;
use std::fmt::Debug;

// ============================================================================
// SLIVER RENDER TRAIT
// ============================================================================

/// Sliver protocol render trait
///
/// Implement this trait for render objects that participate in scrollable layouts.
///
/// # Type Parameters
///
/// - `A`: Arity - compile-time child count (Leaf, Single, Variable, etc.)
///
/// # Example
///
/// ```rust,ignore
/// impl SliverRender<Variable> for RenderSliverList {
///     fn layout(&mut self, ctx: LayoutContext<'_, Variable, SliverProtocol>) -> SliverGeometry {
///         // Layout children and compute sliver geometry
///         SliverGeometry {
///             scroll_extent: total_height,
///             paint_extent: visible_height,
///             ..Default::default()
///         }
///     }
///
///     fn paint(&self, ctx: &mut PaintContext<'_, Variable>) {
///         // Paint visible children
///     }
/// }
/// ```
pub trait SliverRender<A: Arity>: Send + Sync + Debug + 'static {
    /// Computes the sliver geometry given constraints.
    ///
    /// Returns `SliverGeometry` describing scroll extent, paint extent,
    /// layout extent, and other properties for viewport integration.
    ///
    /// # Contract
    ///
    /// - Must respect `ctx.constraints` (remaining paint extent, cache extent)
    /// - Should return accurate `scroll_extent` for proper scrollbar behavior
    fn layout(&mut self, ctx: LayoutContext<'_, A, SliverProtocol>) -> SliverGeometry;

    /// Paints the sliver to the canvas.
    ///
    /// Only paint content within the visible region defined by the geometry.
    fn paint(&self, ctx: &mut PaintContext<'_, A>);

    /// Performs hit testing for pointer events.
    ///
    /// Default implementation first checks visibility, then tests children,
    /// then tests self. Override for custom behavior.
    ///
    /// Returns `true` if this sliver or any child was hit.
    fn hit_test(
        &self,
        ctx: HitTestContext<'_, A, SliverProtocol>,
        result: &mut SliverHitTestResult,
    ) -> bool {
        if !ctx.is_visible() {
            return false;
        }
        let hit_children = self.hit_test_children(&ctx, result);
        if hit_children || self.hit_test_self(ctx.main_axis_position(), ctx.cross_axis_position()) {
            result.add(
                ctx.element_id,
                crate::element::hit_test_entry::SliverHitTestEntry::new(
                    ctx.position,
                    ctx.geometry,
                    0.0,
                    ctx.main_axis_position(),
                ),
            );
            return true;
        }
        false
    }

    /// Tests if the position hits this sliver (excluding children).
    ///
    /// Default returns `false` (transparent to hit testing).
    fn hit_test_self(&self, _main_axis: f32, _cross_axis: f32) -> bool {
        false
    }

    /// Tests if the position hits any children.
    ///
    /// Default iterates children and tests each.
    fn hit_test_children(
        &self,
        ctx: &HitTestContext<'_, A, SliverProtocol>,
        result: &mut SliverHitTestResult,
    ) -> bool {
        let mut hit = false;
        for &child in ctx.children.as_slice().iter() {
            if ctx.hit_test_child(child, ctx.position, result) {
                hit = true;
            }
        }
        hit
    }

    /// Returns a debug name for this sliver render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for ergonomic sliver render object construction.
///
/// Provides builder-style methods to attach children to sliver render objects,
/// automatically creating the appropriate `IntoElement` wrapper.
///
/// # Example
///
/// ```rust,ignore
/// // Leaf (no children)
/// RenderSliverFillRemaining::new().leaf()
///
/// // Single child
/// RenderSliverPadding::new(padding).child(child_sliver)
///
/// // Multiple children
/// RenderSliverList::new().children(vec![item1, item2, item3])
/// ```
pub trait SliverExt: Sized {
    /// Wraps this sliver render as a leaf element (no children).
    fn leaf(self) -> SliverWithLeaf<Self>
    where
        Self: SliverRender<Leaf>,
    {
        SliverWithLeaf { render: self }
    }

    /// Wraps this sliver render with a single child.
    fn child<C: IntoElement>(self, child: C) -> SliverWithChild<Self, C>
    where
        Self: SliverRender<Single>,
    {
        SliverWithChild {
            render: self,
            child,
        }
    }

    /// Wraps this sliver render with an optional child.
    fn maybe_child<C: IntoElement>(self, child: Option<C>) -> SliverWithOptionalChild<Self, C>
    where
        Self: SliverRender<Single>,
    {
        SliverWithOptionalChild {
            render: self,
            child,
        }
    }

    /// Wraps this sliver render with multiple children.
    fn children<C: IntoElement>(self, children: Vec<C>) -> SliverWithChildren<Self, C>
    where
        Self: SliverRender<Variable>,
    {
        SliverWithChildren {
            render: self,
            children,
        }
    }
}

impl<S> SliverExt for S {}

// ============================================================================
// BUILDER WRAPPERS
// ============================================================================

/// Builder wrapper for leaf sliver render objects (no children).
///
/// Created by [`SliverExt::leaf()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct SliverWithLeaf<S> {
    /// The sliver render object.
    pub render: S,
}

/// Builder wrapper for single-child sliver render objects.
///
/// Created by [`SliverExt::child()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct SliverWithChild<S, C> {
    /// The sliver render object.
    pub render: S,
    /// The child element.
    pub child: C,
}

/// Builder wrapper for sliver render objects with optional child.
///
/// Created by [`SliverExt::maybe_child()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct SliverWithOptionalChild<S, C> {
    /// The sliver render object.
    pub render: S,
    /// The optional child element.
    pub child: Option<C>,
}

/// Builder wrapper for multi-child sliver render objects.
///
/// Created by [`SliverExt::children()`]. Implements `IntoElement`.
#[derive(Debug)]
pub struct SliverWithChildren<S, C> {
    /// The sliver render object.
    pub render: S,
    /// The child elements.
    pub children: Vec<C>,
}

// ============================================================================
// INTO ELEMENT IMPLEMENTATIONS
// ============================================================================

impl<S: SliverRender<Leaf>> IntoElement for SliverWithLeaf<S> {
    fn into_element(self) -> Element {
        Element::Render(RenderElement::sliver::<Leaf, _>(self.render))
    }
}

impl<S: SliverRender<Single>, C: IntoElement> IntoElement for SliverWithChild<S, C> {
    fn into_element(self) -> Element {
        let child = self.child.into_element();
        let mut elem = RenderElement::sliver::<Single, _>(self.render);
        elem.set_unmounted_children(vec![child]);
        Element::Render(elem)
    }
}

impl<S: SliverRender<Single>, C: IntoElement> IntoElement for SliverWithOptionalChild<S, C> {
    fn into_element(self) -> Element {
        let children: Vec<Element> = self.child.into_iter().map(|c| c.into_element()).collect();
        let mut elem = RenderElement::sliver::<Single, _>(self.render);
        if !children.is_empty() {
            elem.set_unmounted_children(children);
        }
        Element::Render(elem)
    }
}

impl<S: SliverRender<Variable>, C: IntoElement> IntoElement for SliverWithChildren<S, C> {
    fn into_element(self) -> Element {
        let children: Vec<Element> = self
            .children
            .into_iter()
            .map(|c| c.into_element())
            .collect();
        let mut elem = RenderElement::sliver::<Variable, _>(self.render);
        if !children.is_empty() {
            elem.set_unmounted_children(children);
        }
        Element::Render(elem)
    }
}
