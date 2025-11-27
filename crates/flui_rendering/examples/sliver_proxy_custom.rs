//! Custom RenderSliverProxy Implementations
//!
//! This example demonstrates how to create custom sliver proxy objects
//! that modify painting, hit testing, or layout behavior while maintaining
//! the sliver protocol.
//!
//! RenderSliverProxy is ideal for single-child sliver wrappers that need to:
//! - Apply visual effects (opacity, blur, color filters)
//! - Modify interaction (block pointer events, custom hit testing)
//! - Add debugging visualizations
//! - Apply layout constraints

use flui_rendering::core::{
    LayoutContext, LayoutTree, PaintContext, PaintTree, RenderSliverProxy, Single, SliverProtocol,
};
use flui_types::{SliverConstraints, SliverGeometry};

// ============================================================================
// Example 1: Simple Paint Override - Sliver Debug Outline
// ============================================================================

/// A debugging proxy that draws an outline around the sliver's paint area.
///
/// This is useful during development to visualize sliver boundaries and
/// understand how slivers are being laid out and painted.
#[derive(Debug)]
pub struct RenderSliverDebugOutline {
    /// Color of the debug outline (in RGBA format)
    pub outline_color: [f32; 4],
    /// Width of the outline stroke
    pub stroke_width: f32,
    /// Whether to show scroll offset indicators
    pub show_scroll_offset: bool,
}

impl RenderSliverDebugOutline {
    /// Create a new debug outline with default red color
    pub fn new() -> Self {
        Self {
            outline_color: [1.0, 0.0, 0.0, 0.5], // Semi-transparent red
            stroke_width: 2.0,
            show_scroll_offset: true,
        }
    }

    /// Set the outline color
    pub fn with_color(mut self, r: f32, g: f32, b: f32, a: f32) -> Self {
        self.outline_color = [r, g, b, a];
        self
    }

    /// Set the stroke width
    pub fn with_stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }

    /// Enable/disable scroll offset indicators
    pub fn with_scroll_offset_indicator(mut self, show: bool) -> Self {
        self.show_scroll_offset = show;
        self
    }
}

impl Default for RenderSliverDebugOutline {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderSliverProxy for RenderSliverDebugOutline {
    // Layout: Use default (passes through unchanged)

    // Paint: Custom implementation to draw debug outline
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // First, paint the child normally
        ctx.proxy();

        // TODO: When canvas API is available, draw debug outline here:
        // - Draw rectangle around paint extent
        // - Draw scroll offset marker if enabled
        // - Draw text showing geometry values

        // Example pseudo-code (when painting API is ready):
        // let geometry = ctx.geometry(); // Get sliver geometry from context
        // ctx.canvas().rect(
        //     Rect::new(0.0, 0.0, geometry.cross_axis_extent, geometry.paint_extent),
        //     &Paint::stroke(self.outline_color, self.stroke_width)
        // );
        //
        // if self.show_scroll_offset {
        //     ctx.canvas().text(
        //         &format!("scroll_offset: {}", geometry.scroll_offset),
        //         Offset::new(10.0, 10.0),
        //         &TextStyle::default(),
        //         &Paint::fill(Color::BLACK)
        //     );
        // }
    }
}

// ============================================================================
// Example 2: Layout Override - Sliver Inset Padding
// ============================================================================

/// A proxy that adds inset padding to a sliver, reducing its cross-axis extent.
///
/// Unlike RenderSliverPadding which adds spacing around the sliver,
/// this proxy constrains the child to a smaller cross-axis extent,
/// creating an inset effect.
#[derive(Debug)]
pub struct RenderSliverInset {
    /// Inset from left edge
    pub left: f32,
    /// Inset from right edge
    pub right: f32,
    /// Cached geometry from last layout
    cached_geometry: Option<SliverGeometry>,
}

impl RenderSliverInset {
    /// Create a new inset with symmetric horizontal insets
    pub fn symmetric(horizontal: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            cached_geometry: None,
        }
    }

    /// Create a new inset with asymmetric insets
    pub fn new(left: f32, right: f32) -> Self {
        Self {
            left,
            right,
            cached_geometry: None,
        }
    }

    /// Get total horizontal inset
    pub fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }
}

impl RenderSliverProxy for RenderSliverInset {
    // Layout: Override to reduce cross-axis extent
    fn proxy_layout<T>(
        &mut self,
        mut ctx: LayoutContext<'_, T, Single, SliverProtocol>,
    ) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;

        // Create modified constraints with reduced cross-axis extent
        let inset_total = self.horizontal_total();
        let inset_constraints = SliverConstraints {
            cross_axis_extent: (constraints.cross_axis_extent - inset_total).max(0.0),
            ..constraints
        };

        // Get child ID (Single arity means exactly one child)
        let child_id = ctx.children.single();

        // Layout child with inset constraints
        let child_geometry = ctx.layout_child(child_id, inset_constraints);

        // Return geometry with original cross-axis extent restored
        let geometry = SliverGeometry {
            cross_axis_extent: constraints.cross_axis_extent,
            ..child_geometry
        };

        // Cache for painting
        self.cached_geometry = Some(geometry);

        geometry
    }

    // Paint: Offset child by left inset
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // TODO: When painting API supports transforms:
        // ctx.canvas().save();
        // ctx.canvas().translate(self.left, 0.0);
        ctx.proxy();
        // ctx.canvas().restore();
    }
}

// ============================================================================
// Example 3: Hit Testing Override - Sliver Hit Test Blocker
// ============================================================================

/// A proxy that conditionally blocks hit testing on its child.
///
/// Useful for temporarily disabling interaction with sliver content,
/// such as during animations or when content is loading.
#[derive(Debug)]
pub struct RenderSliverHitTestBlocker {
    /// Whether to block hit tests (true = block, false = allow)
    pub blocking: bool,
    /// Whether to block semantics as well
    pub block_semantics: bool,
}

impl RenderSliverHitTestBlocker {
    /// Create a new hit test blocker
    pub fn new(blocking: bool) -> Self {
        Self {
            blocking,
            block_semantics: false,
        }
    }

    /// Enable blocking
    pub fn enable(&mut self) {
        self.blocking = true;
    }

    /// Disable blocking
    pub fn disable(&mut self) {
        self.blocking = false;
    }

    /// Set whether to also block semantics
    pub fn set_block_semantics(&mut self, block: bool) {
        self.block_semantics = block;
    }
}

impl RenderSliverProxy for RenderSliverHitTestBlocker {
    // Layout: Use default (passes through)
    // Paint: Use default (paints child normally)

    // Hit testing: Override to block when enabled
    // Note: This would be implemented when hit testing API is available
    //
    // fn proxy_hit_test(&self, ctx: &HitTestContext) -> bool {
    //     if self.blocking {
    //         false  // Block all hit tests
    //     } else {
    //         ctx.proxy()  // Pass through to child
    //     }
    // }
}

// ============================================================================
// Example 4: Multi-Purpose Proxy - Sliver Container
// ============================================================================

/// A comprehensive proxy that can apply multiple effects.
///
/// This demonstrates how to combine multiple proxy features into one object,
/// similar to a Container widget but for slivers.
#[derive(Debug)]
pub struct RenderSliverContainer {
    /// Optional opacity (0.0 = transparent, 1.0 = opaque)
    pub opacity: Option<f32>,
    /// Optional cross-axis constraints
    pub max_cross_axis_extent: Option<f32>,
    /// Whether to ignore pointer events
    pub ignore_pointer: bool,
    /// Optional debug outline
    pub debug_outline: bool,
}

impl RenderSliverContainer {
    /// Create a new empty container
    pub fn new() -> Self {
        Self {
            opacity: None,
            max_cross_axis_extent: None,
            ignore_pointer: false,
            debug_outline: false,
        }
    }

    /// Set opacity
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = Some(opacity.clamp(0.0, 1.0));
        self
    }

    /// Set max cross-axis extent
    pub fn with_max_cross_axis_extent(mut self, extent: f32) -> Self {
        self.max_cross_axis_extent = Some(extent);
        self
    }

    /// Ignore pointer events
    pub fn ignore_pointer(mut self) -> Self {
        self.ignore_pointer = true;
        self
    }

    /// Enable debug outline
    pub fn with_debug_outline(mut self) -> Self {
        self.debug_outline = true;
        self
    }
}

impl Default for RenderSliverContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderSliverProxy for RenderSliverContainer {
    // Layout: Apply cross-axis constraints if specified
    fn proxy_layout<T>(
        &mut self,
        mut ctx: LayoutContext<'_, T, Single, SliverProtocol>,
    ) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;

        // Apply max cross-axis extent if specified
        let modified_constraints = if let Some(max_extent) = self.max_cross_axis_extent {
            SliverConstraints {
                cross_axis_extent: constraints.cross_axis_extent.min(max_extent),
                ..constraints
            }
        } else {
            constraints
        };

        // Get child and layout with (possibly modified) constraints
        let child_id = ctx.children.single();
        ctx.layout_child(child_id, modified_constraints)
    }

    // Paint: Apply opacity and debug outline if specified
    fn proxy_paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // TODO: When canvas API supports layers and effects:
        //
        // if let Some(opacity) = self.opacity {
        //     if opacity < 1.0 {
        //         ctx.canvas().save_layer_alpha(opacity);
        //     }
        // }

        // Paint child
        ctx.proxy();

        // if self.opacity.is_some() {
        //     ctx.canvas().restore();
        // }

        // if self.debug_outline {
        //     // Draw debug outline similar to RenderSliverDebugOutline
        // }
    }
}

// ============================================================================
// Usage Examples
// ============================================================================

#[cfg(test)]
mod usage_examples {
    use super::*;

    #[test]
    fn example_debug_outline() {
        // Create a debug outline proxy
        let debug = RenderSliverDebugOutline::new()
            .with_color(0.0, 1.0, 0.0, 0.7) // Green outline
            .with_stroke_width(3.0)
            .with_scroll_offset_indicator(true);

        // In a real app, you would wrap your sliver content:
        // SliverList(
        //     child: RenderSliverDebugOutline::new()
        //         .child(your_list_content)
        // )

        assert_eq!(debug.outline_color, [0.0, 1.0, 0.0, 0.7]);
        assert_eq!(debug.stroke_width, 3.0);
        assert!(debug.show_scroll_offset);
    }

    #[test]
    fn example_inset() {
        // Create symmetric inset
        let inset = RenderSliverInset::symmetric(20.0);
        assert_eq!(inset.horizontal_total(), 40.0);

        // Create asymmetric inset
        let inset = RenderSliverInset::new(10.0, 30.0);
        assert_eq!(inset.horizontal_total(), 40.0);
        assert_eq!(inset.left, 10.0);
        assert_eq!(inset.right, 30.0);

        // Usage:
        // SliverList(
        //     child: RenderSliverInset::symmetric(20.0)
        //         .child(your_list_content)
        // )
    }

    #[test]
    fn example_hit_test_blocker() {
        let mut blocker = RenderSliverHitTestBlocker::new(true);
        assert!(blocker.blocking);

        blocker.disable();
        assert!(!blocker.blocking);

        blocker.enable();
        assert!(blocker.blocking);

        blocker.set_block_semantics(true);
        assert!(blocker.block_semantics);

        // Usage during loading:
        // SliverList(
        //     child: RenderSliverHitTestBlocker::new(is_loading)
        //         .child(your_content)
        // )
    }

    #[test]
    fn example_container() {
        // Create a comprehensive container
        let container = RenderSliverContainer::new()
            .with_opacity(0.8)
            .with_max_cross_axis_extent(400.0)
            .ignore_pointer()
            .with_debug_outline();

        assert_eq!(container.opacity, Some(0.8));
        assert_eq!(container.max_cross_axis_extent, Some(400.0));
        assert!(container.ignore_pointer);
        assert!(container.debug_outline);

        // Usage:
        // SliverList(
        //     child: RenderSliverContainer::new()
        //         .with_opacity(0.8)
        //         .with_max_cross_axis_extent(400.0)
        //         .child(your_content)
        // )
    }

    #[test]
    fn example_composition() {
        // Proxies can be composed (nested)
        let debug = RenderSliverDebugOutline::new();
        let inset = RenderSliverInset::symmetric(16.0);
        let blocker = RenderSliverHitTestBlocker::new(false);

        // In a real app:
        // RenderSliverDebugOutline::new()
        //     .child(
        //         RenderSliverInset::symmetric(16.0)
        //             .child(
        //                 RenderSliverHitTestBlocker::new(is_disabled)
        //                     .child(your_actual_content)
        //             )
        //     )

        // Each proxy adds its behavior to the stack
        assert!(std::mem::size_of_val(&debug) < 100); // Lightweight
        assert!(std::mem::size_of_val(&inset) < 100);
        assert!(std::mem::size_of_val(&blocker) < 100);
    }
}

// ============================================================================
// Performance Notes
// ============================================================================

/// # Performance Characteristics
///
/// - **Zero-cost abstraction**: Proxy objects compile to direct function calls
/// - **No allocations**: All methods work with borrowed data
/// - **Inline-friendly**: Small methods marked #[inline] by compiler
/// - **Type-safe**: Arity checking at compile time (Single child guaranteed)
///
/// # When to Use RenderSliverProxy vs Full SliverRender
///
/// **Use RenderSliverProxy when:**
/// - ✅ You have exactly one child
/// - ✅ Layout constraints pass through mostly unchanged
/// - ✅ You're only modifying paint/hit-test/semantics
/// - ✅ You want minimal boilerplate
///
/// **Use full SliverRender<Single> when:**
/// - ❌ You need complex geometry transformations
/// - ❌ Layout logic is non-trivial
/// - ❌ You need to cache complex state between phases
/// - ❌ You need access to parent data or other advanced features

fn main() {
    println!("RenderSliverProxy Custom Implementations Examples");
    println!("==================================================");
    println!();
    println!("This file demonstrates 4 custom proxy implementations:");
    println!("1. RenderSliverDebugOutline - Visual debugging");
    println!("2. RenderSliverInset - Layout constraint modification");
    println!("3. RenderSliverHitTestBlocker - Interaction control");
    println!("4. RenderSliverContainer - Multi-purpose wrapper");
    println!();
    println!(
        "Run `cargo test -p flui_rendering --example sliver_proxy_custom` to see usage examples."
    );
}
