//! `PictureLayer` — sealed drawing commands, the leaf the paint walk emits.

use flui_painting::DisplayList;
use flui_types::geometry::{Pixels, Rect};
use std::sync::Arc;

/// Picture layer - a leaf layer that contains an immutable recorded picture
///
/// # Architecture
///
/// ```text
/// Canvas → finish() → DisplayList → PictureLayer → GPU
/// ```
///
/// Immutable once recorded: the backend replays the commands without
/// re-running paint, which is what makes a repaint boundary cheap to reuse.
///
/// # Usage
///
/// ```rust
/// use flui_layer::PictureLayer;
/// use flui_painting::{Canvas, Paint};
/// use flui_types::{geometry::{Rect, px}, styling::Color};
///
/// let mut canvas = Canvas::new();
/// canvas.draw_rect(
///     Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0)),
///     &Paint::fill(Color::RED),
/// );
/// let layer = PictureLayer::new(canvas.finish());
/// assert_eq!(layer.picture().len(), 1);
/// ```
#[derive(Clone)]
pub struct PictureLayer {
    /// The recorded drawing commands.
    ///
    /// `Arc` because a `DisplayList` is immutable once recorded and a
    /// retained boundary subtree is cloned into each frame's fresh
    /// `LayerTree`. Copying the command `Vec` per clone would cost about what
    /// regenerating it costs, which is what made cross-frame retention
    /// pointless before this.
    picture: Arc<DisplayList>,
}

impl std::fmt::Debug for PictureLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PictureLayer")
            .field("bounds", &self.picture.bounds())
            .field("command_count", &self.picture.len())
            .finish()
    }
}

impl PictureLayer {
    /// Wraps sealed drawing commands.
    ///
    /// ```rust
    /// use flui_layer::PictureLayer;
    /// use flui_painting::Canvas;
    ///
    /// let layer = PictureLayer::new(Canvas::new().finish());
    /// assert!(layer.is_empty());
    /// ```
    pub fn new(picture: DisplayList) -> Self {
        Self {
            picture: Arc::new(picture),
        }
    }

    /// The recorded commands the backend replays.
    pub fn picture(&self) -> &DisplayList {
        &self.picture
    }

    /// The union of the commands that contribute bounds, or `None` when
    /// none does (a list of only clips or `DrawPaint`s has no extent).
    pub fn bounds(&self) -> Option<Rect<Pixels>> {
        self.picture.bounds()
    }

    /// Whether there are no commands to replay.
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
    use flui_painting::Canvas;
    use flui_types::{Color, Point, Rect, geometry::px, painting::Paint};

    use super::*;

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
        let bounds = layer.bounds().expect("a drawn rect has bounds");

        // Bounds should encompass the drawn rectangle
        assert!(bounds.contains(Point::new(px(10.0), px(20.0))));
        assert!(bounds.contains(Point::new(px(100.0), px(200.0))));
    }

    #[test]
    fn test_empty_picture_layer() {
        let canvas = Canvas::new();
        let picture = canvas.finish();
        let layer = PictureLayer::new(picture);

        assert!(layer.is_empty());
    }
}
