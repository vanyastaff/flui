//! RenderSliverToBoxAdapter - Adapts box protocol widgets to sliver protocol
//!
//! Implements Flutter's adapter pattern for inserting regular box widgets into sliver
//! scroll contexts. Converts SliverConstraints to BoxConstraints, layouts child with box
//! protocol, then converts result back to SliverGeometry. Essential bridge for using
//! standard widgets (Container, Padding, Image, etc.) inside CustomScrollView.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverToBoxAdapter` | `RenderSliverToBoxAdapter` from `package:flutter/src/rendering/sliver.dart` |
//! | `child_constraints()` | Constraints conversion logic |
//! | `calculate_sliver_geometry()` | SliverGeometry calculation from box size |
//! | `child_size` | Cached box child size |
//! | `sliver_geometry` | Output SliverGeometry |
//!
//! # Protocol Conversion
//!
//! **Input (Sliver Protocol):** SliverConstraints
//! - `cross_axis_extent` - Width (vertical scroll) or height (horizontal scroll)
//! - `remaining_paint_extent` - Available main axis space in viewport
//! - `scroll_offset` - How far scrolled
//!
//! **Child (Box Protocol):** BoxConstraints
//! - `max_width` = sliver cross_axis_extent
//! - `max_height` = remaining_paint_extent (or infinite if unlimited)
//! - `min_width` = 0 (loose constraints)
//! - `min_height` = 0 (loose constraints)
//!
//! **Output (Sliver Protocol):** SliverGeometry
//! - `scroll_extent` = child main axis extent (height for vertical)
//! - `paint_extent` = visible portion in viewport
//! - `layout_extent` = extent for layout purposes
//!
//! # Layout Protocol
//!
//! 1. **Convert SliverConstraints to BoxConstraints**
//!    - cross_axis_extent → max_width
//!    - remaining_paint_extent → max_height
//!    - Loose constraints (min = 0)
//!
//! 2. **Layout box child**
//!    - Child uses box protocol (standard widgets)
//!    - Child returns Size
//!    - Cache child size
//!
//! 3. **Calculate SliverGeometry from child size**
//!    - scroll_extent = child main axis extent
//!    - paint_extent = visible portion (accounting for scroll offset)
//!    - Handle partially/fully scrolled off cases
//!
//! 4. **Return SliverGeometry**
//!    - Viewport uses this for scroll calculations
//!
//! # Paint Protocol
//!
//! 1. **Calculate child offset**
//!    - Account for scroll offset
//!    - Position child relative to viewport
//!
//! 2. **Paint box child**
//!    - Child paints using box protocol
//!    - Only paint if visible (paint_extent > 0)
//!
//! # Performance
//!
//! - **Layout**: O(1) + child box layout - simple protocol conversion
//! - **Paint**: O(1) + child box paint - pass-through painting
//! - **Memory**: 40 bytes (cached Size + SliverGeometry)
//!
//! # Use Cases
//!
//! - **Headers in scroll views**: Non-scrolling headers (Image, Container, etc.)
//! - **Spacers**: SizedBox for spacing in CustomScrollView
//! - **Mixed content**: Mix slivers (List, Grid) with box widgets (Padding, Divider)
//! - **Single box in sliver context**: One-off widgets in scrollable
//! - **Banner ads**: Fixed-size banners between scrolling content
//! - **Section headers**: Headers between sliver lists
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderSliverList**: List has many children with lazy loading, Adapter has single box child
//! - **vs RenderSliverPadding**: Padding wraps sliver child, Adapter wraps box child
//! - **vs RenderSliverFillViewport**: FillViewport sizes to viewport, Adapter uses child's natural size
//! - **vs RenderViewport**: Viewport contains slivers, Adapter IS a sliver wrapping box
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverToBoxAdapter;
//!
//! // Wrap a box widget for use in CustomScrollView
//! let adapter = RenderSliverToBoxAdapter::new();
//! // Child would be RenderContainer, RenderPadding, RenderImage, etc.
//!
//! // Typical use in CustomScrollView:
//! // - RenderSliverToBoxAdapter wrapping header Image
//! // - RenderSliverList for scrolling list items
//! // - RenderSliverToBoxAdapter wrapping footer Container
//! ```

use flui_rendering::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use flui_rendering::RenderResult;
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adapts box protocol widgets to sliver protocol.
///
/// Protocol adapter allowing standard box widgets (Container, Image, Padding, etc.)
/// to be used inside sliver scroll contexts (CustomScrollView). Converts SliverConstraints
/// to BoxConstraints for child layout, then converts child Size back to SliverGeometry
/// for viewport. Essential bridge between box and sliver protocols.
///
/// # Arity
///
/// `RuntimeArity` (Single) - Must have exactly one box protocol child.
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
/// Child uses box protocol.
///
/// # Pattern
///
/// **Protocol Adapter (Sliver → Box → Sliver)** - Converts sliver constraints to box
/// constraints, layouts box child, converts box size to sliver geometry, accounts for
/// scroll offset and viewport visibility.
///
/// # Use Cases
///
/// - **Fixed headers**: Image/Container headers in CustomScrollView
/// - **Spacers**: SizedBox spacing between slivers
/// - **Mixed content**: Combine slivers (lists/grids) with boxes (headers/footers)
/// - **Single widgets**: One-off widgets in scrollable contexts
/// - **Section headers**: Non-scrolling headers between lists
/// - **Banner ads**: Fixed banners between content sections
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverToBoxAdapter behavior:
/// - Converts SliverConstraints to loose BoxConstraints
/// - Child sized to natural size within sliver constraints
/// - SliverGeometry calculated from child size
/// - Handles scroll offset for visibility calculations
/// - paint_extent accounts for partially scrolled-off content
///
/// # Protocol Conversion Details
///
/// **Sliver to Box (Layout):**
/// ```text
/// SliverConstraints:
///   cross_axis_extent: 400px
///   remaining_paint_extent: 600px
///   scroll_offset: 0px
///
/// ↓ Converts to ↓
///
/// BoxConstraints:
///   min_width: 0px, max_width: 400px
///   min_height: 0px, max_height: 600px
/// ```
///
/// **Box to Sliver (Result):**
/// ```text
/// Child Size: 400×200
///
/// ↓ Converts to ↓
///
/// SliverGeometry:
///   scroll_extent: 200px (child height)
///   paint_extent: 200px (fully visible)
///   layout_extent: 200px
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverToBoxAdapter;
///
/// // Wrap box widget for CustomScrollView
/// let adapter = RenderSliverToBoxAdapter::new();
/// // Child: RenderImage (header), RenderContainer (banner), etc.
///
/// // Typical CustomScrollView structure:
/// // CustomScrollView {
/// //   slivers: [
/// //     SliverToBoxAdapter { child: Image },  ← Header
/// //     SliverList { ... },                    ← Scrolling list
/// //     SliverToBoxAdapter { child: Divider }, ← Separator
/// //     SliverGrid { ... },                    ← Scrolling grid
/// //   ]
/// // }
/// ```
#[derive(Debug)]
pub struct RenderSliverToBoxAdapter {
    // Layout cache
    child_size: Size,
    sliver_geometry: SliverGeometry,
}

impl RenderSliverToBoxAdapter {
    /// Create new sliver to box adapter
    pub fn new() -> Self {
        Self {
            child_size: Size::ZERO,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Convert sliver constraints to box constraints for child
    fn child_constraints(&self, sliver_constraints: &SliverConstraints) -> BoxConstraints {
        // The child can be as wide as the cross axis extent
        // and as tall as the remaining paint extent (or unlimited)
        let max_width = sliver_constraints.cross_axis_extent;
        let max_height = if sliver_constraints.has_infinite_paint_extent() {
            f32::INFINITY
        } else {
            sliver_constraints.remaining_paint_extent
        };

        BoxConstraints {
            min_width: 0.0,
            max_width,
            min_height: 0.0,
            max_height,
        }
    }

    /// Calculate sliver geometry from child size
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_size: Size,
    ) -> SliverGeometry {
        // The main axis extent of the child
        let child_extent = match constraints.axis_direction.axis() {
            Axis::Vertical => child_size.height,
            Axis::Horizontal => child_size.width,
        };

        // Calculate scroll extent and paint extent
        let scroll_extent = child_extent;
        let scroll_offset = constraints.scroll_offset;

        // Determine how much is actually painted
        let paint_extent = if scroll_offset >= scroll_extent {
            // Child is completely scrolled off
            0.0
        } else if scroll_offset + constraints.remaining_paint_extent >= scroll_extent {
            // Child is completely visible
            (scroll_extent - scroll_offset).max(0.0)
        } else {
            // Child is partially visible
            constraints.remaining_paint_extent
        };

        let paint_extent = paint_extent.min(constraints.remaining_paint_extent);

        SliverGeometry {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if scroll_extent > 0.0 {
                (paint_extent / scroll_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: scroll_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverToBoxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderSliverToBoxAdapter {}

impl RenderSliver<Single> for RenderSliverToBoxAdapter {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;

        // Convert sliver constraints to box constraints for child
        let box_constraints = self.child_constraints(&constraints);

        // Get child
        let child_id = *ctx.children.single();

        // Layout the box child with box constraints
        self.child_size = ctx.tree_mut().perform_layout(child_id, box_constraints)?;

        // Calculate and cache sliver geometry
        self.sliver_geometry = self.calculate_sliver_geometry(&constraints, self.child_size);
        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // Paint child if visible
        if self.sliver_geometry.visible {
            let child_id = *ctx.children.single();

            // Paint child at current offset
            if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset) {
                *ctx.canvas = child_canvas;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_to_box_adapter_new() {
        let adapter = RenderSliverToBoxAdapter::new();

        assert_eq!(adapter.child_size, Size::ZERO);
    }

    #[test]
    fn test_render_sliver_to_box_adapter_default() {
        let adapter = RenderSliverToBoxAdapter::default();

        assert_eq!(adapter.child_size, Size::ZERO);
    }

    #[test]
    fn test_child_constraints_finite() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let box_constraints = adapter.child_constraints(&sliver_constraints);

        assert_eq!(box_constraints.min_width, 0.0);
        assert_eq!(box_constraints.max_width, 400.0);
        assert_eq!(box_constraints.min_height, 0.0);
        assert_eq!(box_constraints.max_height, 600.0);
    }

    #[test]
    fn test_child_constraints_infinite() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: f32::INFINITY,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let box_constraints = adapter.child_constraints(&sliver_constraints);

        assert_eq!(box_constraints.max_width, 400.0);
        assert!(box_constraints.max_height.is_infinite());
    }

    #[test]
    fn test_calculate_sliver_geometry_fully_visible() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 200.0);
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Child is 200px tall, fully visible
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 200.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 50.0, // Scrolled 50px
            remaining_paint_extent: 100.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 200.0);
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Child is 200px tall, but only 100px viewport remains
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 100.0); // Clipped to remaining extent
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.5); // 100/200
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 300.0, // Scrolled past child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 200.0);
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Child is scrolled completely off
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_horizontal() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::LeftToRight,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 600.0,
            cross_axis_direction: AxisDirection::TopToBottom,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(300.0, 600.0);
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        // Horizontal scroll uses width as extent
        assert_eq!(geometry.scroll_extent, 300.0);
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_zero_child() {
        let adapter = RenderSliverToBoxAdapter::new();

        let sliver_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::ZERO;
        let geometry = adapter.calculate_sliver_geometry(&sliver_constraints, child_size);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }
}
