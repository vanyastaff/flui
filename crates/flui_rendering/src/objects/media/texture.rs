//! RenderTexture - GPU texture rendering
//!
//! Flutter reference: <https://api.flutter.dev/flutter/widgets/Texture-class.html>

use crate::core::{BoxProtocol, LayoutContext, Leaf, PaintContext, RenderBox};
use flui_types::styling::BoxFit;
use flui_types::{Rect, Size};

// Re-export TextureId and FilterQuality from flui_types for convenience
pub use flui_types::painting::{FilterQuality, TextureId};

/// RenderObject for displaying GPU textures
///
/// Renders a platform-specific GPU texture (e.g., from video decoder,
/// camera, or external rendering context). The texture is referenced
/// by a TextureId handle.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderTexture, TextureId};
/// use flui_types::BoxFit;
///
/// let texture_id = TextureId::new(42);
/// let mut texture = RenderTexture::new(texture_id);
/// texture.set_fit(BoxFit::Cover);
/// ```
#[derive(Debug)]
pub struct RenderTexture {
    /// GPU texture ID
    pub texture_id: TextureId,
    /// How to inscribe texture into the available space
    pub fit: BoxFit,
    /// Whether to freeze the texture (don't update)
    pub freeze: bool,
    /// Optional filter quality
    pub filter_quality: FilterQuality,

    // Cache for layout
    size: Size,
}

impl RenderTexture {
    /// Create new RenderTexture with texture ID
    pub fn new(texture_id: TextureId) -> Self {
        Self {
            texture_id,
            fit: BoxFit::Contain,
            freeze: false,
            filter_quality: FilterQuality::default(),
            size: Size::ZERO,
        }
    }

    /// Set texture ID
    pub fn set_texture_id(&mut self, texture_id: TextureId) {
        self.texture_id = texture_id;
    }

    /// Set box fit mode
    pub fn set_fit(&mut self, fit: BoxFit) {
        self.fit = fit;
    }

    /// Set freeze flag
    pub fn set_freeze(&mut self, freeze: bool) {
        self.freeze = freeze;
    }

    /// Set filter quality
    pub fn set_filter_quality(&mut self, quality: FilterQuality) {
        self.filter_quality = quality;
    }

    /// Create with specific fit
    pub fn with_fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// Create with freeze enabled
    pub fn frozen(mut self) -> Self {
        self.freeze = true;
        self
    }

    /// Create with specific filter quality
    pub fn with_filter_quality(mut self, quality: FilterQuality) -> Self {
        self.filter_quality = quality;
        self
    }

    /// Calculate destination rectangle based on BoxFit
    /// Returns the rect where the texture should be drawn
    pub fn calculate_dest_rect(&self, texture_size: Size, available_size: Size) -> Rect {
        if texture_size.is_empty() || available_size.is_empty() {
            return Rect::from_xywh(0.0, 0.0, available_size.width, available_size.height);
        }

        match self.fit {
            BoxFit::Fill => {
                // Stretch to fill entire space
                Rect::from_xywh(0.0, 0.0, available_size.width, available_size.height)
            }
            BoxFit::Contain => {
                // Scale to fit inside while maintaining aspect ratio
                let texture_aspect = texture_size.width / texture_size.height;
                let available_aspect = available_size.width / available_size.height;

                let (width, height) = if texture_aspect > available_aspect {
                    // Width-constrained
                    (available_size.width, available_size.width / texture_aspect)
                } else {
                    // Height-constrained
                    (
                        available_size.height * texture_aspect,
                        available_size.height,
                    )
                };

                // Center in available space
                let x = (available_size.width - width) / 2.0;
                let y = (available_size.height - height) / 2.0;

                Rect::from_xywh(x, y, width, height)
            }
            BoxFit::Cover => {
                // Scale to cover entire space while maintaining aspect ratio
                let texture_aspect = texture_size.width / texture_size.height;
                let available_aspect = available_size.width / available_size.height;

                let (width, height) = if texture_aspect > available_aspect {
                    // Height-constrained, width overflow
                    (
                        available_size.height * texture_aspect,
                        available_size.height,
                    )
                } else {
                    // Width-constrained, height overflow
                    (available_size.width, available_size.width / texture_aspect)
                };

                // Center in available space
                let x = (available_size.width - width) / 2.0;
                let y = (available_size.height - height) / 2.0;

                Rect::from_xywh(x, y, width, height)
            }
            BoxFit::FitWidth => {
                // Scale to fit width
                let texture_aspect = texture_size.width / texture_size.height;
                let height = available_size.width / texture_aspect;
                let y = (available_size.height - height) / 2.0;

                Rect::from_xywh(0.0, y, available_size.width, height)
            }
            BoxFit::FitHeight => {
                // Scale to fit height
                let texture_aspect = texture_size.width / texture_size.height;
                let width = available_size.height * texture_aspect;
                let x = (available_size.width - width) / 2.0;

                Rect::from_xywh(x, 0.0, width, available_size.height)
            }
            BoxFit::None => {
                // Original size, centered
                let x = (available_size.width - texture_size.width) / 2.0;
                let y = (available_size.height - texture_size.height) / 2.0;

                Rect::from_xywh(x, y, texture_size.width, texture_size.height)
            }
            BoxFit::ScaleDown => {
                // Like Contain, but never scale up
                if texture_size.width <= available_size.width
                    && texture_size.height <= available_size.height
                {
                    // Use original size
                    let x = (available_size.width - texture_size.width) / 2.0;
                    let y = (available_size.height - texture_size.height) / 2.0;
                    Rect::from_xywh(x, y, texture_size.width, texture_size.height)
                } else {
                    // Scale down like Contain
                    let texture_aspect = texture_size.width / texture_size.height;
                    let available_aspect = available_size.width / available_size.height;

                    let (width, height) = if texture_aspect > available_aspect {
                        (available_size.width, available_size.width / texture_aspect)
                    } else {
                        (
                            available_size.height * texture_aspect,
                            available_size.height,
                        )
                    };

                    let x = (available_size.width - width) / 2.0;
                    let y = (available_size.height - height) / 2.0;

                    Rect::from_xywh(x, y, width, height)
                }
            }
        }
    }
}

impl RenderBox<Leaf> for RenderTexture {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;

        // Textures typically take up all available space
        // In real implementation, we might query texture dimensions
        let size = Size::new(constraints.max_width, constraints.max_height);

        self.size = size;
        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: crate::core::PaintTree,
    {
        // Calculate destination rect based on layout size
        let dst_rect = Rect::from_min_size(flui_types::Point::ZERO, self.size);

        // Draw the GPU texture using Canvas API
        // The texture will be looked up by ID in the rendering engine's texture registry
        ctx.canvas().texture(
            self.texture_id,
            dst_rect,
            None, // Use entire texture (no source rect cropping)
            self.filter_quality,
            1.0, // Full opacity
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_texture_id_new() {
        let id = TextureId::new(42);
        assert_eq!(id.get(), 42);
    }

    #[test]
    fn test_texture_id_equality() {
        let id1 = TextureId::new(42);
        let id2 = TextureId::new(42);
        let id3 = TextureId::new(43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_filter_quality_default() {
        assert_eq!(FilterQuality::default(), FilterQuality::Low);
    }

    #[test]
    fn test_filter_quality_variants() {
        let qualities = vec![
            FilterQuality::None,
            FilterQuality::Low,
            FilterQuality::Medium,
            FilterQuality::High,
        ];

        // Verify all variants are distinct
        for (i, q1) in qualities.iter().enumerate() {
            for (j, q2) in qualities.iter().enumerate() {
                if i == j {
                    assert_eq!(q1, q2);
                } else {
                    assert_ne!(q1, q2);
                }
            }
        }
    }

    #[test]
    fn test_render_texture_new() {
        let texture_id = TextureId::new(123);
        let texture = RenderTexture::new(texture_id);

        assert_eq!(texture.texture_id, texture_id);
        assert_eq!(texture.fit, BoxFit::Contain);
        assert!(!texture.freeze);
        assert_eq!(texture.filter_quality, FilterQuality::Low);
    }

    #[test]
    fn test_render_texture_set_texture_id() {
        let mut texture = RenderTexture::new(TextureId::new(1));
        texture.set_texture_id(TextureId::new(2));

        assert_eq!(texture.texture_id.get(), 2);
    }

    #[test]
    fn test_render_texture_set_fit() {
        let mut texture = RenderTexture::new(TextureId::new(1));
        texture.set_fit(BoxFit::Cover);

        assert_eq!(texture.fit, BoxFit::Cover);
    }

    #[test]
    fn test_render_texture_set_freeze() {
        let mut texture = RenderTexture::new(TextureId::new(1));
        texture.set_freeze(true);

        assert!(texture.freeze);
    }

    #[test]
    fn test_render_texture_set_filter_quality() {
        let mut texture = RenderTexture::new(TextureId::new(1));
        texture.set_filter_quality(FilterQuality::High);

        assert_eq!(texture.filter_quality, FilterQuality::High);
    }

    #[test]
    fn test_render_texture_with_fit() {
        let texture = RenderTexture::new(TextureId::new(1)).with_fit(BoxFit::Fill);

        assert_eq!(texture.fit, BoxFit::Fill);
    }

    #[test]
    fn test_render_texture_frozen() {
        let texture = RenderTexture::new(TextureId::new(1)).frozen();

        assert!(texture.freeze);
    }

    #[test]
    fn test_render_texture_with_filter_quality() {
        let texture =
            RenderTexture::new(TextureId::new(1)).with_filter_quality(FilterQuality::Medium);

        assert_eq!(texture.filter_quality, FilterQuality::Medium);
    }

    #[test]
    fn test_calculate_dest_rect_fill() {
        let texture = RenderTexture::new(TextureId::new(1)).with_fit(BoxFit::Fill);
        let texture_size = Size::new(1920.0, 1080.0);
        let available_size = Size::new(800.0, 600.0);

        let rect = texture.calculate_dest_rect(texture_size, available_size);

        assert_eq!(rect.left(), 0.0);
        assert_eq!(rect.top(), 0.0);
        assert_eq!(rect.width(), 800.0);
        assert_eq!(rect.height(), 600.0);
    }

    #[test]
    fn test_calculate_dest_rect_contain() {
        let texture = RenderTexture::new(TextureId::new(1)).with_fit(BoxFit::Contain);
        let texture_size = Size::new(1920.0, 1080.0);
        let available_size = Size::new(800.0, 600.0);

        let rect = texture.calculate_dest_rect(texture_size, available_size);

        // Should be width-constrained (aspect ratio 16:9)
        assert_eq!(rect.width(), 800.0);
        assert!((rect.height() - 450.0).abs() < 0.1); // 800 * 9/16 = 450
        assert_eq!(rect.left(), 0.0);
        assert!((rect.top() - 75.0).abs() < 0.1); // Centered vertically
    }

    #[test]
    fn test_calculate_dest_rect_cover() {
        let texture = RenderTexture::new(TextureId::new(1)).with_fit(BoxFit::Cover);
        let texture_size = Size::new(1920.0, 1080.0);
        let available_size = Size::new(800.0, 600.0);

        let rect = texture.calculate_dest_rect(texture_size, available_size);

        // Should be height-constrained
        assert_eq!(rect.height(), 600.0);
        assert!((rect.width() - 1066.666).abs() < 0.1); // 600 * 16/9
        assert!((rect.left() + 133.333).abs() < 0.1); // Centered with overflow
        assert_eq!(rect.top(), 0.0);
    }

    #[test]
    fn test_calculate_dest_rect_none() {
        let texture = RenderTexture::new(TextureId::new(1)).with_fit(BoxFit::None);
        let texture_size = Size::new(200.0, 100.0);
        let available_size = Size::new(800.0, 600.0);

        let rect = texture.calculate_dest_rect(texture_size, available_size);

        // Should be original size, centered
        assert_eq!(rect.width(), 200.0);
        assert_eq!(rect.height(), 100.0);
        assert_eq!(rect.left(), 300.0); // (800 - 200) / 2
        assert_eq!(rect.top(), 250.0); // (600 - 100) / 2
    }

    #[test]
    fn test_calculate_dest_rect_scale_down_no_scaling() {
        let texture = RenderTexture::new(TextureId::new(1)).with_fit(BoxFit::ScaleDown);
        let texture_size = Size::new(200.0, 100.0);
        let available_size = Size::new(800.0, 600.0);

        let rect = texture.calculate_dest_rect(texture_size, available_size);

        // Should use original size (no scaling up)
        assert_eq!(rect.width(), 200.0);
        assert_eq!(rect.height(), 100.0);
    }

    #[test]
    fn test_calculate_dest_rect_scale_down_with_scaling() {
        let texture = RenderTexture::new(TextureId::new(1)).with_fit(BoxFit::ScaleDown);
        let texture_size = Size::new(1920.0, 1080.0);
        let available_size = Size::new(800.0, 600.0);

        let rect = texture.calculate_dest_rect(texture_size, available_size);

        // Should scale down like Contain
        assert_eq!(rect.width(), 800.0);
        assert!((rect.height() - 450.0).abs() < 0.1);
    }
}
