//! Opacity layer - applies opacity to child layer

use crate::layer::{BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::{Offset, Rect};
use flui_types::events::{Event, HitTestResult};

/// Layer that applies opacity to its child
///
/// Opacity is applied multiplicatively - if this layer has opacity 0.5
/// and the parent also has opacity 0.5, the effective opacity is 0.25.
///
/// # Example
///
/// ```text
/// OpacityLayer (opacity: 0.5)
///   └─ PictureLayer (draws red box)
/// Result: Semi-transparent red box
/// ```
pub struct OpacityLayer {
    /// The child layer to apply opacity to
    child: BoxedLayer,

    /// Opacity value (0.0 = transparent, 1.0 = opaque)
    opacity: f32,
}

impl OpacityLayer {
    /// Create a new opacity layer
    ///
    /// # Arguments
    /// * `child` - The child layer to apply opacity to
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque)
    pub fn new(child: BoxedLayer, opacity: f32) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&opacity),
            "Opacity must be between 0.0 and 1.0"
        );

        Self {
            child,
            opacity: opacity.clamp(0.0, 1.0),
        }
    }

    /// Get the opacity value
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Set the opacity value
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Get the child layer
    pub fn child(&self) -> &BoxedLayer {
        &self.child
    }
}

impl Layer for OpacityLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.opacity <= 0.0 {
            // Fully transparent - skip painting
            return;
        }

        if self.opacity >= 1.0 {
            // Fully opaque - just paint child directly
            self.child.paint(painter);
            return;
        }

        // Apply opacity and paint child
        painter.save();
        painter.set_opacity(self.opacity);
        self.child.paint(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        self.child.bounds()
    }

    fn is_visible(&self) -> bool {
        self.opacity > 0.0 && self.child.is_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Opacity doesn't affect hit testing geometry
        // Just forward to child
        if self.opacity > 0.0 {
            self.child.hit_test(position, result)
        } else {
            false // Fully transparent layers don't receive hits
        }
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Forward event to child
        self.child.handle_event(event)
    }
}
