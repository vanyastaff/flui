//! PhysicalModelLayer - Material Design elevation and shadows
//!
//! This module provides Material Design physical model effects with automatic
//! shadow generation based on elevation. Simulates the Material Design concept
//! of layered surfaces floating at different heights.

use crate::layer::{BoxedLayer, Layer};
use crate::painter::{Paint, Painter, RRect};
use flui_types::styling::{BorderRadius, Elevation, MaterialType, PhysicalShape};
use flui_types::{Color, Event, HitTestResult, Offset, Rect};

/// A layer that renders Material Design elevation with automatic shadows.
///
/// PhysicalModelLayer simulates a physical material surface elevated above
/// the canvas. It automatically calculates appropriate shadows based on the
/// elevation level following Material Design guidelines.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_engine::layer::{PhysicalModelLayer, Elevation};
/// use flui_types::styling::PhysicalShape;
/// use flui_types::{Color, BorderRadius};
///
/// // Card with elevation
/// let card = PhysicalModelLayer::new(child)
///     .with_elevation(Elevation::LEVEL_2)
///     .with_color(Color::WHITE)
///     .with_shape(PhysicalShape::Rectangle)
///     .with_border_radius(BorderRadius::circular(8.0));
///
/// // Floating action button
/// let fab = PhysicalModelLayer::new(child)
///     .with_elevation(Elevation::LEVEL_3)
///     .with_color(Color::rgb(33, 150, 243))
///     .with_shape(PhysicalShape::Circle);
/// ```
pub struct PhysicalModelLayer {
    /// Child layer to render on the elevated surface
    child: Option<BoxedLayer>,

    /// Shape of the physical model
    shape: PhysicalShape,

    /// Material type (affects rendering characteristics)
    material_type: MaterialType,

    /// Elevation in density-independent pixels (dp)
    elevation: f32,

    /// Background color of the material surface
    color: Color,

    /// Shadow color (typically semi-transparent black)
    shadow_color: Color,

    /// Border radius (for Rectangle shape only)
    border_radius: BorderRadius,

    /// Cached bounds including shadow extent
    cached_bounds: Option<Rect>,

    /// Whether this layer has been disposed
    disposed: bool,
}

impl PhysicalModelLayer {
    /// Create a new physical model layer with default settings.
    ///
    /// # Arguments
    ///
    /// * `child` - Child layer to render on the elevated surface
    #[must_use]
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            child: Some(child),
            shape: PhysicalShape::default(),
            material_type: MaterialType::default(),
            elevation: Elevation::LEVEL_1,
            color: Color::WHITE,
            shadow_color: Color::rgba(0, 0, 0, 76), // 30% opacity
            border_radius: BorderRadius::ZERO,
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Set the elevation level.
    #[must_use]
    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = elevation.clamp(0.0, Elevation::MAX);
        self.cached_bounds = None;
        self
    }

    /// Set the shape.
    #[must_use]
    pub fn with_shape(mut self, shape: PhysicalShape) -> Self {
        self.shape = shape;
        self
    }

    /// Set the material type.
    #[must_use]
    pub fn with_material_type(mut self, material_type: MaterialType) -> Self {
        self.material_type = material_type;
        self
    }

    /// Set the background color.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Set the shadow color.
    #[must_use]
    pub fn with_shadow_color(mut self, color: Color) -> Self {
        self.shadow_color = color;
        self
    }

    /// Set the border radius (Rectangle shape only).
    #[must_use]
    pub fn with_border_radius(mut self, radius: BorderRadius) -> Self {
        self.border_radius = radius;
        self
    }

    /// Update the elevation.
    pub fn set_elevation(&mut self, elevation: f32) {
        self.elevation = elevation.clamp(0.0, Elevation::MAX);
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Update the shape.
    pub fn set_shape(&mut self, shape: PhysicalShape) {
        self.shape = shape;
        self.mark_needs_paint();
    }

    /// Update the color.
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
        self.mark_needs_paint();
    }

    /// Get the current elevation.
    pub fn elevation(&self) -> f32 {
        self.elevation
    }

    /// Get the current shape.
    pub fn shape(&self) -> PhysicalShape {
        self.shape
    }

    /// Get the current color.
    pub fn color(&self) -> Color {
        self.color
    }

    /// Get the child layer.
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.child.as_ref()
    }

    /// Set the child layer.
    pub fn set_child(&mut self, child: BoxedLayer) {
        self.child = Some(child);
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Calculate shadow extent based on elevation.
    fn calculate_shadow_extent(&self) -> (f32, f32, f32, f32) {
        if self.elevation == 0.0 {
            return (0.0, 0.0, 0.0, 0.0);
        }

        let offset = Elevation::shadow_offset(self.elevation);
        let blur = Elevation::blur_radius(self.elevation);

        // Shadow extent includes offset + blur radius
        let left = (-offset.dx + blur).max(0.0);
        let top = (-offset.dy + blur).max(0.0);
        let right = (offset.dx + blur).max(0.0);
        let bottom = (offset.dy + blur).max(0.0);

        (left, top, right, bottom)
    }

    /// Render the material shadow.
    fn paint_shadow(&self, painter: &mut dyn Painter, bounds: Rect) {
        if self.elevation == 0.0 {
            return;
        }

        painter.save();

        let offset = Elevation::shadow_offset(self.elevation);
        let blur = Elevation::blur_radius(self.elevation);
        let spread = Elevation::spread_radius(self.elevation);

        // Apply shadow offset
        painter.translate(offset);

        // Calculate shadow bounds with spread
        let shadow_bounds = if spread != 0.0 {
            Rect::from_xywh(
                bounds.left() - spread,
                bounds.top() - spread,
                bounds.width() + spread * 2.0,
                bounds.height() + spread * 2.0,
            )
        } else {
            bounds
        };

        // Convert shadow color to Paint color array
        let shadow_color_array = [
            self.shadow_color.r as f32 / 255.0,
            self.shadow_color.g as f32 / 255.0,
            self.shadow_color.b as f32 / 255.0,
            self.shadow_color.a as f32 / 255.0,
        ];

        // Render shadow with blur simulation
        if blur > 0.0 {
            // Multi-pass blur simulation
            let blur_steps = 3;
            let step_alpha = (self.shadow_color.a as f32 / 255.0) / blur_steps as f32;

            for i in 0..blur_steps {
                let blur_offset = (i as f32 / blur_steps as f32 - 0.5) * blur;
                let inflate = blur_offset.abs();

                let blur_bounds = Rect::from_xywh(
                    shadow_bounds.left() - inflate,
                    shadow_bounds.top() - inflate,
                    shadow_bounds.width() + inflate * 2.0,
                    shadow_bounds.height() + inflate * 2.0,
                );

                let blur_paint = Paint {
                    color: [
                        shadow_color_array[0],
                        shadow_color_array[1],
                        shadow_color_array[2],
                        step_alpha,
                    ],
                    stroke_width: 0.0,
                    anti_alias: true,
                };

                match self.shape {
                    PhysicalShape::Rectangle => {
                        if self.border_radius != BorderRadius::ZERO {
                            // Rounded rectangle shadow (use top-left radius)
                            let rrect = RRect {
                                rect: blur_bounds,
                                corner_radius: self.border_radius.top_left.x,
                            };
                            painter.rrect(rrect, &blur_paint);
                        } else {
                            // Rectangle shadow
                            painter.rect(blur_bounds, &blur_paint);
                        }
                    }
                    PhysicalShape::Circle => {
                        // Ellipse shadow (oval bounds)
                        let center = blur_bounds.center();
                        painter.ellipse(
                            center,
                            blur_bounds.width() / 2.0,
                            blur_bounds.height() / 2.0,
                            &blur_paint,
                        );
                    }
                }
            }
        } else {
            // Hard-edged shadow (no blur)
            let shadow_paint = Paint {
                color: shadow_color_array,
                stroke_width: 0.0,
                anti_alias: true,
            };

            match self.shape {
                PhysicalShape::Rectangle => {
                    if self.border_radius != BorderRadius::ZERO {
                        let rrect = RRect {
                            rect: shadow_bounds,
                            corner_radius: self.border_radius.top_left.x,
                        };
                        painter.rrect(rrect, &shadow_paint);
                    } else {
                        painter.rect(shadow_bounds, &shadow_paint);
                    }
                }
                PhysicalShape::Circle => {
                    let center = shadow_bounds.center();
                    painter.ellipse(
                        center,
                        shadow_bounds.width() / 2.0,
                        shadow_bounds.height() / 2.0,
                        &shadow_paint,
                    );
                }
            }
        }

        painter.restore();
    }

    /// Render the material surface.
    fn paint_surface(&self, painter: &mut dyn Painter, bounds: Rect) {
        // Skip rendering if transparent material
        if self.material_type == MaterialType::Transparency {
            return;
        }

        let surface_paint = Paint {
            color: [
                self.color.r as f32 / 255.0,
                self.color.g as f32 / 255.0,
                self.color.b as f32 / 255.0,
                self.color.a as f32 / 255.0,
            ],
            stroke_width: 0.0,
            anti_alias: true,
        };

        match self.shape {
            PhysicalShape::Rectangle => {
                if self.border_radius != BorderRadius::ZERO {
                    let rrect = RRect {
                        rect: bounds,
                        corner_radius: self.border_radius.top_left.x,
                    };
                    painter.rrect(rrect, &surface_paint);
                } else {
                    painter.rect(bounds, &surface_paint);
                }
            }
            PhysicalShape::Circle => {
                let center = bounds.center();
                painter.ellipse(
                    center,
                    bounds.width() / 2.0,
                    bounds.height() / 2.0,
                    &surface_paint,
                );
            }
        }
    }

    /// Apply clipping for the child content.
    fn apply_clip(&self, painter: &mut dyn Painter, bounds: Rect) {
        match self.shape {
            PhysicalShape::Rectangle => {
                if self.border_radius != BorderRadius::ZERO {
                    let rrect = RRect {
                        rect: bounds,
                        corner_radius: self.border_radius.top_left.x,
                    };
                    painter.clip_rrect(rrect);
                } else {
                    painter.clip_rect(bounds);
                }
            }
            PhysicalShape::Circle => {
                painter.clip_oval(bounds);
            }
        }
    }
}

impl Layer for PhysicalModelLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot paint disposed PhysicalModelLayer");
        }

        let Some(child) = &self.child else {
            return;
        };

        painter.save();

        let child_bounds = child.bounds();

        // 1. Render shadow (if elevation > 0)
        self.paint_shadow(painter, child_bounds);

        // 2. Render material surface
        self.paint_surface(painter, child_bounds);

        // 3. Clip and render child content
        painter.save();
        self.apply_clip(painter, child_bounds);
        child.paint(painter);
        painter.restore();

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

        // Hit testing considers the material surface, not the shadow
        let Some(child) = &self.child else {
            return false;
        };

        let bounds = child.bounds();

        // Check if position is within the material shape
        let in_shape = match self.shape {
            PhysicalShape::Rectangle => {
                if self.border_radius != BorderRadius::ZERO {
                    // Rounded rectangle hit test (simplified)
                    bounds.contains(position)
                } else {
                    bounds.contains(position)
                }
            }
            PhysicalShape::Circle => {
                // Oval hit test
                let center = bounds.center();
                let dx = (position.dx - center.x) / (bounds.width() / 2.0);
                let dy = (position.dy - center.y) / (bounds.height() / 2.0);
                dx * dx + dy * dy <= 1.0
            }
        };

        if in_shape {
            child.hit_test(position, result)
        } else {
            false
        }
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

    #[test]
    fn test_physical_model_new() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let layer = PhysicalModelLayer::new(child);

        assert!(!layer.is_disposed());
        assert!(layer.child().is_some());
        assert_eq!(layer.elevation(), Elevation::LEVEL_1);
        assert_eq!(layer.shape(), PhysicalShape::Rectangle);
        assert_eq!(layer.color(), Color::WHITE);
    }

    #[test]
    fn test_physical_model_with_elevation() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let layer = PhysicalModelLayer::new(child).with_elevation(Elevation::LEVEL_3);

        assert_eq!(layer.elevation(), Elevation::LEVEL_3);
    }

    #[test]
    fn test_physical_model_with_shape() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let layer = PhysicalModelLayer::new(child).with_shape(PhysicalShape::Circle);

        assert_eq!(layer.shape(), PhysicalShape::Circle);
    }

    #[test]
    fn test_physical_model_with_color() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let blue = Color::rgb(33, 150, 243);
        let layer = PhysicalModelLayer::new(child).with_color(blue);

        assert_eq!(layer.color(), blue);
    }

    #[test]
    fn test_physical_model_shadow_extent() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let layer = PhysicalModelLayer::new(child).with_elevation(Elevation::LEVEL_4);

        let (left, top, right, bottom) = layer.calculate_shadow_extent();

        // Shadow should have some extent for non-zero elevation
        assert!(left >= 0.0);
        assert!(top >= 0.0);
        assert!(right > 0.0);
        assert!(bottom > 0.0);
    }

    #[test]
    fn test_physical_model_no_shadow_at_zero_elevation() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let layer = PhysicalModelLayer::new(child).with_elevation(0.0);

        let (left, top, right, bottom) = layer.calculate_shadow_extent();

        assert_eq!(left, 0.0);
        assert_eq!(top, 0.0);
        assert_eq!(right, 0.0);
        assert_eq!(bottom, 0.0);
    }

    #[test]
    fn test_physical_model_dispose() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let mut layer = PhysicalModelLayer::new(child);

        assert!(!layer.is_disposed());
        layer.dispose();
        assert!(layer.is_disposed());
        assert!(layer.child().is_none());
    }

    #[test]
    fn test_physical_model_clamps_elevation() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;

        // Test max clamping
        let layer_high = PhysicalModelLayer::new(child).with_elevation(100.0);
        assert_eq!(layer_high.elevation(), Elevation::MAX);

        // Test min clamping
        let child2 = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let layer_low = PhysicalModelLayer::new(child2).with_elevation(-5.0);
        assert_eq!(layer_low.elevation(), 0.0);
    }
}
