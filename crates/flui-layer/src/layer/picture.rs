//! Picture layer - leaf layer with recorded drawing commands
//!
//! PictureLayer is Flutter's standard layer for caching drawing commands.
//! It stores an immutable Picture (DisplayList) that can be replayed efficiently.

use flui_painting::{DisplayListCore, Picture};
use flui_types::geometry::{Pixels, Rect};

/// Picture layer - a leaf layer that contains an immutable recorded picture
///
/// # Architecture
///
/// ```text
/// Canvas → finish() → Picture (DisplayList) → PictureLayer → GPU
/// ```
///
/// PictureLayer is used by Flutter's PaintingContext to cache drawing commands
/// for efficient repainting. Unlike CanvasLayer (mutable), PictureLayer stores
/// an immutable Picture that was already recorded.
///
/// # Flutter Equivalence
///
/// ```dart
/// class PictureLayer extends Layer {
///   Picture? picture;
///
///   void addToScene(SceneBuilder builder) {
///     if (picture != null) {
///       builder.addPicture(offset, picture!);
///     }
///   }
/// }
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use flui_layer::PictureLayer;
/// use flui_painting::Canvas;
///
/// // Record drawing commands
/// let mut canvas = Canvas::new();
/// canvas.draw_rect(rect, &paint);
/// let picture = canvas.finish();
///
/// // Store in layer
/// let layer = PictureLayer::new(picture);
///
/// // Later: replay for rendering
/// let display_list = layer.picture();
/// ```
///
/// # Performance
///
/// PictureLayer enables Flutter's repaint boundary optimization:
/// - Cached pictures can be replayed without re-executing paint methods
/// - Reduces CPU overhead for unchanged content
/// - Enables partial screen updates
#[derive(Clone)]
pub struct PictureLayer {
    /// The recorded picture (immutable DisplayList)
    picture: Picture,

    /// Estimated bounds for culling
    bounds: Rect<Pixels>,
}

impl std::fmt::Debug for PictureLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PictureLayer")
            .field("bounds", &self.bounds)
            .field("command_count", &self.picture.len())
            .finish()
    }
}

impl PictureLayer {
    /// Creates a new picture layer with recorded drawing commands.
    ///
    /// # Arguments
    ///
    /// * `picture` - The recorded Picture (DisplayList) to store
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_layer::PictureLayer;
    /// use flui_painting::Canvas;
    ///
    /// let mut canvas = Canvas::new();
    /// canvas.draw_circle(Point::ZERO, 50.0, &paint);
    /// let picture = canvas.finish();
    ///
    /// let layer = PictureLayer::new(picture);
    /// ```
    pub fn new(picture: Picture) -> Self {
        let bounds = picture.bounds();
        Self { picture, bounds }
    }

    /// Creates a picture layer with explicit bounds.
    ///
    /// This is useful when the bounds are known upfront or need to be
    /// different from the picture's calculated bounds (e.g., for culling).
    ///
    /// # Arguments
    ///
    /// * `picture` - The recorded Picture (DisplayList)
    /// * `bounds` - Explicit bounds for this layer
    pub fn with_bounds(picture: Picture, bounds: Rect<Pixels>) -> Self {
        Self { picture, bounds }
    }

    /// Returns a reference to the stored picture.
    ///
    /// The picture contains the recorded drawing commands and can be
    /// replayed by the rendering backend.
    pub fn picture(&self) -> &Picture {
        &self.picture
    }

    /// Returns the bounds of this layer's content.
    ///
    /// These bounds are used for culling - if the layer is outside the
    /// visible viewport, it can be skipped during rendering.
    pub fn bounds(&self) -> Rect<Pixels> {
        self.bounds
    }

    /// Updates the bounds of this layer.
    ///
    /// This is useful when the layer is repositioned or when custom
    /// bounds are needed for culling optimization.
    pub fn set_bounds(&mut self, bounds: Rect<Pixels>) {
        self.bounds = bounds;
    }

    /// Replaces the picture in this layer.
    ///
    /// This updates both the picture and automatically recalculates bounds
    /// from the new picture.
    ///
    /// # Arguments
    ///
    /// * `picture` - New recorded picture to store
    pub fn set_picture(&mut self, picture: Picture) {
        self.bounds = picture.bounds();
        self.picture = picture;
    }

    /// Returns the number of drawing commands in the picture.
    pub fn command_count(&self) -> usize {
        self.picture.len()
    }

    /// Checks if the layer is empty (has no drawing commands).
    pub fn is_empty(&self) -> bool {
        self.picture.is_empty()
    }
}

impl Default for PictureLayer {
    fn default() -> Self {
        // Create empty picture from empty canvas
        let canvas = flui_painting::Canvas::new();
        let picture = canvas.finish();
        Self::new(picture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_painting::Canvas;
    use flui_types::geometry::px;
    use flui_types::painting::Paint;
    use flui_types::{Color, Point, Rect};

    #[test]
    fn test_picture_layer_creation() {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
            &Paint::fill(Color::RED),
        );
        let picture = canvas.finish();

        let layer = PictureLayer::new(picture);
        assert!(!layer.is_empty());
        assert_eq!(layer.command_count(), 1);
    }

    #[test]
    fn test_picture_layer_bounds() {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_ltrb(px(10.0), px(20.0), px(100.0), px(200.0)),
            &Paint::fill(Color::BLUE),
        );
        let picture = canvas.finish();

        let layer = PictureLayer::new(picture);
        let bounds = layer.bounds();

        // Bounds should encompass the drawn rectangle
        assert!(bounds.contains(Point::new(px(10.0), px(20.0))));
        assert!(bounds.contains(Point::new(px(100.0), px(200.0))));
    }

    #[test]
    fn test_picture_layer_with_custom_bounds() {
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(50.0), px(50.0)),
            &Paint::fill(Color::GREEN),
        );
        let picture = canvas.finish();

        let custom_bounds = Rect::from_ltrb(px(0.0), px(0.0), px(200.0), px(200.0));
        let layer = PictureLayer::with_bounds(picture, custom_bounds);

        assert_eq!(layer.bounds(), custom_bounds);
    }

    #[test]
    fn test_picture_layer_set_bounds() {
        let mut canvas = Canvas::new();
        canvas.draw_circle(Point::new(px(50.0), px(50.0)), px(25.0), &Paint::fill(Color::YELLOW));
        let picture = canvas.finish();

        let mut layer = PictureLayer::new(picture);
        let new_bounds = Rect::from_ltrb(px(0.0), px(0.0), px(150.0), px(150.0));

        layer.set_bounds(new_bounds);
        assert_eq!(layer.bounds(), new_bounds);
    }

    #[test]
    fn test_empty_picture_layer() {
        let canvas = Canvas::new();
        let picture = canvas.finish();
        let layer = PictureLayer::new(picture);

        assert!(layer.is_empty());
        assert_eq!(layer.command_count(), 0);
    }
}
