//! TextureLayer - External GPU texture rendering
//!
//! This layer displays an external GPU texture (video, camera, platform view)
//! at a specific location. Corresponds to Flutter's `TextureLayer`.

use flui_types::geometry::Rect;
use flui_types::painting::{FilterQuality, TextureId};

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
/// use flui_layer::TextureLayer;
/// use flui_types::geometry::Rect;
/// use flui_types::painting::{TextureId, FilterQuality};
///
/// // Create a texture layer for video playback
/// let texture_id = TextureId::new(42);
/// let rect = Rect::from_xywh(0.0, 0.0, 640.0, 480.0);
/// let layer = TextureLayer::new(texture_id, rect);
///
/// // With custom filter quality
/// let hq_layer = TextureLayer::new(texture_id, rect)
///     .with_filter_quality(FilterQuality::High);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureLayer {
    /// The texture ID referencing an external GPU texture
    texture_id: TextureId,

    /// Destination rectangle where the texture will be drawn
    rect: Rect,

    /// Whether the texture is frozen (not updating)
    freeze: bool,

    /// Filter quality for texture sampling
    filter_quality: FilterQuality,

    /// Opacity (0.0 = transparent, 1.0 = opaque)
    opacity: f32,
}

impl TextureLayer {
    /// Creates a new texture layer.
    ///
    /// # Arguments
    ///
    /// * `texture_id` - ID of the external GPU texture
    /// * `rect` - Destination rectangle for the texture
    #[inline]
    pub fn new(texture_id: TextureId, rect: Rect) -> Self {
        Self {
            texture_id,
            rect,
            freeze: false,
            filter_quality: FilterQuality::Low,
            opacity: 1.0,
        }
    }

    /// Creates a frozen texture layer (texture won't update).
    #[inline]
    pub fn frozen(texture_id: TextureId, rect: Rect) -> Self {
        Self {
            texture_id,
            rect,
            freeze: true,
            filter_quality: FilterQuality::Low,
            opacity: 1.0,
        }
    }

    /// Sets the filter quality for texture sampling.
    #[inline]
    pub fn with_filter_quality(mut self, quality: FilterQuality) -> Self {
        self.filter_quality = quality;
        self
    }

    /// Sets the opacity.
    #[inline]
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Sets the freeze state.
    #[inline]
    pub fn with_freeze(mut self, freeze: bool) -> Self {
        self.freeze = freeze;
        self
    }

    /// Returns the texture ID.
    #[inline]
    pub fn texture_id(&self) -> TextureId {
        self.texture_id
    }

    /// Returns the destination rectangle.
    #[inline]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Returns the bounds (same as rect for texture layers).
    #[inline]
    pub fn bounds(&self) -> Rect {
        self.rect
    }

    /// Returns whether the texture is frozen.
    #[inline]
    pub fn is_frozen(&self) -> bool {
        self.freeze
    }

    /// Returns the filter quality.
    #[inline]
    pub fn filter_quality(&self) -> FilterQuality {
        self.filter_quality
    }

    /// Returns the opacity.
    #[inline]
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the texture ID.
    #[inline]
    pub fn set_texture_id(&mut self, texture_id: TextureId) {
        self.texture_id = texture_id;
    }

    /// Sets the destination rectangle.
    #[inline]
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Sets the freeze state.
    #[inline]
    pub fn set_freeze(&mut self, freeze: bool) {
        self.freeze = freeze;
    }

    /// Sets the filter quality.
    #[inline]
    pub fn set_filter_quality(&mut self, quality: FilterQuality) {
        self.filter_quality = quality;
    }

    /// Sets the opacity.
    #[inline]
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Returns true if the texture is fully transparent.
    #[inline]
    pub fn is_invisible(&self) -> bool {
        self.opacity <= 0.0
    }

    /// Returns true if the texture is fully opaque.
    #[inline]
    pub fn is_opaque(&self) -> bool {
        self.opacity >= 1.0
    }
}

impl Default for TextureLayer {
    fn default() -> Self {
        Self {
            texture_id: TextureId::new(0),
            rect: Rect::ZERO,
            freeze: false,
            filter_quality: FilterQuality::Low,
            opacity: 1.0,
        }
    }
}

// Thread safety
unsafe impl Send for TextureLayer {}
unsafe impl Sync for TextureLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_texture_layer_new() {
        let id = TextureId::new(123);
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let layer = TextureLayer::new(id, rect);

        assert_eq!(layer.texture_id(), id);
        assert_eq!(layer.rect(), rect);
        assert!(!layer.is_frozen());
        assert_eq!(layer.filter_quality(), FilterQuality::Low);
        assert_eq!(layer.opacity(), 1.0);
    }

    #[test]
    fn test_texture_layer_frozen() {
        let id = TextureId::new(456);
        let rect = Rect::from_xywh(0.0, 0.0, 640.0, 480.0);
        let layer = TextureLayer::frozen(id, rect);

        assert!(layer.is_frozen());
    }

    #[test]
    fn test_texture_layer_with_filter_quality() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let layer = TextureLayer::new(id, rect).with_filter_quality(FilterQuality::High);

        assert_eq!(layer.filter_quality(), FilterQuality::High);
    }

    #[test]
    fn test_texture_layer_with_opacity() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let layer = TextureLayer::new(id, rect).with_opacity(0.5);

        assert_eq!(layer.opacity(), 0.5);
    }

    #[test]
    fn test_texture_layer_opacity_clamping() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let layer1 = TextureLayer::new(id, rect).with_opacity(-0.5);
        assert_eq!(layer1.opacity(), 0.0);

        let layer2 = TextureLayer::new(id, rect).with_opacity(1.5);
        assert_eq!(layer2.opacity(), 1.0);
    }

    #[test]
    fn test_texture_layer_bounds() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        let layer = TextureLayer::new(id, rect);

        assert_eq!(layer.bounds(), rect);
    }

    #[test]
    fn test_texture_layer_setters() {
        let mut layer = TextureLayer::default();

        layer.set_texture_id(TextureId::new(999));
        layer.set_rect(Rect::from_xywh(5.0, 5.0, 50.0, 50.0));
        layer.set_freeze(true);
        layer.set_filter_quality(FilterQuality::Medium);
        layer.set_opacity(0.8);

        assert_eq!(layer.texture_id(), TextureId::new(999));
        assert_eq!(layer.rect().left(), 5.0);
        assert!(layer.is_frozen());
        assert_eq!(layer.filter_quality(), FilterQuality::Medium);
        assert_eq!(layer.opacity(), 0.8);
    }

    #[test]
    fn test_texture_layer_visibility() {
        let id = TextureId::new(1);
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

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

    #[test]
    fn test_texture_layer_clone_copy() {
        let layer = TextureLayer::new(TextureId::new(1), Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
        let copied = layer;
        let cloned = layer.clone();

        assert_eq!(layer, copied);
        assert_eq!(layer, cloned);
    }

    #[test]
    fn test_texture_layer_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<TextureLayer>();
        assert_sync::<TextureLayer>();
    }
}
