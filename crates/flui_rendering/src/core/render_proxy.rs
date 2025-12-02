//! Proxy traits for pass-through render objects.
//!
//! This module provides traits for render objects that simply forward their
//! operations to a single child without modification, making it easy to add
//! metadata, semantic labels, or other transparent wrappers.
//!
//! # Design Philosophy
//!
//! - **Minimal boilerplate**: Just implement the proxy trait, get full RenderBox/RenderSliver for free
//! - **Transparent forwarding**: All operations pass through to child by default
//! - **Optional hooks**: Override specific methods when you need custom behavior
//! - **Type safe**: Compile-time validation of child relationships
//!
//! # Proxy Types
//!
//! ## RenderProxyBox
//!
//! For transparent box protocol wrappers (always has exactly one child):
//! - Forwards layout to child
//! - Forwards paint to child
//! - Forwards hit testing to child
//! - Use for: metadata, semantic labels, debugging wrappers
//!
//! ## RenderProxySliver
//!
//! For transparent sliver protocol wrappers (always has exactly one child):
//! - Forwards layout to child
//! - Forwards paint to child
//! - Forwards hit testing to child
//! - Use for: scroll metadata, semantic labels
//!
//! # Examples
//!
//! ## Simple Metadata Wrapper
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderProxyBox, RenderObject};
//!
//! #[derive(Debug)]
//! struct RenderMetadata {
//!     label: String,
//! }
//!
//! // That's it! Full RenderBox implementation for free
//! impl RenderProxyBox for RenderMetadata {}
//!
//! impl RenderObject for RenderMetadata {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! ## Custom Transform Proxy
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderOpacity {
//!     opacity: f32,
//! }
//!
//! impl RenderProxyBox for RenderOpacity {
//!     // Override paint to apply opacity
//!     fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
//!         ctx.canvas_mut().save();
//!         ctx.canvas_mut().set_opacity(self.opacity);
//!         ctx.paint_single_child(Offset::ZERO);
//!         ctx.canvas_mut().restore();
//!     }
//! }
//!
//! impl RenderObject for RenderOpacity {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```

use std::fmt;

use super::arity::Single;
use super::contexts::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, SliverHitTestContext,
    SliverLayoutContext, SliverPaintContext,
};
use super::render_box::RenderBox;
use super::render_object::RenderObject;
use super::render_sliver::RenderSliver;
use crate::RenderResult;
use flui_interaction::HitTestResult;
use flui_types::{Offset, Rect, Size, SliverGeometry};

// ============================================================================
// RENDER PROXY BOX
// ============================================================================

/// Trait for box protocol renders objects that forward all operations to a single child.
///
/// Implement this trait to create transparent wrappers around box render objects.
/// All methods have default implementations that simply pass through to the child.
///
/// # Arity
///
/// Proxy boxes always have exactly one child (arity = `Single`).
///
/// # Default Behavior
///
/// - **Layout**: Passes constraints directly to child, returns child size
/// - **Paint**: Paints child at offset (0, 0)
/// - **Hit test**: Forwards to child
///
/// # Override Points
///
/// Override any method to customize behavior:
/// - [`proxy_layout`](Self::proxy_layout) - Modify constraints or size
/// - [`proxy_paint`](Self::proxy_paint) - Add visual effects
/// - [`proxy_hit_test`](Self::proxy_hit_test) - Custom hit testing
///
/// # Examples
///
/// ## Minimal Proxy (Metadata)
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderSemantics {
///     label: String,
/// }
///
/// impl RenderProxyBox for RenderSemantics {}
///
/// impl RenderObject for RenderSemantics {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
///
/// ## Custom Layout Proxy
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderConstrainedProxy {
///     max_width: f32,
/// }
///
/// impl RenderProxyBox for RenderConstrainedProxy {
///     fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> Size {
///         // Constrain child width
///         let constrained = BoxConstraints {
///             max_width: self.max_width.min(ctx.constraints.max_width),
///             ..ctx.constraints
///         };
///         ctx.layout_single_child_with(|_| constrained)
///     }
/// }
///
/// impl RenderObject for RenderConstrainedProxy {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
pub trait RenderProxyBox: RenderObject + fmt::Debug + Send + Sync {
    /// Performs layout by forwarding to the child.
    ///
    /// The default implementation passes constraints directly to child and returns child size.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default: pass through
    /// fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> Size {
    ///     ctx.layout_single_child()
    /// }
    ///
    /// // Custom: modify constraints
    /// fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> Size {
    ///     let modified = ctx.constraints.loosen();
    ///     ctx.layout_single_child_with(|_| modified)
    /// }
    /// ```
    fn proxy_layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        // Default: pass constraints directly to the child
        ctx.layout_single_child()
    }

    /// Performs painting by forwarding to the child.
    ///
    /// Default implementation paints child at offset (0, 0).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default: paint at origin
    /// fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
    ///     ctx.paint_single_child(Offset::ZERO);
    /// }
    ///
    /// // Custom: add visual effects
    /// fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
    ///     ctx.canvas_mut().save();
    ///     ctx.canvas_mut().set_opacity(0.5);
    ///     ctx.paint_single_child(Offset::ZERO);
    ///     ctx.canvas_mut().restore();
    /// }
    /// ```
    fn proxy_paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        // Default: paint child at origin
        ctx.paint_single_child(Offset::ZERO);
    }

    /// Performs hit testing by forwarding to the child.
    ///
    /// Default implementation forwards to child and adds self if the child was hit.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default: forward to child
    /// fn proxy_hit_test(
    ///     &self,
    ///     ctx: &BoxHitTestContext<'_, Single>,
    ///     result: &mut HitTestResult,
    /// ) -> bool {
    ///     if ctx.hit_test_single_child(ctx.position, result) {
    ///         ctx.hit_test_self(result);
    ///         true
    ///     } else {
    ///         false
    ///     }
    /// }
    ///
    /// // Custom: always handle hits
    /// fn proxy_hit_test(
    ///     &self,
    ///     ctx: &BoxHitTestContext<'_, Single>,
    ///     result: &mut HitTestResult,
    /// ) -> bool {
    ///     ctx.hit_test_single_child(ctx.position, result);
    ///     ctx.hit_test_self(result);
    ///     true  // Always handle
    /// }
    /// ```
    fn proxy_hit_test(
        &self,
        ctx: &BoxHitTestContext<'_, Single>,
        result: &mut HitTestResult,
    ) -> bool {
        // Default: forward to child, add self if child was hit
        if ctx.hit_test_single_child(ctx.position, result) {
            ctx.hit_test_self(result);
            true
        } else {
            false
        }
    }

    /// Gets the local bounding rectangle.
    ///
    /// Default uses the RenderObject's bounds.
    fn proxy_local_bounds(&self) -> Rect {
        (self as &dyn RenderObject).local_bounds()
    }
}

// Blanket implementation: RenderProxyBox -> RenderBox
impl<T: RenderProxyBox> RenderBox<Single> for T {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
        self.proxy_paint(ctx)
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        self.proxy_hit_test(ctx, result)
    }

    fn local_bounds(&self) -> Rect {
        self.proxy_local_bounds()
    }
}

// ============================================================================
// RENDER PROXY SLIVER
// ============================================================================

/// Trait for sliver protocol render objects that forward all operations to a single child.
///
/// Implement this trait to create transparent wrappers around sliver render objects.
/// All methods have default implementations that simply pass through to the child.
///
/// # Arity
///
/// Proxy slivers always have exactly one child (arity = `Single`).
///
/// # Default Behavior
///
/// - **Layout**: Passes constraints directly to child, returns child geometry
/// - **Paint**: Paints child at offset (0, 0)
/// - **Hit test**: Forwards to child
///
/// # Override Points
///
/// Override any method to customize behavior:
/// - [`proxy_layout`](Self::proxy_layout) - Modify constraints or geometry
/// - [`proxy_paint`](Self::proxy_paint) - Add visual effects
/// - [`proxy_hit_test`](Self::proxy_hit_test) - Custom hit testing
///
/// # Examples
///
/// ## Minimal Proxy (Metadata)
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderSliverSemantics {
///     label: String,
/// }
///
/// impl RenderProxySliver for RenderSliverSemantics {}
///
/// impl RenderObject for RenderSliverSemantics {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
///
/// ## Custom Layout Proxy
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderSliverOffsetProxy {
///     offset: f32,
/// }
///
/// impl RenderProxySliver for RenderSliverOffsetProxy {
///     fn proxy_layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> SliverGeometry {
///         // Adjust scroll offset
///         let adjusted = SliverConstraints {
///             scroll_offset: (ctx.constraints.scroll_offset - self.offset).max(0.0),
///             ..ctx.constraints
///         };
///
///         let mut geometry = ctx.layout_single_child_with(|_| adjusted);
///         geometry.scroll_extent += self.offset;
///         geometry
///     }
/// }
///
/// impl RenderObject for RenderSliverOffsetProxy {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// ```
pub trait RenderProxySliver: RenderObject + fmt::Debug + Send + Sync {
    /// Performs layout by forwarding to the child.
    ///
    /// The default implementation passes constraints directly to child and returns child geometry.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default: pass through
    /// fn proxy_layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> SliverGeometry {
    ///     ctx.layout_single_child()
    /// }
    ///
    /// // Custom: modify constraints
    /// fn proxy_layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> SliverGeometry {
    ///     let modified = SliverConstraints {
    ///         scroll_offset: ctx.constraints.scroll_offset + 100.0,
    ///         ..ctx.constraints
    ///     };
    ///     ctx.layout_single_child_with(|_| modified)
    /// }
    /// ```
    fn proxy_layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> SliverGeometry {
        // Default: pass constraints directly to the child
        ctx.layout_single_child()
    }

    /// Performs painting by forwarding to the child.
    ///
    /// Default implementation paints child at offset (0, 0).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default: paint at origin
    /// fn proxy_paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
    ///     ctx.paint_single_child(Offset::ZERO);
    /// }
    ///
    /// // Custom: offset painting
    /// fn proxy_paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
    ///     let offset = Offset::new(0.0, 10.0);
    ///     ctx.paint_single_child(offset);
    /// }
    /// ```
    fn proxy_paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // Default: paint child at origin
        ctx.paint_single_child(Offset::ZERO);
    }

    /// Performs hit testing by forwarding to the child.
    ///
    /// Default implementation forwards to child and adds self if child was hit.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Default: forward to child
    /// fn proxy_hit_test(
    ///     &self,
    ///     ctx: &SliverHitTestContext<'_, Single>,
    ///     result: &mut HitTestResult,
    /// ) -> bool {
    ///     if ctx.hit_test_single_child(ctx.position, result) {
    ///         ctx.hit_test_self(result);
    ///         true
    ///     } else {
    ///         false
    ///     }
    /// }
    /// ```
    fn proxy_hit_test(
        &self,
        ctx: &SliverHitTestContext<'_, Single>,
        result: &mut HitTestResult,
    ) -> bool {
        // Default: forward to child, add self if child was hit
        if ctx.hit_test_single_child(ctx.position, result) {
            ctx.hit_test_self(result);
            true
        } else {
            false
        }
    }

    /// Gets the local bounding rectangle.
    ///
    /// Default uses the RenderObject's bounds.
    fn proxy_local_bounds(&self) -> Rect {
        (self as &dyn RenderObject).local_bounds()
    }
}

// Blanket implementation: RenderProxySliver -> RenderSliver
impl<T: RenderProxySliver> RenderSliver<Single> for T {
    fn layout(&mut self, ctx: SliverLayoutContext<'_, Single>) -> SliverGeometry {
        self.proxy_layout(ctx)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        self.proxy_paint(ctx)
    }

    fn hit_test(&self, ctx: &SliverHitTestContext<'_, Single>, result: &mut HitTestResult) -> bool {
        self.proxy_hit_test(ctx, result)
    }

    fn local_bounds(&self) -> Rect {
        self.proxy_local_bounds()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test proxy box
    #[derive(Debug)]
    struct TestProxyBox {
        label: String,
    }

    impl RenderProxyBox for TestProxyBox {}

    impl RenderObject for TestProxyBox {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    // Simple test proxy sliver
    #[derive(Debug)]
    struct TestProxySliver {
        label: String,
    }

    impl RenderProxySliver for TestProxySliver {}

    impl RenderObject for TestProxySliver {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_proxy_box_is_render_box() {
        let proxy = TestProxyBox {
            label: "test".to_string(),
        };

        // Should compile - TestProxyBox implements RenderBox<Single>
        let _: &dyn RenderBox<Single> = &proxy;
    }

    #[test]
    fn test_proxy_sliver_is_render_sliver() {
        let proxy = TestProxySliver {
            label: "test".to_string(),
        };

        // Should compile - TestProxySliver implements RenderSliver<Single>
        let _: &dyn RenderSliver<Single> = &proxy;
    }
}
