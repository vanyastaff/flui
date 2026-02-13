//! PlatformTextSystem trait contract — text/font API surface.
//!
//! Design contract for the implementation phase.

use std::borrow::Cow;

// --- Font Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphId(pub u32);

#[derive(Debug, Clone)]
pub struct Font {
    pub family: String,
    pub weight: FontWeight,
    pub style: FontStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    Thin,
    Light,
    #[default]
    Normal,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
    Black,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy)]
pub struct FontRun {
    pub font_id: FontId,
    pub len: usize,
}

// --- Metrics ---

#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub cap_height: f32,
    pub x_height: f32,
}

#[derive(Debug, Clone)]
pub struct LineLayout {
    pub font_size: f32,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub runs: Vec<ShapedRun>,
    pub len: usize,
}

#[derive(Debug, Clone)]
pub struct ShapedRun {
    pub font_id: FontId,
    pub glyphs: Vec<ShapedGlyph>,
}

#[derive(Debug, Clone, Copy)]
pub struct ShapedGlyph {
    pub id: GlyphId,
    pub position_x: f32,
    pub position_y: f32,
    pub index: usize,
}

// --- Trait ---

pub trait PlatformTextSystem: Send + Sync {
    /// Load font data from bytes.
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> anyhow::Result<()>;

    /// List all available font family names.
    fn all_font_names(&self) -> Vec<String>;

    /// Resolve a font descriptor to a FontId.
    fn font_id(&self, descriptor: &Font) -> anyhow::Result<FontId>;

    /// Get metrics for a loaded font.
    fn font_metrics(&self, font_id: FontId) -> FontMetrics;

    /// Map a character to its glyph ID in a font.
    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId>;

    /// Layout a single line of text with font runs.
    fn layout_line(&self, text: &str, font_size: f32, runs: &[FontRun]) -> LineLayout;
}
