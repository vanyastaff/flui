//! ShadowLayer - adds drop shadows and inner shadows to child layers
//!
//! This module provides shadow effects similar to CSS box-shadow, supporting:
//! - Drop shadows (rendered below content)
//! - Inner shadows (rendered above content)
//! - Multiple shadows
//! - Configurable offset, blur radius, spread radius, and color

use crate::layer::{BoxedLayer, Layer};
use crate::painter::{Paint, Painter};
use flui_types::styling::{BoxShadow, ShadowQuality};
use flui_types::{Event, HitTestResult, Offset, Rect};

/// A layer that renders shadows around its child.
///
/// Similar to CSS box-shadow, this layer can render multiple shadows
/// with configurable offset, blur, spread, and color. Supports both
/// drop shadows (below content) and inner shadows (above content).
///
/// # Examples
///
/// ```rust,ignore
/// use flui_engine::layer::{ShadowLayer, Shadow};
/// use flui_types::{Offset, Color};
///
/// // Single drop shadow
/// let shadow_layer = ShadowLayer::new(child)
///     .with_shadow(Shadow::new(
///         Offset::new(0.0, 4.0),
///         8.0,  // blur
///         0.0,  // spread
///         Color::rgba(0, 0, 0, 76),  // 30% opacity
///     ));
///
/// // Multiple shadows (like CSS)
/// let multi_shadow = ShadowLayer::new(child)
///     .with_shadows(vec![
///         Shadow::new(Offset::new(0.0, 2.0), 4.0, 0.0, Color::rgba(0, 0, 0, 51)),
///         Shadow::new(Offset::new(0.0, 4.0), 8.0, 0.0, Color::rgba(0, 0, 0, 26)),
///     ]);
/// ```
pub struct ShadowLayer {
    /// Child layer to render with shadows
    child: Option<BoxedLayer>,

    /// List of shadows to render (order matters: first shadow is on bottom)
    shadows: Vec<BoxShadow>,

    /// Shadow rendering quality
    quality: ShadowQuality,

    /// Cached bounds including shadow extent
    cached_bounds: Option<Rect>,

    /// Whether this layer has been disposed
    disposed: bool,
}

impl ShadowLayer {
    /// Create a new shadow layer with a child
    ///
    /// # Arguments
    ///
    /// * `child` - Child layer to render with shadows
    #[must_use]
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            child: Some(child),
            shadows: vec![BoxShadow::default()],
            quality: ShadowQuality::default(),
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Create a shadow layer with no initial shadows
    #[must_use]
    pub fn with_child(child: BoxedLayer) -> Self {
        Self {
            child: Some(child),
            shadows: Vec::new(),
            quality: ShadowQuality::default(),
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Set a single shadow (replaces existing shadows)
    #[must_use]
    pub fn with_shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadows = vec![shadow];
        self.cached_bounds = None;
        self
    }

    /// Set multiple shadows (replaces existing shadows)
    #[must_use]
    pub fn with_shadows(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.shadows = shadows;
        self.cached_bounds = None;
        self
    }

    /// Add a shadow to the existing list
    pub fn add_shadow(&mut self, shadow: BoxShadow) {
        self.shadows.push(shadow);
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Set shadow quality
    #[must_use]
    pub fn with_quality(mut self, quality: ShadowQuality) -> Self {
        self.quality = quality;
        self
    }

    /// Get the shadows
    pub fn shadows(&self) -> &[BoxShadow] {
        &self.shadows
    }

    /// Get the child layer
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.child.as_ref()
    }

    /// Set the child layer
    pub fn set_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Calculate the maximum shadow extent (how far shadows extend beyond content)
    fn calculate_shadow_extent(&self) -> (f32, f32, f32, f32) {
        let mut left = 0.0f32;
        let mut top = 0.0f32;
        let mut right = 0.0f32;
        let mut bottom = 0.0f32;

        for shadow in &self.shadows {
            // Shadow extent = offset + blur + spread
            let blur_extent = shadow.blur_radius;
            let spread = shadow.spread_radius.max(0.0);

            left = left.max(-shadow.offset.dx - blur_extent - spread);
            top = top.max(-shadow.offset.dy - blur_extent - spread);
            right = right.max(shadow.offset.dx + blur_extent + spread);
            bottom = bottom.max(shadow.offset.dy + blur_extent + spread);
        }

        (left, top, right, bottom)
    }

    /// Render a single shadow
    fn paint_shadow(&self, painter: &mut dyn Painter, shadow: &BoxShadow, child_bounds: Rect) {
        painter.save();

        // Apply shadow offset
        painter.translate(shadow.offset);

        // Calculate shadow bounds with spread
        let shadow_bounds = if shadow.spread_radius != 0.0 {
            let spread = shadow.spread_radius;
            Rect::from_xywh(
                child_bounds.left() - spread,
                child_bounds.top() - spread,
                child_bounds.width() + spread * 2.0,
                child_bounds.height() + spread * 2.0,
            )
        } else {
            child_bounds
        };

        // Convert Color to Paint color array [f32; 4]
        let color_array = [
            shadow.color.r as f32 / 255.0,
            shadow.color.g as f32 / 255.0,
            shadow.color.b as f32 / 255.0,
            shadow.color.a as f32 / 255.0,
        ];

        // Render shadow based on blur radius
        if shadow.blur_radius > 0.0 {
            // Simple blur simulation with multiple passes
            let blur_steps = match self.quality {
                ShadowQuality::Low => 1,
                ShadowQuality::Medium => 3,
                ShadowQuality::High => 5,
            };

            let step_size = shadow.blur_radius / blur_steps as f32;
            let step_alpha = (shadow.color.a as f32 / 255.0) / blur_steps as f32;

            for i in 0..blur_steps {
                let offset = i as f32 * step_size - shadow.blur_radius / 2.0;
                let inflate_amount = offset.abs();

                let blur_bounds = Rect::from_xywh(
                    shadow_bounds.left() - inflate_amount,
                    shadow_bounds.top() - inflate_amount,
                    shadow_bounds.width() + inflate_amount * 2.0,
                    shadow_bounds.height() + inflate_amount * 2.0,
                );

                let blur_paint = Paint {
                    color: [color_array[0], color_array[1], color_array[2], step_alpha],
                    stroke_width: 0.0,
                    anti_alias: true,
                };

                painter.rect(blur_bounds, &blur_paint);
            }
        } else {
            // Hard-edged shadow (no blur)
            let shadow_paint = Paint {
                color: color_array,
                stroke_width: 0.0,
                anti_alias: true,
            };
            painter.rect(shadow_bounds, &shadow_paint);
        }

        painter.restore();
    }
}

impl Layer for ShadowLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot paint disposed ShadowLayer");
        }

        let Some(child) = &self.child else {
            return;
        };

        painter.save();

        // Get child bounds for shadow sizing
        let child_bounds = child.bounds();

        // Separate shadows into drop shadows and inner shadows
        let drop_shadows: Vec<_> = self.shadows.iter().filter(|s| !s.inset).collect();
        let inner_shadows: Vec<_> = self.shadows.iter().filter(|s| s.inset).collect();

        // 1. Render drop shadows (bottom to top)
        for shadow in drop_shadows {
            self.paint_shadow(painter, shadow, child_bounds);
        }

        // 2. Render child content
        child.paint(painter);

        // 3. Render inner shadows (with clipping)
        if !inner_shadows.is_empty() {
            painter.save();

            // Clip to child bounds for inner shadows
            painter.clip_rect(child_bounds);

            for shadow in inner_shadows {
                // Inner shadows are inverted (dark inside, light outside content)
                self.paint_shadow(painter, shadow, child_bounds);
            }

            painter.restore();
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        let child_bounds = self.child.as_ref().map_or(Rect::ZERO, |c| c.bounds());
        let (left, top, right, bottom) = self.calculate_shadow_extent();

        // Expand child bounds by shadow extent
        Rect::from_xywh(
            child_bounds.left() - left,
            child_bounds.top() - top,
            child_bounds.width() + left + right,
            child_bounds.height() + top + bottom,
        )
    }

    fn is_visible(&self) -> bool {
        !self.disposed && self.child.as_ref().is_some_and(|c| c.is_visible())
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.disposed {
            return false;
        }

        // Hit testing only considers child, not shadows
        self.child
            .as_ref()
            .is_some_and(|c| c.hit_test(position, result))
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        if self.disposed {
            return false;
        }

        self.child.as_mut().is_some_and(|c| c.handle_event(event))
    }

    fn dispose(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.dispose();
        }
        self.disposed = true;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn mark_needs_paint(&mut self) {
        if let Some(child) = &mut self.child {
            child.mark_needs_paint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Shadow;

    #[test]
    fn test_shadow_new() {
        let shadow = Shadow::new(Color::rgba(0, 0, 0, 76), Offset::new(2.0, 2.0), 4.0, 0.0);

        assert_eq!(shadow.offset, Offset::new(2.0, 2.0));
        assert_eq!(shadow.blur_radius, 4.0);
        assert_eq!(shadow.spread_radius, 0.0);
        assert!(!shadow.inset);
    }

    #[test]
    fn test_shadow_inner() {
        let shadow = Shadow::inner(Color::rgba(0, 0, 0, 128), Offset::new(0.0, 2.0), 4.0, 0.0);

        assert!(shadow.inset);
    }

    #[test]
    fn test_shadow_with_no_blur() {
        let shadow = Shadow::new(Color::rgba(0, 0, 0, 51), Offset::new(1.0, 1.0), 0.0, 0.0);

        assert_eq!(shadow.blur_radius, 0.0);
        assert_eq!(shadow.spread_radius, 0.0);
    }

    #[test]
    fn test_shadow_quality() {
        assert_eq!(ShadowQuality::default(), ShadowQuality::Medium);
    }
}
