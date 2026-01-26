//! OffsetLayer - Simple translation layer
//!
//! This layer applies a simple offset (translation) to its children.
//! Corresponds to Flutter's `OffsetLayer`, which is the base class
//! for repaint boundary layers.

use flui_types::geometry::{Pixels, Rect, Vec2};
use flui_types::Offset;

/// Layer that applies a simple offset to its children.
///
/// `OffsetLayer` is used primarily for repaint boundaries where
/// the entire subtree can be offset without repainting. This is
/// more efficient than a full `TransformLayer` when only translation
/// is needed.
///
/// # Use Cases
///
/// - Repaint boundary layers (like Flutter's `RepaintBoundary`)
/// - Scrolling content
/// - Animated translations
///
/// # Architecture
///
/// ```text
/// OffsetLayer
///   │
///   │ Apply offset to child coordinates
///   ▼
/// Children rendered at offset position
/// ```
///
/// # Example
///
/// ```rust
/// use flui_layer::OffsetLayer;
/// use flui_types::Offset;
///
/// let layer = OffsetLayer::new(Offset::new(10.0, 20.0));
///
/// assert_eq!(layer.offset().dx, 10.0);
/// assert_eq!(layer.offset().dy, 20.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OffsetLayer {
    /// The offset to apply to children
    offset: Offset<Pixels>,
}

impl OffsetLayer {
    /// Creates a new offset layer with the given offset.
    #[inline]
    pub const fn new(offset: Offset<Pixels>) -> Self {
        Self { offset }
    }

    /// Creates an offset layer with zero offset.
    #[inline]
    pub const fn zero() -> Self {
        Self::new(Offset::ZERO)
    }

    /// Creates an offset layer from x and y components.
    #[inline]
    pub const fn from_xy(dx: f32, dy: f32) -> Self {
        Self::new(Offset::new(dx, dy))
    }

    /// Returns the offset.
    #[inline]
    pub const fn offset(&self) -> Offset<Pixels> {
        self.offset
    }

    /// Sets the offset.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset<Pixels>) {
        self.offset = offset;
    }

    /// Returns the x component of the offset.
    #[inline]
    pub const fn dx(&self) -> f32 {
        self.offset.dx
    }

    /// Returns the y component of the offset.
    #[inline]
    pub const fn dy(&self) -> f32 {
        self.offset.dy
    }

    /// Returns true if the offset is zero.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.offset.dx == 0.0 && self.offset.dy == 0.0
    }

    /// Transforms a point by applying the offset.
    #[inline]
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        (x + self.offset.dx, y + self.offset.dy)
    }

    /// Transforms bounds by applying the offset.
    ///
    /// Returns the bounds translated by the offset.
    #[inline]
    pub fn transform_bounds(&self, bounds: Rect<Pixels>) -> Rect<Pixels> {
        bounds.translate(Vec2::new(self.offset.dx, self.offset.dy))
    }

    /// Computes the bounds for rendering to an image.
    ///
    /// This is used when capturing the layer to a texture for caching.
    #[inline]
    pub fn to_image_bounds(&self, child_bounds: Rect<Pixels>) -> Rect<Pixels> {
        self.transform_bounds(child_bounds)
    }

    /// Adds another offset to this layer.
    #[inline]
    pub fn add_offset(&mut self, offset: Offset<Pixels>) {
        self.offset = Offset::new(self.offset.dx + offset.dx, self.offset.dy + offset.dy);
    }
}

// Thread safety (Copy type, always safe)
unsafe impl Send for OffsetLayer {}
unsafe impl Sync for OffsetLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_layer_new() {
        let layer = OffsetLayer::new(Offset::new(10.0, 20.0));

        assert_eq!(layer.offset().dx, 10.0);
        assert_eq!(layer.offset().dy, 20.0);
    }

    #[test]
    fn test_offset_layer_zero() {
        let layer = OffsetLayer::zero();

        assert!(layer.is_zero());
        assert_eq!(layer.dx(), 0.0);
        assert_eq!(layer.dy(), 0.0);
    }

    #[test]
    fn test_offset_layer_from_xy() {
        let layer = OffsetLayer::from_xy(5.0, 15.0);

        assert_eq!(layer.dx(), 5.0);
        assert_eq!(layer.dy(), 15.0);
    }

    #[test]
    fn test_offset_layer_default() {
        let layer = OffsetLayer::default();

        assert!(layer.is_zero());
    }

    #[test]
    fn test_offset_layer_set_offset() {
        let mut layer = OffsetLayer::zero();

        layer.set_offset(Offset::new(100.0, 200.0));
        assert_eq!(layer.dx(), 100.0);
        assert_eq!(layer.dy(), 200.0);
    }

    #[test]
    fn test_offset_layer_transform_point() {
        let layer = OffsetLayer::new(Offset::new(10.0, 20.0));

        let (x, y) = layer.transform_point(5.0, 5.0);
        assert_eq!(x, 15.0);
        assert_eq!(y, 25.0);
    }

    #[test]
    fn test_offset_layer_transform_bounds() {
        let layer = OffsetLayer::new(Offset::new(10.0, 20.0));
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);

        let transformed = layer.transform_bounds(bounds);
        assert_eq!(transformed.left(), 10.0);
        assert_eq!(transformed.top(), 20.0);
        assert_eq!(transformed.width(), 100.0);
        assert_eq!(transformed.height(), 50.0);
    }

    #[test]
    fn test_offset_layer_add_offset() {
        let mut layer = OffsetLayer::new(Offset::new(10.0, 20.0));

        layer.add_offset(Offset::new(5.0, 10.0));
        assert_eq!(layer.dx(), 15.0);
        assert_eq!(layer.dy(), 30.0);
    }

    #[test]
    fn test_offset_layer_clone_copy() {
        let layer = OffsetLayer::new(Offset::new(10.0, 20.0));
        let cloned = layer.clone();
        let copied = layer; // Copy

        assert_eq!(layer, cloned);
        assert_eq!(layer, copied);
    }

    #[test]
    fn test_offset_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<OffsetLayer>();
        assert_sync::<OffsetLayer>();
    }
}
