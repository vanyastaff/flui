//! Proxy traits for pass-through render objects
//!
//! These traits provide default implementations for render objects that simply
//! forward layout/paint/hit-test to their child without modification.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::RenderBoxProxy;
//!
//! #[derive(Debug)]
//! struct RenderSemantics {
//!     label: String,
//! }
//!
//! // Just implement the proxy trait - get full RenderBox impl for free!
//! impl RenderBoxProxy for RenderSemantics {}
//! ```

use crate::core::{
    BoxProtocol, HitTestContext, HitTestResult, HitTestTree, LayoutContext, LayoutTree,
    PaintContext, PaintTree, RenderBox, Single, SliverProtocol, SliverRender,
};
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
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug)]
/// pub struct RenderMetadata {
///     pub label: String,
/// }
///
/// impl RenderBoxProxy for RenderMetadata {}
/// // That's it! Now has full RenderBox implementation
/// ```
pub trait RenderBoxProxy: Debug + Send + Sync + 'static {
    /// Layout the child with the same constraints
    fn proxy_layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        ctx.proxy()
    }

    /// Paint the child at the same offset
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        ctx.proxy();
    }

    /// Forward hit test to child
    fn proxy_hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        // Default: just return false (no hit)
        // Override if you need custom hit test behavior
        let _ = (ctx, result);
        false
    }
}

/// Blanket implementation: any RenderBoxProxy automatically implements RenderBox<Single>
impl<P: RenderBoxProxy> RenderBox<Single> for P {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        self.proxy_layout(ctx)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        self.proxy_paint(ctx);
    }

    fn hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, Single, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
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
/// - `proxy_hit_test`: Default (no hit)
///
/// Override any method to customize behavior while keeping others default.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverProxy;
///
/// #[derive(Debug)]
/// pub struct RenderSliverMetadata {
///     pub id: usize,
/// }
///
/// impl RenderSliverProxy for RenderSliverMetadata {}
/// // That's it! Now has full SliverRender implementation
/// ```
pub trait RenderSliverProxy: Debug + Send + Sync + 'static {
    /// Layout the child with the same sliver constraints
    fn proxy_layout<T>(
        &mut self,
        mut ctx: LayoutContext<'_, T, Single, SliverProtocol>,
    ) -> SliverGeometry
    where
        T: LayoutTree,
    {
        ctx.proxy()
    }

    /// Paint the child at the same offset
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        ctx.proxy();
    }

    /// Forward hit test to child (default: no hit)
    fn proxy_hit_test<T>(
        &self,
        _ctx: &HitTestContext<'_, T, Single, SliverProtocol>,
        _result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        false
    }
}

/// Blanket implementation: any RenderSliverProxy automatically implements SliverRender<Single>
impl<P: RenderSliverProxy> SliverRender<Single> for P {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Single, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree,
    {
        self.proxy_layout(ctx)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        self.proxy_paint(ctx);
    }

    fn hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, Single, SliverProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        self.proxy_hit_test(ctx, result)
    }
}
