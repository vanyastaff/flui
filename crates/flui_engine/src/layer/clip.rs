//! Clip layer - applies clipping to child layer

use flui_types::Rect;
use crate::layer::{Layer, BoxedLayer};
use crate::painter::{Painter, RRect};

/// Type of clipping to apply
#[derive(Debug, Clone)]
pub enum ClipBehavior {
    /// Clip to rectangle
    Rect(Rect),

    /// Clip to rounded rectangle
    RRect(RRect),

    // TODO: Add path clipping when needed
}

/// Layer that clips its child to a specific region
///
/// Content outside the clip region is not painted. This is used for
/// effects like scrolling, borders, and shape masking.
///
/// # Example
///
/// ```text
/// ClipLayer (clip to 100x100 rect)
///   └─ PictureLayer (draws 200x200 image)
/// Result: Only top-left 100x100 pixels visible
/// ```
pub struct ClipLayer {
    /// The child layer to clip
    child: BoxedLayer,

    /// The clipping behavior
    clip: ClipBehavior,
}

impl ClipLayer {
    /// Create a new clip layer
    pub fn new(child: BoxedLayer, clip: ClipBehavior) -> Self {
        Self {
            child,
            clip,
        }
    }

    /// Create a rectangular clip layer
    pub fn rect(child: BoxedLayer, rect: Rect) -> Self {
        Self::new(child, ClipBehavior::Rect(rect))
    }

    /// Create a rounded rectangular clip layer
    pub fn rrect(child: BoxedLayer, rrect: RRect) -> Self {
        Self::new(child, ClipBehavior::RRect(rrect))
    }

    /// Get the clip behavior
    pub fn clip(&self) -> &ClipBehavior {
        &self.clip
    }

    /// Set the clip behavior
    pub fn set_clip(&mut self, clip: ClipBehavior) {
        self.clip = clip;
    }

    /// Get the child layer
    pub fn child(&self) -> &BoxedLayer {
        &self.child
    }
}

impl Layer for ClipLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();

        // Apply clipping
        match &self.clip {
            ClipBehavior::Rect(rect) => {
                painter.clip_rect(*rect);
            }
            ClipBehavior::RRect(rrect) => {
                painter.clip_rrect(*rrect);
            }
        }

        // Paint child with clipping applied
        self.child.paint(painter);

        painter.restore();
    }

    fn bounds(&self) -> Rect {
        // Bounds are the intersection of child bounds and clip region
        let child_bounds = self.child.bounds();

        match &self.clip {
            ClipBehavior::Rect(rect) => {
                child_bounds.intersection(rect).unwrap_or(Rect::ZERO)
            }
            ClipBehavior::RRect(rrect) => {
                // Conservative approximation - use outer rect
                child_bounds.intersection(&rrect.rect).unwrap_or(Rect::ZERO)
            }
        }
    }

    fn is_visible(&self) -> bool {
        // Check if clip region has area
        let has_area = match &self.clip {
            ClipBehavior::Rect(rect) => rect.width() > 0.0 && rect.height() > 0.0,
            ClipBehavior::RRect(rrect) => {
                rrect.rect.width() > 0.0 && rrect.rect.height() > 0.0
            }
        };

        has_area && self.child.is_visible()
    }
}
