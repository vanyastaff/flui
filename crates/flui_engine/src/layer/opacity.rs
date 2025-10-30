//! Opacity layer - applies opacity to child layer

use crate::layer::{base_single_child::SingleChildLayerBase, BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::events::{Event, HitTestResult};
use flui_types::{Offset, Rect};

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
    /// Base single-child layer functionality
    base: SingleChildLayerBase,

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
            base: SingleChildLayerBase::new(child),
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
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.base.child()
    }
}

impl Layer for OpacityLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.opacity <= 0.0 {
            // Fully transparent - skip painting
            return;
        }

        let Some(child) = self.base.child() else {
            return;
        };

        if self.opacity >= 1.0 {
            // Fully opaque - just paint child directly
            child.paint(painter);
            return;
        }

        // Apply opacity and paint child
        painter.save();
        painter.set_opacity(self.opacity);
        child.paint(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        self.base.child_bounds()
    }

    fn is_visible(&self) -> bool {
        self.opacity > 0.0 && self.base.is_child_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // Opacity doesn't affect hit testing geometry
        // Fully transparent layers don't receive hits
        if self.opacity > 0.0 {
            self.base.child_hit_test(position, result)
        } else {
            false
        }
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        self.base.child_handle_event(event)
    }

    fn dispose(&mut self) {
        self.base.dispose_child();
    }
}
