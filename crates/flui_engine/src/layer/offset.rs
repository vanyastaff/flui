//! Offset layer for simple positional shifts.
//!
//! OffsetLayer translates its children by a fixed offset without using
//! a full transformation matrix. This is more efficient than TransformLayer
//! for simple translations.
//!
//! ## Example
//!
//! ```rust,ignore
//! let mut container = ContainerLayer::new();
//! container.child(Box::new(some_content));
//!
//! let offset_layer = OffsetLayer::new(Box::new(container))
//!     .with_offset(Offset::new(100.0, 50.0));
//! ```

use crate::layer::{base_single_child::SingleChildLayerBase, BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::events::{Event, HitTestResult};
use flui_types::{Offset, Rect};

/// A layer that shifts its children by a fixed offset.
///
/// This is simpler and more efficient than using TransformLayer for
/// basic translations. The offset is applied to all child content.
///
/// ## Performance
///
/// OffsetLayer is optimized for translation-only operations and
/// avoids the overhead of matrix calculations.
pub struct OffsetLayer {
    /// Base single-child layer functionality
    base: SingleChildLayerBase,

    /// The offset to apply
    offset: Offset,
}

impl OffsetLayer {
    /// Creates a new offset layer with the given child.
    ///
    /// The offset defaults to (0, 0).
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            base: SingleChildLayerBase::new(child),
            offset: Offset::ZERO,
        }
    }

    /// Sets the offset for this layer.
    ///
    /// This determines how far the child content is shifted.
    #[must_use]
    pub fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self.base.invalidate_cache();
        self
    }

    /// Updates the offset value.
    ///
    /// This invalidates cached bounds.
    pub fn set_offset(&mut self, offset: Offset) {
        if self.offset != offset {
            self.offset = offset;
            self.base.invalidate_cache();
        }
    }

    /// Gets the current offset.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Gets a reference to the child layer.
    pub fn child(&self) -> Option<&BoxedLayer> {
        self.base.child()
    }

    /// Gets a mutable reference to the child layer.
    pub fn child_mut(&mut self) -> Option<&mut BoxedLayer> {
        self.base.child_mut()
    }
}

impl Layer for OffsetLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        let Some(child) = self.base.child() else {
            return;
        };

        // Apply offset as translation
        painter.save();
        painter.translate(self.offset);
        child.paint(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        // Check cache first
        if let Some(cached) = self.base.cached_bounds() {
            return cached;
        }

        let child_bounds = self.base.child_bounds();

        // Translate the bounds by the offset
        // Note: Can't cache because bounds() takes &self, but this is fine
        Rect::from_xywh(
            child_bounds.left() + self.offset.dx,
            child_bounds.top() + self.offset.dy,
            child_bounds.width(),
            child_bounds.height(),
        )
    }

    fn is_visible(&self) -> bool {
        self.base.is_child_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        let Some(child) = self.base.child() else {
            return false;
        };

        // Transform the position by subtracting the offset
        let local_position =
            Offset::new(position.dx - self.offset.dx, position.dy - self.offset.dy);

        child.hit_test(local_position, result)
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
        self.base.invalidate_cache();
        if let Some(child) = self.base.child_mut() {
            child.mark_needs_paint();
        }
    }
}

impl Default for OffsetLayer {
    fn default() -> Self {
        Self {
            base: SingleChildLayerBase::default(),
            offset: Offset::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::PictureLayer;

    #[test]
    fn test_offset_layer_creation() {
        let picture = PictureLayer::new();
        let layer = OffsetLayer::new(Box::new(picture));

        assert_eq!(layer.offset(), Offset::ZERO);
    }

    #[test]
    fn test_offset_layer_with_offset() {
        let picture = PictureLayer::new();
        let layer = OffsetLayer::new(Box::new(picture)).with_offset(Offset::new(10.0, 20.0));

        assert_eq!(layer.offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_offset_layer_set_offset() {
        let picture = PictureLayer::new();
        let mut layer = OffsetLayer::new(Box::new(picture));

        layer.set_offset(Offset::new(30.0, 40.0));
        assert_eq!(layer.offset(), Offset::new(30.0, 40.0));
    }
}
