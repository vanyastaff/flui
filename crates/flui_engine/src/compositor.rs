//! Compositor - composes layers into final output
//!
//! The Compositor takes a Scene and renders it to a Surface using a Painter.
//! It handles:
//! - Layer traversal and painting
//! - Optimization (culling, caching)
//! - Performance tracking
//! - Frame synchronization

use crate::painter::Painter;
use crate::scene::Scene;
use flui_types::Rect;
use std::time::{Duration, Instant};

/// Statistics about composition
#[derive(Debug, Clone, Default)]
pub struct CompositionStats {
    /// Time spent compositing the last frame
    pub composition_time: Duration,

    /// Number of layers painted
    pub layers_painted: usize,

    /// Number of layers culled (skipped due to being off-screen)
    pub layers_culled: usize,

    /// Total bounds of painted content
    pub painted_bounds: Rect,

    /// Frame number
    pub frame_number: u64,
}

/// Compositor options
#[derive(Debug, Clone)]
pub struct CompositorOptions {
    /// Enable layer culling (skip layers outside viewport)
    pub enable_culling: bool,

    /// Viewport for culling calculations
    pub viewport: Rect,

    /// Enable debug visualization
    pub debug_mode: bool,

    /// Enable performance tracking
    pub track_performance: bool,
}

impl Default for CompositorOptions {
    fn default() -> Self {
        Self {
            enable_culling: true,
            viewport: Rect::ZERO,
            debug_mode: false,
            track_performance: true,
        }
    }
}

/// The Compositor renders a Scene to a Surface
///
/// # Architecture
///
/// ```text
/// Scene (Layer Tree)
///       │
///       ▼
/// Compositor (traversal + optimization)
///       │
///       ▼
/// Painter (backend-specific)
///       │
///       ▼
/// Surface (screen/buffer)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// let mut compositor = Compositor::new();
/// let mut scene = Scene::new(Size::new(800.0, 600.0));
///
/// // Build scene...
/// scene.add_layer(Box::new(my_layer));
///
/// // Composite to painter
/// compositor.composite(&scene, &mut painter);
///
/// // Check stats
/// println!("Painted {} layers in {:?}",
///     compositor.stats().layers_painted,
///     compositor.stats().composition_time);
/// ```
pub struct Compositor {
    /// Compositor options
    options: CompositorOptions,

    /// Composition statistics
    stats: CompositionStats,
}

impl Compositor {
    /// Create a new compositor with default options
    pub fn new() -> Self {
        Self {
            options: CompositorOptions::default(),
            stats: CompositionStats::default(),
        }
    }

    /// Create a compositor with custom options
    pub fn with_options(options: CompositorOptions) -> Self {
        Self {
            options,
            stats: CompositionStats::default(),
        }
    }

    /// Get compositor options
    pub fn options(&self) -> &CompositorOptions {
        &self.options
    }

    /// Get mutable compositor options
    pub fn options_mut(&mut self) -> &mut CompositorOptions {
        &mut self.options
    }

    /// Get composition statistics
    pub fn stats(&self) -> &CompositionStats {
        &self.stats
    }

    /// Set the viewport for culling
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.options.viewport = viewport;
    }

    /// Enable or disable layer culling
    pub fn set_culling_enabled(&mut self, enabled: bool) {
        self.options.enable_culling = enabled;
    }

    /// Composite a scene to a painter
    ///
    /// This is the main entry point for rendering. It traverses the scene's
    /// layer tree and paints each layer using the provided painter.
    ///
    /// # Arguments
    /// * `scene` - The scene to composite
    /// * `painter` - The painter to render with
    pub fn composite(&mut self, scene: &Scene, painter: &mut dyn Painter) {
        let start = if self.options.track_performance {
            Some(Instant::now())
        } else {
            None
        };

        // Reset stats
        self.stats.layers_painted = 0;
        self.stats.layers_culled = 0;
        self.stats.painted_bounds = Rect::ZERO;
        self.stats.frame_number = scene.metadata().frame_number;

        // Paint the scene root
        self.paint_layer(scene.root(), painter);

        // Update stats
        if let Some(start_time) = start {
            self.stats.composition_time = start_time.elapsed();
        }
    }

    /// Paint a single layer (with culling if enabled)
    fn paint_layer(&mut self, layer: &dyn crate::layer::Layer, painter: &mut dyn Painter) {
        // Check visibility
        if !layer.is_visible() {
            return;
        }

        // Cull layers outside viewport
        if self.options.enable_culling {
            let bounds = layer.bounds();
            if !bounds.intersects(&self.options.viewport) {
                self.stats.layers_culled += 1;
                return;
            }
        }

        // Paint the layer
        layer.paint(painter);

        // Update stats
        self.stats.layers_painted += 1;
        let bounds = layer.bounds();
        if self.stats.layers_painted == 1 {
            self.stats.painted_bounds = bounds;
        } else {
            self.stats.painted_bounds = self.stats.painted_bounds.union(&bounds);
        }
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = CompositionStats::default();
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::PictureLayer;
    use crate::painter::{Paint, Painter};
    use crate::scene::Scene;
    use flui_types::{Offset, Point, Size};

    // Mock painter for testing
    struct MockPainter {
        call_count: usize,
    }

    impl Painter for MockPainter {
        fn rect(&mut self, _rect: Rect, _paint: &Paint) {
            self.call_count += 1;
        }

        fn rrect(&mut self, _rrect: crate::painter::RRect, _paint: &Paint) {
            self.call_count += 1;
        }

        fn circle(&mut self, _center: Point, _radius: f32, _paint: &Paint) {
            self.call_count += 1;
        }

        fn line(&mut self, _p1: Point, _p2: Point, _paint: &Paint) {
            self.call_count += 1;
        }

        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _offset: Offset) {}
        fn rotate(&mut self, _angle: f32) {}
        fn scale(&mut self, _sx: f32, _sy: f32) {}
        fn clip_rect(&mut self, _rect: Rect) {}
        fn clip_rrect(&mut self, _rrect: crate::painter::RRect) {}
        fn set_opacity(&mut self, _opacity: f32) {}
    }

    #[test]
    fn test_compositor_creation() {
        let compositor = Compositor::new();
        assert!(compositor.options().enable_culling);
        assert!(compositor.options().track_performance);
    }

    #[test]
    fn test_composite_empty_scene() {
        let mut compositor = Compositor::new();
        let scene = Scene::new(Size::new(800.0, 600.0));
        let mut painter = MockPainter { call_count: 0 };

        compositor.composite(&scene, &mut painter);

        assert_eq!(compositor.stats().layers_painted, 0);
        assert_eq!(painter.call_count, 0);
    }

    #[test]
    fn test_composite_with_layers() {
        let mut compositor = Compositor::new();
        let mut scene = Scene::new(Size::new(800.0, 600.0));

        // Add a layer with content
        let mut picture = PictureLayer::new();
        picture.draw_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0), Paint::default());
        scene.add_layer(Box::new(picture));

        let mut painter = MockPainter { call_count: 0 };
        compositor.composite(&scene, &mut painter);

        assert_eq!(compositor.stats().layers_painted, 1);
        assert_eq!(painter.call_count, 1); // One rect drawn
    }

    #[test]
    fn test_culling_disabled() {
        let mut compositor = Compositor::new();
        compositor.set_culling_enabled(false);

        assert!(!compositor.options().enable_culling);
    }
}
