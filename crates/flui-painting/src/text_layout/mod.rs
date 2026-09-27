//! Text shaping and layout over cosmic-text.
//!
//! - `font_resolve` — picking a family the host actually carries.
//! - `layout` — the process-wide font system, `TextLayout` (shape, truncate,
//!   caret/hit-test/line queries), and the style → `Attrs` mapping.
//! - `glyphs` — the placed-glyph and glyph-bitmap types a rasteriser reads.

use flui_types::geometry::{Pixels, Size, px};

pub(crate) mod font_resolve;
pub(crate) mod glyphs;
pub(crate) mod layout;

pub use glyphs::{GlyphContent, GlyphImage, GlyphKey, GlyphRasterizer, PlacedGlyph};
pub(crate) use layout::paint_color;
pub use layout::{ResolvedFont, Shaper, SharedFontSystem, TextLayout, shared_font_system};
// Test-support only: pinning the process-wide font system is irreversible, so
// it stays off the shipped surface. See its docs.
#[cfg(any(test, feature = "testing"))]
pub use layout::{font_system_initialized, init_font_system_with_faces};

/// Text layout result containing computed metrics.
#[derive(Debug, Clone)]
pub struct TextLayoutResult {
    /// Total width of the laid out text.
    pub width: f32,
    /// Total height of the laid out text.
    pub height: f32,
    /// Number of lines after layout.
    pub line_count: usize,
    /// Width of the longest line.
    pub max_line_width: f32,
    /// Distance to alphabetic baseline from top.
    pub alphabetic_baseline: f32,
    /// Distance to the ideographic baseline from top.
    ///
    /// Derived from the first line's descent edge (`line_top +
    /// line_height`) — the closest shaper-derived bound until per-font
    /// ideographic metrics are plumbed (cosmic-text does not expose
    /// them per run).
    pub ideographic_baseline: f32,
    /// Whether the layout was truncated to a maximum line count.
    pub truncated: bool,
}

impl TextLayoutResult {
    /// Returns the size as a `Size` struct.
    #[inline]
    #[must_use]
    pub fn size(&self) -> Size<Pixels> {
        Size::new(px(self.width), px(self.height))
    }
}
