//! `TextureLayer` — an external GPU texture (video, camera) drawn into a rectangle.

use flui_foundation::geometry::Rect;
use flui_painting::paint::{FilterQuality, TextureId};

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
/// use flui_foundation::geometry::Rect;
/// use flui_painting::paint::{FilterQuality, TextureId};
///
/// // Create a texture layer for video playback
/// let texture_id = TextureId::new(42);
/// let rect = Rect::from_xywh(0.0, 0.0, 640.0, 480.0);
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
    rect: Rect<f64>,

    /// Whether the texture is frozen (not updating)
    freeze: bool,

    /// Filter quality for texture sampling
    filter_quality: FilterQuality,

    /// Opacity (0.0 = transparent, 1.0 = opaque)
    opacity: f64,
}

impl TextureLayer {
    /// Draws the external GPU texture `texture_id` into `rect`.
    #[inline]
    pub fn new(texture_id: TextureId, rect: Rect<f64>) -> Self {
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
    pub fn with_opacity(mut self, opacity: f64) -> Self {
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
    pub fn bounds(&self) -> Rect<f64> {
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
    pub fn opacity(&self) -> f64 {
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
