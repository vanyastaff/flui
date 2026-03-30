//! Web text system (estimated metrics for MVP)

use std::borrow::Cow;

use crate::traits::{Font, FontId, FontMetrics, FontRun, GlyphId, LineLayout, PlatformTextSystem};

pub struct WebTextSystem;

unsafe impl Send for WebTextSystem {}
unsafe impl Sync for WebTextSystem {}

impl WebTextSystem {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformTextSystem for WebTextSystem {
    fn add_fonts(&self, _fonts: Vec<Cow<'static, [u8]>>) -> anyhow::Result<()> {
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        vec![
            "sans-serif".into(),
            "serif".into(),
            "monospace".into(),
        ]
    }

    fn font_id(&self, _descriptor: &Font) -> anyhow::Result<FontId> {
        Ok(FontId(0))
    }

    fn font_metrics(&self, _font_id: FontId) -> FontMetrics {
        FontMetrics {
            units_per_em: 1000,
            ascent: 800.0,
            descent: 200.0,
            line_gap: 0.0,
            underline_position: -100.0,
            underline_thickness: 50.0,
            cap_height: 700.0,
            x_height: 500.0,
        }
    }

    fn glyph_for_char(&self, _font_id: FontId, ch: char) -> Option<GlyphId> {
        Some(GlyphId(ch as u32))
    }

    fn layout_line(&self, text: &str, font_size: f32, _runs: &[FontRun]) -> LineLayout {
        LineLayout {
            font_size,
            width: text.chars().count() as f32 * font_size * 0.6,
            ascent: font_size * 0.8,
            descent: font_size * 0.2,
            runs: Vec::new(),
            len: text.len(),
        }
    }
}
