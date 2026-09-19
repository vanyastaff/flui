//! `TextureLayer` — an external GPU texture (video, camera) drawn into a rectangle.

use flui_types::{
    geometry::{Pixels, Rect},
    painting::{FilterQuality, TextureId},
};

/// Layer that displays an external GPU texture.
///
/// Used for rendering content that comes from external sources:
/// - Video playback
/// - Camera preview
/// - Platform views (native UI)
/// - Custom GPU computations
///
/// # Architecture
///
/// ```text
/// External Source (Video/Camera/Native)
///   │
///   │ Provides GPU texture
///   ▼
/// TextureLayer
///   │
///   │ Composites texture at rect
///   ▼
/// Final Output
/// ```
///
/// # Example
///
/// ```rust
/// use flui_types::geometry::px;
/// use flui_layer::TextureLayer;
/// use flui_types::{
///     geometry::Rect,
///     painting::{FilterQuality, TextureId},
/// };
///
/// // Create a texture layer for video playback
/// let texture_id = TextureId::new(42);
/// let rect = Rect::from_xywh(px(0.0), px(0.0), px(640.0), px(480.0));
/// let layer = TextureLayer::new(texture_id, rect);
///
/// // With custom filter quality
/// let hq_layer = TextureLayer::new(texture_id, rect).with_filter_quality(FilterQuality::High);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureLayer {
    /// The texture ID referencing an external GPU texture
    texture_id: TextureId,

    /// Destination rectangle where the texture will be drawn
    rect: Rect<Pixels>,

    /// Whether the texture is frozen (not updating)
    freeze: bool,

    /// Filter quality for texture sampling
    filter_quality: FilterQuality,

    /// Opacity (0.0 = transparent, 1.0 = opaque)
    opacity: f32,
}

impl TextureLayer {
    /// Draws the external GPU texture `texture_id` into `rect`.
    #[inline]
    pub fn new(texture_id: TextureId, rect: Rect<Pixels>) -> Self {
        Self {
            texture_id,
            rect,
            freeze: false,
            filter_quality: FilterQuality::Low,
            opacity: 1.0,
        }
    }

    /// How the texture is sampled when scaled.
    #[inline]
    #[must_use]
    pub fn with_filter_quality(mut self, quality: FilterQuality) -> Self {
        self.filter_quality = quality;
        self
    }

    /// The alpha the texture is drawn with, clamped to `0.0..=1.0`.
    #[inline]
    #[must_use]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = super::unit_alpha(opacity);
        self
    }

    /// The external texture to draw.
    #[inline]
    pub fn texture_id(&self) -> TextureId {
        self.texture_id
    }

    /// The destination rectangle.
    #[inline]
    pub fn bounds(&self) -> Rect<Pixels> {
        self.rect
    }

    /// See [`Self::set_freeze`].
    #[inline]
    pub fn is_frozen(&self) -> bool {
        self.freeze
    }

    /// See [`Self::with_filter_quality`].
    #[inline]
    pub fn filter_quality(&self) -> FilterQuality {
        self.filter_quality
    }

    /// See [`Self::with_opacity`].
    #[inline]
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// While frozen the backend keeps showing the last
    /// frame it received, so a texture resized by the platform does not flicker.
    #[inline]
    pub fn set_freeze(&mut self, freeze: bool) {
        self.freeze = freeze;
    }

    /// Whether opacity is 0.
    #[inline]
    pub fn is_invisible(&self) -> bool {
        self.opacity <= 0.0
    }

    /// Whether opacity is 1.
    #[inline]
    pub fn is_opaque(&self) -> bool {
        self.opacity >= 1.0
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_texture_layer_new() {
        let id = TextureId::new(123);
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = TextureLayer::new(id, rect);

        assert_eq!(layer.texture_id(), id);
        assert_eq!(layer.bounds(), rect);
        assert!(!layer.is_frozen());
        assert_eq!(layer.filter_quality(), FilterQuality::Low);
        assert_eq!(layer.opacity(), 1.0);
    }

    #[test]
    fn test_texture_layer_with_filter_quality() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let layer = TextureLayer::new(id, rect).with_filter_quality(FilterQuality::High);

        assert_eq!(layer.filter_quality(), FilterQuality::High);
    }

    #[test]
    fn test_texture_layer_with_opacity() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let layer = TextureLayer::new(id, rect).with_opacity(0.5);

        assert_eq!(layer.opacity(), 0.5);
    }

    #[test]
    fn test_texture_layer_opacity_clamping() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        let layer1 = TextureLayer::new(id, rect).with_opacity(-0.5);
        assert_eq!(layer1.opacity(), 0.0);

        let layer2 = TextureLayer::new(id, rect).with_opacity(1.5);
        assert_eq!(layer2.opacity(), 1.0);
    }

    #[test]
    fn test_texture_layer_bounds() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0));
        let layer = TextureLayer::new(id, rect);

        assert_eq!(layer.bounds(), rect);
    }

    #[test]
    fn test_texture_layer_visibility() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));

        let invisible = TextureLayer::new(id, rect).with_opacity(0.0);
        assert!(invisible.is_invisible());
        assert!(!invisible.is_opaque());

        let opaque = TextureLayer::new(id, rect).with_opacity(1.0);
        assert!(!opaque.is_invisible());
        assert!(opaque.is_opaque());

        let semi = TextureLayer::new(id, rect).with_opacity(0.5);
        assert!(!semi.is_invisible());
        assert!(!semi.is_opaque());
    }
}
