//! `BackdropFilterLayer` — filters what is already painted behind it: frosted glass.

use flui_foundation::geometry::Rect;
use flui_painting::paint::{BlendMode, ImageFilter};

/// Layer that applies an image filter to backdrop content
///
/// # Architecture
///
/// ```text
/// Backdrop Content → Capture Texture → Apply Filter → Render Child → Composite
/// ```
///
/// # Rendering Process
///
/// 1. Capture current framebuffer content in specified bounds
/// 2. Apply image filter (blur, color adjustments, etc.) via GPU
/// 3. Render filtered backdrop to framebuffer
/// 4. Render child content on top (if present)
/// 5. Composite with blend mode
///
/// # Example
///
/// ```rust
/// use flui_layer::BackdropFilterLayer;
/// use flui_foundation::geometry::Rect;
/// use flui_painting::paint::{BlendMode, ImageFilter};
///
/// // Create frosted glass effect
/// let frosted_glass = BackdropFilterLayer::new(
///     ImageFilter::blur(10.0), // 10px gaussian blur
///     BlendMode::SrcOver,
///     Rect::from_xywh(0.0, 0.0, 400.0, 300.0),
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BackdropFilterLayer {
    /// Image filter to apply to backdrop
    filter: ImageFilter,

    /// Blend mode for compositing
    blend_mode: BlendMode,

    /// Bounds for backdrop capture (pre-computed for performance)
    bounds: Rect<f64>,
}

impl BackdropFilterLayer {
    /// Filters what is already painted behind `bounds` with `filter`, compositing with `blend_mode`.
    pub fn new(filter: ImageFilter, blend_mode: BlendMode, bounds: Rect<f64>) -> Self {
        Self {
            filter,
            blend_mode,
            bounds,
        }
    }

    /// The filter applied to the backdrop.
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// How the filtered backdrop composites.
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// The rectangle of backdrop captured and filtered.
    pub fn bounds(&self) -> Rect<f64> {
        self.bounds
    }
}
