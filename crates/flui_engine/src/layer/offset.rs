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
//! container.add_child(Box::new(some_content));
//!
//! let offset_layer = OffsetLayer::new(Box::new(container))
//!     .with_offset(Offset::new(100.0, 50.0));
//! ```

use crate::layer::{BoxedLayer, Layer};
use crate::painter::Painter;
use flui_types::{Offset, Rect};
use flui_types::events::{Event, HitTestResult};

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
    /// The child layer to be offset
    child: Option<BoxedLayer>,

    /// The offset to apply
    offset: Offset,

    /// Cached bounds including offset
    cached_bounds: Option<Rect>,

    /// Disposal flag
    disposed: bool,
}

impl OffsetLayer {
    /// Creates a new offset layer with the given child.
    ///
    /// The offset defaults to (0, 0).
    pub fn new(child: BoxedLayer) -> Self {
        Self {
            child: Some(child),
            offset: Offset::ZERO,
            cached_bounds: None,
            disposed: false,
        }
    }

    /// Sets the offset for this layer.
    ///
    /// This determines how far the child content is shifted.
    pub fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self.cached_bounds = None;
        self
    }

    /// Updates the offset value.
    ///
    /// This invalidates cached bounds.
    pub fn set_offset(&mut self, offset: Offset) {
        if self.offset != offset {
            self.offset = offset;
            self.cached_bounds = None;
        }
    }

    /// Gets the current offset.
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Gets a reference to the child layer.
    pub fn child(&self) -> Option<&dyn Layer> {
        self.child.as_ref().map(|c| &**c as &dyn Layer)
    }

    /// Gets a mutable reference to the child layer.
    pub fn child_mut(&mut self) -> Option<&mut dyn Layer> {
        self.child.as_mut().map(|c| &mut **c as &mut dyn Layer)
    }
}

impl Layer for OffsetLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.disposed {
            return;
        }

        let Some(child) = &self.child else {
            return;
        };

        // Apply offset as translation
        painter.save();
        painter.translate(self.offset);
        child.paint(painter);
        painter.restore();
    }

    fn bounds(&self) -> Rect {
        if let Some(bounds) = self.cached_bounds {
            return bounds;
        }

        let child_bounds = self.child.as_ref().map_or(Rect::ZERO, |c| c.bounds());

        // Translate the bounds by the offset
        Rect::from_xywh(
            child_bounds.left() + self.offset.dx,
            child_bounds.top() + self.offset.dy,
            child_bounds.width(),
            child_bounds.height(),
        )
    }

    fn is_visible(&self) -> bool {
        !self.disposed && self.child.as_ref().is_some_and(|c| c.is_visible())
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        if self.disposed {
            return false;
        }

        let Some(child) = &self.child else {
            return false;
        };

        // Transform the position by subtracting the offset
        let local_position =
            Offset::new(position.dx - self.offset.dx, position.dy - self.offset.dy);

        child.hit_test(local_position, result)
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
        self.cached_bounds = None;
    }

    fn is_disposed(&self) -> bool {
        self.disposed
    }

    fn mark_needs_paint(&mut self) {
        self.cached_bounds = None;
        if let Some(child) = &mut self.child {
            child.mark_needs_paint();
        }
    }
}

impl Default for OffsetLayer {
    fn default() -> Self {
        Self {
            child: None,
            offset: Offset::ZERO,
            cached_bounds: None,
            disposed: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::PictureLayer;

    #[test]
    fn test_offset_layer_new() {
        let picture = PictureLayer::new();
        let offset_layer = OffsetLayer::new(Box::new(picture));

        assert_eq!(offset_layer.offset(), Offset::ZERO);
        assert!(!offset_layer.is_disposed());
    }

    #[test]
    fn test_offset_layer_with_offset() {
        let picture = PictureLayer::new();
        let offset = Offset::new(100.0, 50.0);
        let offset_layer = OffsetLayer::new(Box::new(picture)).with_offset(offset);

        assert_eq!(offset_layer.offset(), offset);
    }

    #[test]
    fn test_offset_layer_set_offset() {
        let picture = PictureLayer::new();
        let mut offset_layer = OffsetLayer::new(Box::new(picture));

        let new_offset = Offset::new(200.0, 100.0);
        offset_layer.set_offset(new_offset);

        assert_eq!(offset_layer.offset(), new_offset);
    }

    #[test]
    fn test_offset_layer_bounds() {
        let mut picture = PictureLayer::new();
        // Assume picture has some content at (0,0) with size 100x100
        // (In real usage, picture would have actual drawing commands)

        let offset = Offset::new(50.0, 30.0);
        let offset_layer = OffsetLayer::new(Box::new(picture)).with_offset(offset);

        let bounds = offset_layer.bounds();
        // Bounds should be translated by offset
        assert_eq!(bounds.left(), offset.dx);
        assert_eq!(bounds.top(), offset.dy);
    }

    #[test]
    fn test_offset_layer_dispose() {
        let picture = PictureLayer::new();
        let mut offset_layer = OffsetLayer::new(Box::new(picture));

        assert!(!offset_layer.is_disposed());

        offset_layer.dispose();

        assert!(offset_layer.is_disposed());
        assert!(offset_layer.child().is_none());
    }

    #[test]
    fn test_offset_layer_visibility() {
        use crate::layer::DrawCommand;
        use crate::painter::Paint;

        let mut picture = PictureLayer::new();
        // Add some content to make it visible
        picture.add_command(DrawCommand::Rect {
            rect: Rect::from_xywh(0.0, 0.0, 100.0, 100.0),
            paint: Paint::default(),
        });

        let offset_layer = OffsetLayer::new(Box::new(picture));

        // Should be visible if child is visible
        assert!(offset_layer.is_visible());
    }

    #[test]
    fn test_offset_layer_child_access() {
        let picture = PictureLayer::new();
        let mut offset_layer = OffsetLayer::new(Box::new(picture));

        assert!(offset_layer.child().is_some());
        assert!(offset_layer.child_mut().is_some());
    }
}
