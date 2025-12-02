//! Simple proxy traits for pass-through render objects
//!
//! These traits provide default implementations for render objects that simply
//! forward layout/paint/hit-test to their child without modification.
//!
//! # When to use
//!
//! Use `SimpleBoxProxy` / `SimpleSliverProxy` for simple pass-through wrappers
//! that don't modify layout, paint, or hit testing behavior.
//!
//! For more complex proxies that transform constraints, sizes, or offsets,
//! use the `ProxyRender` trait from `wrappers.rs` instead.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::SimpleBoxProxy;
//!
//! #[derive(Debug)]
//! struct RenderSemantics {
//!     label: String,
//! }
//!
//! // Just implement the proxy trait - get full RenderBox impl for free!
//! impl<T: FullRenderTree> SimpleBoxProxy<T> for RenderSemantics {}
//! ```

use crate::core::{
    BoxProtocol, FullRenderTree, HitTestContext, HitTestResult, LayoutContext, PaintContext,
    RenderBox, RenderSliver, Single, SliverProtocol,
};
use flui_types::{Size, SliverGeometry};
use std::fmt::Debug;

// ============================================================================
// SIMPLE BOX PROXY
// ============================================================================

/// Simple proxy for Box protocol with Single child.
///
/// Implement this trait for render objects that simply pass through to their
/// child without modifying layout, paint, or hit testing behavior.
///
/// For complex proxies that transform constraints/sizes/offsets, use
/// `ProxyRender` from `wrappers.rs` instead.
///
/// # Type Parameters
///
/// - `T`: Tree type implementing `FullRenderTree`
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
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug)]
/// pub struct RenderMetadata {
///     pub label: String,
/// }
///
/// impl<T: FullRenderTree> SimpleBoxProxy<T> for RenderMetadata {}
/// // That's it! Now has full RenderBox implementation
/// ```
pub trait SimpleBoxProxy<T: FullRenderTree>: Debug + Send + Sync + 'static {
    /// Layout the child with the same constraints
    fn proxy_layout(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size {
        ctx.proxy()
    }

    /// Paint the child at the same offset
    fn proxy_paint(&self, ctx: &mut PaintContext<'_, T, Single>) {
        ctx.proxy();
    }

    /// Forward hit test to child
    fn proxy_hit_test(
        &self,
        ctx: &HitTestContext<'_, T, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool {
        // Default: just return false (no hit)
        // Override if you need custom hit test behavior
        let _ = (ctx, result);
        false
    }
}

/// Blanket implementation: any SimpleBoxProxy<T> automatically implements RenderBox<T, Single>
impl<T: FullRenderTree, P: SimpleBoxProxy<T>> RenderBox<T, Single> for P {
    fn layout(&mut self, ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, T, Single>) {
        self.proxy_paint(ctx);
    }

    fn hit_test(
        &self,
        ctx: &HitTestContext<'_, T, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool {
        self.proxy_hit_test(ctx, result)
    }
}

// ============================================================================
// SIMPLE SLIVER PROXY
// ============================================================================

/// Simple proxy for Sliver protocol with Single child.
///
/// Similar to `SimpleBoxProxy` but for sliver render objects in scrollable
/// containers.
///
/// For complex proxies that transform constraints/geometry, use
/// `ProxyRender` from `wrappers.rs` instead.
///
/// # Type Parameters
///
/// - `T`: Tree type implementing `FullRenderTree`
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
/// - `proxy_hit_test`: Default (no hit)
///
/// Override any method to customize behavior while keeping others default.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::SimpleSliverProxy;
///
/// #[derive(Debug)]
/// pub struct RenderSliverMetadata {
///     pub id: usize,
/// }
///
/// impl<T: FullRenderTree> SimpleSliverProxy<T> for RenderSliverMetadata {}
/// // That's it! Now has full SliverRender implementation
/// ```
pub trait SimpleSliverProxy<T: FullRenderTree>: Debug + Send + Sync + 'static {
    /// Layout the child with the same sliver constraints
    fn proxy_layout(
        &mut self,
        mut ctx: LayoutContext<'_, T, Single, SliverProtocol>,
    ) -> SliverGeometry {
        ctx.proxy()
    }

    /// Paint the child at the same offset
    fn proxy_paint(&self, ctx: &mut PaintContext<'_, T, Single>) {
        ctx.proxy();
    }

    /// Forward hit test to child (default: no hit)
    fn proxy_hit_test(
        &self,
        _ctx: &HitTestContext<'_, T, Single, SliverProtocol>,
        _result: &mut HitTestResult,
    ) -> bool {
        false
    }
}

/// Blanket implementation: any SimpleSliverProxy<T> automatically implements RenderSliver<T, Single>
impl<T: FullRenderTree, P: SimpleSliverProxy<T>> RenderSliver<T, Single> for P {
    fn layout(&mut self, ctx: LayoutContext<'_, T, Single, SliverProtocol>) -> SliverGeometry {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut PaintContext<'_, T, Single>) {
        self.proxy_paint(ctx);
    }

    fn hit_test(
        &self,
        ctx: &HitTestContext<'_, T, Single, SliverProtocol>,
        result: &mut HitTestResult,
    ) -> bool {
        self.proxy_hit_test(ctx, result)
    }
}
