//! FilterLayer - applies color filters and transformations to child layers
//!
//! This module provides color manipulation filters similar to CSS filters,
//! including brightness, contrast, saturation, hue rotation, grayscale, sepia,
//! and custom color matrix transforms.

use crate::layer::{base_single_child::SingleChildLayerBase, BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::events::{Event, HitTestResult};
use flui_types::painting::effects::{ColorFilter as EffectColorFilter, ColorMatrix};
use flui_types::{Offset, Rect};

/// A filter layer that applies color transformations
pub struct FilterLayer {
    /// Base single-child layer functionality
    base: SingleChildLayerBase,

    /// Filters to apply (in order)
    filters: Vec<EffectColorFilter>,
}

impl FilterLayer {
    /// Create a new filter layer with a child
    ///
    /// # Arguments
    ///
    /// * `child` - Child layer to filter
    #[must_use]
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            base: SingleChildLayerBase::new(child),
            filters: Vec::new(),
        }
    }

    /// Set a single filter (replaces existing)
    #[must_use]
    pub fn with_filter(mut self, filter: EffectColorFilter) -> Self {
        self.filters = vec![filter];
        self
    }

    /// Set multiple filters (replaces existing)
    #[must_use]
    pub fn with_filters(mut self, filters: Vec<EffectColorFilter>) -> Self {
        self.filters = filters;
        self
    }

    /// Add a filter to the existing list
    pub fn add_filter(&mut self, filter: EffectColorFilter) {
        self.filters.push(filter);
        self.mark_needs_paint();
    }

    /// Get the filters
    pub fn filters(&self) -> &[EffectColorFilter] {
        &self.filters
    }

    /// Get the child layer
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.base.child()
    }

    /// Set the child layer
    pub fn set_child(&mut self, child: BoxedLayer) {
        self.base.set_child(child);
        self.mark_needs_paint();
    }

    /// Convert filter to color matrix
    #[allow(dead_code)]
    fn filter_to_matrix(&self, filter: &EffectColorFilter) -> ColorMatrix {
        match filter {
            EffectColorFilter::Brightness(amount) => ColorMatrix::brightness(*amount),
            EffectColorFilter::Contrast(amount) => ColorMatrix::contrast(*amount),
            EffectColorFilter::Saturation(amount) => ColorMatrix::saturation(*amount),
            EffectColorFilter::HueRotate(degrees) => ColorMatrix::hue_rotate(*degrees),
            EffectColorFilter::Grayscale(amount) => {
                // Lerp between identity and grayscale
                let gray = ColorMatrix::grayscale();
                let ident = ColorMatrix::identity();
                self.lerp_matrix(&ident, &gray, *amount)
            }
            EffectColorFilter::Sepia(amount) => {
                // Lerp between identity and sepia
                let sepia = ColorMatrix::sepia();
                let ident = ColorMatrix::identity();
                self.lerp_matrix(&ident, &sepia, *amount)
            }
            EffectColorFilter::Invert(amount) => {
                // Lerp between identity and invert
                let invert = ColorMatrix::invert();
                let ident = ColorMatrix::identity();
                self.lerp_matrix(&ident, &invert, *amount)
            }
            EffectColorFilter::Opacity(amount) => {
                let mut matrix = ColorMatrix::identity();
                matrix.values[18] = *amount; // Scale alpha channel
                matrix
            }
            EffectColorFilter::Matrix(matrix) => matrix.clone(),
        }
    }

    /// Linear interpolate between two matrices
    #[allow(dead_code)]
    fn lerp_matrix(&self, a: &ColorMatrix, b: &ColorMatrix, t: f32) -> ColorMatrix {
        let t = t.clamp(0.0, 1.0);
        let mut result = ColorMatrix::identity();

        for i in 0..20 {
            result.values[i] = a.values[i] * (1.0 - t) + b.values[i] * t;
        }

        result
    }

    /// Combine all filters into a single color matrix
    #[allow(dead_code)]
    fn combined_matrix(&self) -> ColorMatrix {
        if self.filters.is_empty() {
            return ColorMatrix::identity();
        }

        let mut result = self.filter_to_matrix(&self.filters[0]);

        for filter in &self.filters[1..] {
            let matrix = self.filter_to_matrix(filter);
            result = result.multiply(&matrix);
        }

        result
    }
}

impl Layer for FilterLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        let Some(child) = self.base.child() else {
            return;
        };

        if self.filters.is_empty() {
            // No filters, just paint child
            child.paint(painter);
            return;
        }

        // Use save_layer to render child to offscreen buffer for filtering
        // Note: Full color filter implementation requires GPU backend with shader support
        let child_bounds = child.bounds();
        let paint = crate::painter::Paint::default(); // TODO: Apply color matrix via paint/shader

        painter.save_layer(child_bounds, &paint);

        // In a full GPU backend implementation:
        // 1. save_layer() creates offscreen render target
        // 2. Child is rendered to the offscreen buffer
        // 3. Color matrix shader is applied to the buffer
        // 4. restore() composites the filtered result back
        //
        // For non-GPU backends, child is painted normally without filter effect

        child.paint(painter);

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        self.base.child_bounds()
    }

    fn is_visible(&self) -> bool {
        self.base.is_child_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.base.child_hit_test(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.child_handle_event(event)
    }

    fn dispose(&mut self) {
        self.base.dispose_child();
    }

    fn is_disposed(&self) -> bool {
        self.base.is_disposed()
    }

    fn mark_needs_paint(&mut self) {
        if let Some(child) = self.base.child_mut() {
            child.mark_needs_paint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_matrix_identity() {
        let matrix = ColorMatrix::identity();
        let color = [1.0, 0.5, 0.0, 1.0];
        let result = matrix.apply(color);

        assert!((result[0] - 1.0).abs() < 0.001);
        assert!((result[1] - 0.5).abs() < 0.001);
        assert!((result[2] - 0.0).abs() < 0.001);
        assert!((result[3] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_color_matrix_grayscale() {
        let matrix = ColorMatrix::grayscale();
        let color = [1.0, 0.0, 0.0, 1.0]; // Pure red
        let result = matrix.apply(color);

        // All RGB channels should be equal (grayscale)
        assert!((result[0] - result[1]).abs() < 0.001);
        assert!((result[1] - result[2]).abs() < 0.001);
    }

    #[test]
    fn test_color_matrix_brightness() {
        let matrix = ColorMatrix::brightness(0.5);
        let color = [0.5, 0.5, 0.5, 1.0];
        let result = matrix.apply(color);

        // All channels should be brighter
        assert!(result[0] > 0.5);
        assert!(result[1] > 0.5);
        assert!(result[2] > 0.5);
    }

    #[test]
    fn test_filter_layer_empty() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let filter = FilterLayer::new(child);

        assert!(filter.filters().is_empty());
    }

    #[test]
    fn test_filter_layer_single() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let filter = FilterLayer::new(child).with_filter(EffectColorFilter::Brightness(0.2));

        assert_eq!(filter.filters().len(), 1);
    }

    #[test]
    fn test_filter_layer_multiple() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let filter = FilterLayer::new(child).with_filters(vec![
            EffectColorFilter::Contrast(1.2),
            EffectColorFilter::Saturation(1.5),
        ]);

        assert_eq!(filter.filters().len(), 2);
    }
}
