//! RenderOverflowIndicator - Debug-only overflow visualization
//!
//! Shows visual indicators when content overflows in debug mode.
//! Zero cost in release builds - completely compiled out.
//!
//! # Usage
//!
//! Any RenderObject can call `paint_overflow_indicators()` helper:
//!
//! ```rust,ignore
//! #[cfg(debug_assertions)]
//! if overflow_detected {
//!     paint_overflow_indicators(
//!         &mut container,
//!         overflow_h,
//!         overflow_v,
//!         container_size,
//!         offset
//!     );
//! }
//! ```

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
#[cfg(debug_assertions)]
#[cfg(debug_assertions)]
#[cfg(debug_assertions)]
#[cfg(debug_assertions)]
#[cfg(debug_assertions)]
#[cfg(debug_assertions)]
use flui_types::{Color, Offset, Rect, Size};

/// Paint overflow indicator with diagonal stripes (debug mode only)
///
/// This creates a Flutter-style overflow indicator with:
/// - 45° diagonal red/yellow warning stripes (like warning tape)
/// - Red border around the overflow area
/// - Clipped to show only where content actually overflows
///
/// # Arguments
/// * `container` - The container layer to add indicators to
/// * `overflow_h` - Horizontal overflow in pixels (0.0 if none)
/// * `overflow_v` - Vertical overflow in pixels (0.0 if none)
/// * `container_size` - Size of the container
/// * `offset` - Offset of the container in screen coordinates
///
/// # Example
/// ```rust,ignore
/// fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
///     let mut container = pool::acquire_container();
///     // ... paint children ...
///
///     #[cfg(debug_assertions)]
///     if self.overflow_pixels > 0.0 {
///         paint_overflow_indicators(&mut container, self.overflow_h, self.overflow_v, self.size, offset);
///     }
///
///     Box::new(container)
/// }
/// ```
#[cfg(debug_assertions)]
pub fn paint_overflow_indicators(
    container: &mut flui_engine::layer::ContainerLayer,
    overflow_h: f32,
    overflow_v: f32,
    container_size: Size,
    offset: Offset,
) {
    // Early exit if no overflow
    if overflow_h <= 0.0 && overflow_v <= 0.0 {
        return;
    }

    // Create indicator picture layer
    let mut indicator = pool::acquire_picture();

    // Paint overflow region(s) with diagonal stripes
    if overflow_h > 0.0 && overflow_v > 0.0 {
        // Both axes overflow - paint L-shaped region covering right and bottom
        paint_overflow_region(
            &mut indicator,
            overflow_h,
            overflow_v,
            container_size,
            offset,
        );
    } else if overflow_h > 0.0 {
        // Horizontal overflow only - paint right side
        let overflow_rect = Rect::from_ltrb(
            offset.dx + container_size.width,
            offset.dy,
            offset.dx + container_size.width + overflow_h,
            offset.dy + container_size.height,
        );
        paint_diagonal_stripes(&mut indicator, overflow_rect);
        paint_border(&mut indicator, overflow_rect);
    } else {
        // Vertical overflow only - paint bottom
        let overflow_rect = Rect::from_ltrb(
            offset.dx,
            offset.dy + container_size.height,
            offset.dx + container_size.width,
            offset.dy + container_size.height + overflow_v,
        );
        paint_diagonal_stripes(&mut indicator, overflow_rect);
        paint_border(&mut indicator, overflow_rect);
    }

    // Add indicator on top of children (clipped by container)
    container.add_child(Box::new(indicator));
}

/// Paint overflow region when both axes overflow (L-shaped area)
#[cfg(debug_assertions)]
fn paint_overflow_region(
    picture: &mut flui_engine::layer::PictureLayer,
    overflow_h: f32,
    overflow_v: f32,
    container_size: Size,
    offset: Offset,
) {
    // Right side overflow (vertical strip)
    let right_rect = Rect::from_ltrb(
        offset.dx + container_size.width,
        offset.dy,
        offset.dx + container_size.width + overflow_h,
        offset.dy + container_size.height,
    );
    paint_diagonal_stripes(picture, right_rect);
    paint_border(picture, right_rect);

    // Bottom overflow (horizontal strip)
    let bottom_rect = Rect::from_ltrb(
        offset.dx,
        offset.dy + container_size.height,
        offset.dx + container_size.width,
        offset.dy + container_size.height + overflow_v,
    );
    paint_diagonal_stripes(picture, bottom_rect);
    paint_border(picture, bottom_rect);

    // Corner overflow (where both meet)
    let corner_rect = Rect::from_ltrb(
        offset.dx + container_size.width,
        offset.dy + container_size.height,
        offset.dx + container_size.width + overflow_h,
        offset.dy + container_size.height + overflow_v,
    );
    paint_diagonal_stripes(picture, corner_rect);
    paint_border(picture, corner_rect);
}

/// Paint 45° diagonal stripes (warning tape pattern)
#[cfg(debug_assertions)]
fn paint_diagonal_stripes(picture: &mut flui_engine::layer::PictureLayer, rect: Rect) {
    use flui_types::Point;

    const STRIPE_WIDTH: f32 = 6.0; // Width of each stripe (adjusted for better density)
    const RED: Color = Color {
        r: 211,
        g: 47,
        b: 47,
        a: 255,
    };
    const YELLOW: Color = Color {
        r: 255,
        g: 193,
        b: 7,
        a: 255,
    };

    let paint_red = Paint::builder().color(RED).anti_alias(true).build();

    let paint_yellow = Paint::builder().color(YELLOW).anti_alias(true).build();

    let left = rect.min.x;
    let right = rect.max.x;
    let top = rect.min.y;
    let bottom = rect.max.y;
    let width = rect.width();
    let height = rect.height();

    // For 45° diagonal stripes, we need to cover from top-left to bottom-right
    let diagonal_span = width + height;

    // Start from far enough left to cover the entire rectangle
    let start_offset = -height;

    // Calculate how many stripes we need to cover the entire diagonal span
    let num_stripes = ((diagonal_span + height) / (STRIPE_WIDTH * 2.0)).ceil() as i32 + 2;

    // Draw diagonal stripes at 45° angle
    for i in 0..num_stripes {
        let stripe_offset = start_offset + (i as f32 * STRIPE_WIDTH * 2.0);
        let is_red = i % 2 == 0;
        let paint = if is_red { &paint_red } else { &paint_yellow };

        // Create a parallelogram for each stripe at 45°
        let points = vec![
            Point::new(left + stripe_offset, top),
            Point::new(left + stripe_offset + STRIPE_WIDTH, top),
            Point::new(right + stripe_offset + STRIPE_WIDTH, bottom),
            Point::new(right + stripe_offset, bottom),
        ];

        picture.add_command(DrawCommand::Polygon {
            points: std::sync::Arc::new(points),
            paint: paint.clone(),
        });
    }
}

/// Paint red border around overflow region
#[cfg(debug_assertions)]
fn paint_border(picture: &mut flui_engine::layer::PictureLayer, rect: Rect) {
    const BORDER_COLOR: Color = Color {
        r: 211,
        g: 47,
        b: 47,
        a: 255,
    };

    let border_paint = Paint::builder()
        .color(BORDER_COLOR)
        .stroke(flui_engine::Stroke::new(3.0)) // Thicker border for visibility
        .anti_alias(true)
        .build();

    picture.add_command(DrawCommand::Rect {
        rect,
        paint: border_paint,
    });
}

/// RenderObject that wraps content and adds overflow indicators
///
/// **Debug mode only** - completely removed in release builds.
///
/// Draws colored stripes on the edges where overflow occurs:
/// - Horizontal overflow: 8px amber stripe on right edge
/// - Vertical overflow: 8px amber stripe on bottom edge
///
/// # Architecture
///
/// This render object has arity Exact(1) and:
/// 1. Passes through layout to its child (no constraint changes)
/// 2. Wraps the child's layer during paint with indicator layers
///
/// # Usage
///
/// RenderFlex and other layout objects should wrap overflowing children:
///
/// ```rust,ignore
/// #[cfg(debug_assertions)]
/// if overflow_detected {
///     // Wrap child with indicator
///     let indicator = RenderOverflowIndicator::new(overflow_h, overflow_v, container_size);
///     // Use indicator as render object
/// }
/// ```
#[cfg(debug_assertions)]
#[derive(Debug, Clone)]
pub struct RenderOverflowIndicator {
    /// Horizontal overflow in pixels
    pub overflow_h: f32,
    /// Vertical overflow in pixels
    pub overflow_v: f32,
    /// Container size (for positioning indicators)
    pub container_size: Size,
}

#[cfg(debug_assertions)]
impl RenderOverflowIndicator {
    /// Create new overflow indicator
    ///
    /// # Arguments
    /// * `overflow_h` - Horizontal overflow in pixels (0.0 if none)
    /// * `overflow_v` - Vertical overflow in pixels (0.0 if none)
    /// * `container_size` - Size of the container
    pub fn new(overflow_h: f32, overflow_v: f32, container_size: Size) -> Self {
        Self {
            overflow_h: overflow_h.max(0.0),
            overflow_v: overflow_v.max(0.0),
            container_size,
        }
    }

    /// Paint overflow stripes on a picture layer
    fn paint_overflow_stripe(picture: &mut flui_engine::layer::PictureLayer, rect: Rect) {
        // Warning colors - bright and attention-grabbing
        const STRIPE_COLOR: Color = Color {
            r: 255,
            g: 193,
            b: 7,
            a: 220,
        }; // Amber/yellow
        const BORDER_COLOR: Color = Color {
            r: 211,
            g: 47,
            b: 47,
            a: 255,
        }; // Red

        // Fill stripe with warning color
        let fill_paint = Paint::builder()
            .color(STRIPE_COLOR)
            .anti_alias(true)
            .build();

        picture.add_command(DrawCommand::Rect {
            rect,
            paint: fill_paint,
        });

        // Add red border for emphasis
        let border_paint = Paint::builder()
            .color(BORDER_COLOR)
            .stroke(flui_engine::Stroke::new(2.0))
            .anti_alias(true)
            .build();

        picture.add_command(DrawCommand::Rect {
            rect,
            paint: border_paint,
        });
    }
}

#[cfg(debug_assertions)]
impl Render for RenderOverflowIndicator {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Pass through layout to child - no changes
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Paint child first
        let child_layer = tree.paint_child(child_id, offset);

        // Early exit if no overflow
        if self.overflow_h <= 0.0 && self.overflow_v <= 0.0 {
            return child_layer;
        }

        // Create container to hold content + indicators
        let mut container = pool::acquire_container();
        container.add_child(child_layer);

        // Create indicator layer
        let mut indicator = pool::acquire_picture();

        // Draw horizontal overflow indicator on right edge
        if self.overflow_h > 0.0 {
            let indicator_width = 8.0f32;
            let stripe_rect = Rect::from_ltrb(
                offset.dx + self.container_size.width - indicator_width,
                offset.dy,
                offset.dx + self.container_size.width,
                offset.dy + self.container_size.height,
            );
            Self::paint_overflow_stripe(&mut indicator, stripe_rect);
        }

        // Draw vertical overflow indicator on bottom edge
        if self.overflow_v > 0.0 {
            let indicator_height = 8.0f32;
            let stripe_rect = Rect::from_ltrb(
                offset.dx,
                offset.dy + self.container_size.height - indicator_height,
                offset.dx + self.container_size.width,
                offset.dy + self.container_size.height,
            );
            Self::paint_overflow_stripe(&mut indicator, stripe_rect);
        }

        // Add indicator on top of content
        container.add_child(Box::new(indicator));

        Box::new(container)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Default - update if needed
    }
}

#[cfg(test)]
#[cfg(debug_assertions)]
mod tests {
    use super::*;

    #[test]
    fn test_overflow_indicator_new() {
        let indicator = RenderOverflowIndicator::new(25.0, 0.0, Size::new(300.0, 200.0));
        assert_eq!(indicator.overflow_h, 25.0);
        assert_eq!(indicator.overflow_v, 0.0);
        assert_eq!(indicator.container_size, Size::new(300.0, 200.0));
    }

    #[test]
    fn test_overflow_indicator_negative_clamped() {
        let indicator = RenderOverflowIndicator::new(-10.0, -5.0, Size::new(100.0, 100.0));
        assert_eq!(indicator.overflow_h, 0.0);
        assert_eq!(indicator.overflow_v, 0.0);

        
    }
}
