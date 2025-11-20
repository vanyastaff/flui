//! Proxy traits for pass-through render objects
//!
//! These traits provide default implementations for render objects that simply
//! forward layout/paint/hit-test to their child without modification.
//!
//! # Example
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderSemantics {
//!     label: String,
//! }
//!
//! // Just implement the proxy trait - get full Render impl for free!
//! impl RenderBoxProxy for RenderSemantics {}
//! ```

use crate::element::hit_test::BoxHitTestResult;
use crate::render::arity::Single;
use crate::render::contexts::{HitTestContext, LayoutContext, PaintContext};
use crate::render::protocol::{BoxProtocol, SliverProtocol};
use crate::render::render_box::RenderBox;
use crate::render::render_silver::SliverRender;
use flui_types::{Size, SliverGeometry};
use std::fmt::Debug;

// ============================================================================
// BOX PROXY
// ============================================================================

/// Proxy for Box protocol with Single child
///
/// Implement this trait for render objects that simply pass through to their
/// child without modifying layout, paint, or hit testing behavior.
///
/// Perfect for:
/// - Semantic annotations
/// - Debug wrappers
/// - Event listeners that don't affect layout
///
/// # Default Behavior
///
/// - `proxy_layout`: Passes constraints directly to child, returns child size
/// - `proxy_paint`: Paints child at same offset
/// - `proxy_hit_test`: Forwards hit test to child
///
/// Override any method to customize behavior while keeping others default.
pub trait RenderBoxProxy: Debug + Send + Sync + 'static {
    /// Layout the child with the same constraints
    fn proxy_layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        ctx.layout_child(ctx.children.single(), ctx.constraints)
    }

    /// Paint the child at the same offset
    fn proxy_paint(&self, ctx: &mut PaintContext<'_, Single>) {
        ctx.paint_child(ctx.children.single(), ctx.offset);
    }

    /// Forward hit test to child
    fn proxy_hit_test(
        &self,
        ctx: HitTestContext<'_, Single, BoxProtocol>,
        result: &mut BoxHitTestResult,
    ) -> bool {
        ctx.hit_test_child(ctx.children.single(), ctx.position, result)
    }
}

impl<T: RenderBoxProxy> RenderBox<Single> for T {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        self.proxy_paint(ctx);
    }

    fn hit_test(
        &self,
        ctx: HitTestContext<'_, Single, BoxProtocol>,
        result: &mut BoxHitTestResult,
    ) -> bool {
        self.proxy_hit_test(ctx, result)
    }
}

// ============================================================================
// SLIVER PROXY
// ============================================================================

/// Proxy for Sliver protocol with Single child.
///
/// Similar to `RenderBoxProxy` but for sliver render objects in scrollable
/// containers.
///
/// Perfect for:
/// - Semantic annotations on slivers
/// - Debug wrappers for scrollable content
/// - Event listeners that don't affect sliver geometry
///
/// # Default Behavior
///
/// - `proxy_layout`: Passes sliver constraints directly to child
/// - `proxy_paint`: Paints child at same offset
pub trait RenderSliverProxy: Debug + Send + Sync + 'static {
    /// Layouts the child with the same sliver constraints.
    fn proxy_layout(&mut self, ctx: LayoutContext<'_, Single, SliverProtocol>) -> SliverGeometry {
        ctx.layout_child(ctx.children.single(), ctx.constraints)
    }

    /// Paints the child at the same offset.
    fn proxy_paint(&self, ctx: &mut PaintContext<'_, Single>) {
        ctx.paint_child(ctx.children.single(), ctx.offset);
    }
}

impl<T: RenderSliverProxy> SliverRender<Single> for T {
    fn layout(&mut self, ctx: LayoutContext<'_, Single, SliverProtocol>) -> SliverGeometry {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        self.proxy_paint(ctx);
    }
}
