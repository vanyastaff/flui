//! BlurLayer - applies gaussian blur effect to child layers
//!
//! This module provides blur effects similar to CSS backdrop-filter and filter: blur(),
//! supporting various blur algorithms with configurable quality and radius.

use flui_types::{Rect, Offset, Event, HitTestResult};
use crate::layer::{Layer, BoxedLayer};
use crate::painter::Painter;
use flui_types::painting::effects::{BlurQuality, BlurMode};

/// A layer that applies gaussian blur to its child or backdrop.
///
/// Similar to CSS filter: blur() and backdrop-filter: blur(). Supports
/// configurable blur radius and quality levels.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_engine::layer::{BlurLayer, BlurMode, BlurQuality};
///
/// // Blur child content
/// let blur = BlurLayer::new(child)
///     .with_sigma(5.0)
///     .with_quality(BlurQuality::High);
///
/// // Backdrop blur (frosted glass effect)
/// let backdrop = BlurLayer::new(child)
///     .with_sigma(10.0)
///     .with_mode(BlurMode::Backdrop);
/// ```
pub struct BlurLayer {
    /// Child layer
    child: Option<BoxedLayer>,

    /// Blur radius (sigma for gaussian blur)
    sigma: f32,

    /// Blur quality/algorithm
    quality: BlurQuality,

    /// Blur mode (content or backdrop)
    mode: BlurMode,

    /// Tile mode for edges (true = clamp, false = transparent)
    tile_mode_clamp: bool,

    /// Cached bounds including blur extent
    cached_bounds: Option<Rect>,

    /// Whether this layer has been disposed
    disposed: bool,
}

impl BlurLayer {
    /// Create a new blur layer with a child
    ///
    /// # Arguments
    ///
    /// * `child` - Child layer to blur
    #[must_use]
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            child: Some(child),
            sigma: 5.0,
            quality: BlurQuality::default(),
            mode: BlurMode::default(),
            tile_mode_clamp: true,
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Set blur radius (sigma)
    ///
    /// Typical values: 0-20 (0 = no blur, 20 = very blurry)
    #[must_use]
    pub fn with_sigma(mut self, sigma: f32) -> Self {
        self.sigma = sigma.max(0.0);
        self.cached_bounds = None;
        self
    }

    /// Set blur quality
    #[must_use]
    pub fn with_quality(mut self, quality: BlurQuality) -> Self {
        self.quality = quality;
        self
    }

    /// Set blur mode (content or backdrop)
    #[must_use]
    pub fn with_mode(mut self, mode: BlurMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set tile mode for edges
    #[must_use]
    pub fn with_tile_mode_clamp(mut self, clamp: bool) -> Self {
        self.tile_mode_clamp = clamp;
        self
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

    /// Update blur sigma
    pub fn set_sigma(&mut self, sigma: f32) {
        self.sigma = sigma.max(0.0);
        self.cached_bounds = None;
        self.mark_needs_paint();
    }

    /// Calculate blur extent (how far blur extends beyond content)
    fn calculate_blur_extent(&self) -> f32 {
        // Gaussian blur typically extends about 3 sigma
        self.sigma * 3.0
    }

    /// Apply blur effect by rendering multiple passes
    fn apply_blur(&self, painter: &mut dyn Painter, bounds: Rect) {
        if self.sigma <= 0.0 {
            return;
        }

        // Number of passes based on quality
        let passes = match self.quality {
            BlurQuality::Low => 1,
            BlurQuality::Medium => 3,
            BlurQuality::High => 5,
        };

        // Box blur approximation of gaussian
        // Each pass reduces sigma by sqrt(passes)
        let pass_sigma = self.sigma / (passes as f32).sqrt();

        // For now, we simulate blur by rendering semi-transparent
        // expanded versions (proper blur requires offscreen rendering)
        painter.save();

        for i in 0..passes {
            let expansion = (i + 1) as f32 * pass_sigma;
            let _alpha = 1.0 / (passes as f32 * 2.0);

            let _expanded_bounds = Rect::from_xywh(
                bounds.left() - expansion,
                bounds.top() - expansion,
                bounds.width() + expansion * 2.0,
                bounds.height() + expansion * 2.0,
            );

            // Note: This is a simplified blur simulation
            // Production implementation would use proper gaussian convolution
            // or separable box blur filters
            if let Some(child) = &self.child {
                painter.save();
                painter.translate(Offset::new(-expansion, -expansion));

                // Apply reduced opacity for blur effect
                // (This is a placeholder - real blur needs offscreen rendering)
                child.paint(painter);

                painter.restore();
            }
        }

        painter.restore();
    }
}

impl Layer for BlurLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            panic!("Cannot paint disposed BlurLayer");
        }

        let Some(child) = &self.child else {
            return;
        };

        painter.save();

        match self.mode {
            BlurMode::Content => {
                // Apply blur to child content
                if self.sigma > 0.0 {
                    self.apply_blur(painter, child.bounds());
                } else {
                    // No blur, just paint child
                    child.paint(painter);
                }
            }
            BlurMode::Backdrop => {
                // Backdrop blur: blur what's behind the child
                // Note: This requires rendering the backdrop first,
                // applying blur, then rendering the child on top
                // For now, we render child with reduced opacity
                // (proper implementation needs compositor support)

                painter.save();
                // TODO: Implement proper backdrop blur with offscreen rendering
                child.paint(painter);
                painter.restore();
            }
        }

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        let child_bounds = self.child.as_ref().map_or(Rect::ZERO, |c| c.bounds());

        // Expand bounds by blur extent
        let extent = self.calculate_blur_extent();
        Rect::from_xywh(
            child_bounds.left() - extent,
            child_bounds.top() - extent,
            child_bounds.width() + extent * 2.0,
            child_bounds.height() + extent * 2.0,
        )
    }

    fn is_visible(&self) -> bool {
        !self.disposed && self.child.as_ref().is_some_and(|c| c.is_visible())
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.disposed {
            return false;
        }

        // Hit testing considers child, blur doesn't affect hit testing
        self.child.as_ref().is_some_and(|c| c.hit_test(position, result))
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
    fn test_blur_quality() {
        assert_eq!(BlurQuality::default(), BlurQuality::Medium);
    }

    #[test]
    fn test_blur_mode() {
        assert_eq!(BlurMode::default(), BlurMode::Content);
    }

    #[test]
    fn test_blur_extent() {
        let child = Box::new(crate::layer::picture::PictureLayer::new()) as BoxedLayer;
        let blur = BlurLayer::new(child).with_sigma(10.0);

        // Blur extent should be ~3 sigma
        let extent = blur.calculate_blur_extent();
        assert!((extent - 30.0).abs() < 0.1);
    }
}
