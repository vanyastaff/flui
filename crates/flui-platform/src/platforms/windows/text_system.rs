//! DirectWrite text system implementation
//!
//! Uses Windows DirectWrite API (IDWriteFactory5) for font enumeration,
//! text measurement, and glyph shaping. Requires Windows 10 1703+.

use crate::traits::{
    Font, FontId, FontMetrics, FontRun, FontStyle, FontWeight, GlyphId, LineLayout,
    PlatformTextSystem, ShapedRun,
};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::borrow::Cow;
use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::DirectWrite::*;

/// Font face info stored per FontId
struct FontInfo {
    family: String,
    font_face: IDWriteFontFace3,
    weight: DWRITE_FONT_WEIGHT,
    style: DWRITE_FONT_STYLE,
}

/// DirectWrite-based text system for Windows
///
/// Provides font enumeration, metrics, glyph lookup, and text layout
/// using the DirectWrite API. Each `FontId` maps to a cached `IDWriteFontFace3`.
pub struct DirectWriteTextSystem {
    factory: IDWriteFactory5,
    system_collection: IDWriteFontCollection1,
    locale: String,
    state: Mutex<DirectWriteState>,
}

struct DirectWriteState {
    fonts: Vec<FontInfo>,
}

// SAFETY: DirectWrite COM objects are thread-safe (apartment-threaded COM initialized).
// The factory and collections are immutable after creation. Font face access is
// synchronized via the Mutex on DirectWriteState.
unsafe impl Send for DirectWriteTextSystem {}
unsafe impl Sync for DirectWriteTextSystem {}

impl DirectWriteTextSystem {
    /// Create a new DirectWrite text system
    ///
    /// Initializes IDWriteFactory5 and loads the system font collection.
    pub fn new() -> Result<Self> {
        unsafe {
            // Create DirectWrite factory
            let factory: IDWriteFactory5 =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).context("DWriteCreateFactory")?;

            // Get system font collection
            let mut collection_ptr = std::mem::zeroed();
            factory
                .GetSystemFontCollection(false, &mut collection_ptr, true)
                .context("GetSystemFontCollection")?;
            let system_collection: IDWriteFontCollection1 = collection_ptr
                .ok_or_else(|| anyhow::anyhow!("System font collection is null"))?
                .cast()
                .context("Cast to IDWriteFontCollection1")?;

            // Get user locale
            let locale = get_user_locale();

            tracing::info!("DirectWrite text system initialized (locale: {})", locale);

            Ok(Self {
                factory,
                system_collection,
                locale,
                state: Mutex::new(DirectWriteState { fonts: Vec::new() }),
            })
        }
    }

    /// Resolve or create a font face, returning its FontId
    fn resolve_font(&self, family: &str, weight: FontWeight, style: FontStyle) -> Result<FontId> {
        let dw_weight = to_dwrite_weight(weight);
        let dw_style = to_dwrite_style(style);

        // Check if already cached
        {
            let state = self.state.lock();
            for (idx, info) in state.fonts.iter().enumerate() {
                if info.family == family && info.weight == dw_weight && info.style == dw_style {
                    return Ok(FontId(idx));
                }
            }
        }

        // Resolve via DirectWrite
        let font_face = unsafe {
            let family_name = HSTRING::from(family);
            let mut family_index = 0u32;
            let mut exists = false.into();

            self.system_collection
                .FindFamilyName(&family_name, &mut family_index, &mut exists)
                .context("FindFamilyName")?;

            if !exists.as_bool() {
                return Err(anyhow::anyhow!("Font family '{}' not found", family));
            }

            let font_family = self
                .system_collection
                .GetFontFamily(family_index)
                .context("GetFontFamily")?;

            let font = font_family
                .GetFirstMatchingFont(dw_weight, DWRITE_FONT_STRETCH_NORMAL, dw_style)
                .context("GetFirstMatchingFont")?;

            let font_face: IDWriteFontFace = font.CreateFontFace().context("CreateFontFace")?;
            let font_face3: IDWriteFontFace3 =
                font_face.cast().context("Cast to IDWriteFontFace3")?;
            font_face3
        };

        let mut state = self.state.lock();
        let id = state.fonts.len();
        state.fonts.push(FontInfo {
            family: family.to_string(),
            font_face,
            weight: dw_weight,
            style: dw_style,
        });
        Ok(FontId(id))
    }
}

impl PlatformTextSystem for DirectWriteTextSystem {
    fn add_fonts(&self, _fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        // Custom font loading deferred to post-MVP (requires IDWriteInMemoryFontFileLoader)
        tracing::warn!("add_fonts: custom font loading not yet implemented");
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        unsafe {
            let count = self.system_collection.GetFontFamilyCount();
            for i in 0..count {
                if let Ok(family) = self.system_collection.GetFontFamily(i) {
                    if let Ok(localized_names) = family.GetFamilyNames() {
                        if let Some(name) = get_localized_name(&localized_names, &self.locale) {
                            names.push(name);
                        }
                    }
                }
            }
        }
        names
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        self.resolve_font(&descriptor.family, descriptor.weight, descriptor.style)
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        let state = self.state.lock();
        let info = match state.fonts.get(font_id.0) {
            Some(info) => info,
            None => {
                tracing::error!("Invalid FontId: {}", font_id.0);
                return FontMetrics {
                    units_per_em: 1000,
                    ascent: 800.0,
                    descent: 200.0,
                    line_gap: 0.0,
                    underline_position: -100.0,
                    underline_thickness: 50.0,
                    cap_height: 700.0,
                    x_height: 500.0,
                };
            }
        };

        unsafe {
            let mut metrics = std::mem::zeroed::<DWRITE_FONT_METRICS1>();
            info.font_face
                .GetMetrics(&mut metrics as *mut DWRITE_FONT_METRICS1);

            FontMetrics {
                units_per_em: metrics.Base.designUnitsPerEm,
                ascent: metrics.Base.ascent as f32,
                descent: metrics.Base.descent as f32,
                line_gap: metrics.Base.lineGap as f32,
                underline_position: metrics.Base.underlinePosition as f32,
                underline_thickness: metrics.Base.underlineThickness as f32,
                cap_height: metrics.Base.capHeight as f32,
                x_height: metrics.Base.xHeight as f32,
            }
        }
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        let state = self.state.lock();
        let info = state.fonts.get(font_id.0)?;

        unsafe {
            let codepoints = [ch as u32];
            let mut glyph_indices = [0u16; 1];

            info.font_face
                .GetGlyphIndices(codepoints.as_ptr(), 1, glyph_indices.as_mut_ptr())
                .ok()?;

            if glyph_indices[0] == 0 {
                None // Glyph index 0 = .notdef (missing glyph)
            } else {
                Some(GlyphId(glyph_indices[0] as u32))
            }
        }
    }

    fn layout_line(&self, text: &str, font_size: f32, runs: &[FontRun]) -> LineLayout {
        if text.is_empty() {
            return LineLayout {
                font_size,
                width: 0.0,
                ascent: 0.0,
                descent: 0.0,
                runs: Vec::new(),
                len: 0,
            };
        }

        // Convert text to UTF-16 for DirectWrite
        let text_wide: Vec<u16> = text.encode_utf16().collect();

        let state = self.state.lock();

        // Get font info for the first run (or default)
        let first_font = if !runs.is_empty() {
            state.fonts.get(runs[0].font_id.0)
        } else {
            state.fonts.first()
        };

        let (family_name, dw_weight, dw_style) = match first_font {
            Some(info) => (info.family.as_str(), info.weight, info.style),
            None => {
                // Fallback: approximate layout
                let char_count = text.chars().count() as f32;
                return LineLayout {
                    font_size,
                    width: char_count * font_size * 0.6,
                    ascent: font_size * 0.8,
                    descent: font_size * 0.2,
                    runs: Vec::new(),
                    len: text.len(),
                };
            }
        };

        // Create text format and layout
        let result = unsafe {
            self.create_layout(
                &text_wide,
                family_name,
                dw_weight,
                dw_style,
                font_size,
                text,
                runs,
            )
        };

        match result {
            Ok(layout) => layout,
            Err(e) => {
                tracing::error!("DirectWrite layout_line failed: {:?}", e);
                let char_count = text.chars().count() as f32;
                LineLayout {
                    font_size,
                    width: char_count * font_size * 0.6,
                    ascent: font_size * 0.8,
                    descent: font_size * 0.2,
                    runs: Vec::new(),
                    len: text.len(),
                }
            }
        }
    }
}

impl DirectWriteTextSystem {
    /// Create an IDWriteTextLayout and extract metrics
    ///
    /// # Safety
    /// Caller must ensure `text_wide` is valid UTF-16 and font info is valid.
    #[allow(clippy::too_many_arguments)]
    unsafe fn create_layout(
        &self,
        text_wide: &[u16],
        family_name: &str,
        weight: DWRITE_FONT_WEIGHT,
        style: DWRITE_FONT_STYLE,
        font_size: f32,
        text: &str,
        runs: &[FontRun],
    ) -> Result<LineLayout> {
        let family_hstring = HSTRING::from(family_name);
        let locale_hstring = HSTRING::from(&self.locale);

        // Create text format
        let format = self
            .factory
            .CreateTextFormat(
                &family_hstring,
                None, // Use default font collection
                weight,
                style,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                &locale_hstring,
            )
            .context("CreateTextFormat")?;

        // Create text layout
        let layout = self
            .factory
            .CreateTextLayout(text_wide, &format, f32::MAX, f32::MAX)
            .context("CreateTextLayout")?;

        // Apply per-run font settings if multiple runs
        if runs.len() > 1 {
            let state = self.state.lock();
            let mut utf16_offset = 0u32;
            let mut byte_offset = 0usize;

            for run in runs {
                let run_text = &text[byte_offset..byte_offset + run.len];
                let run_utf16_len = run_text.encode_utf16().count() as u32;

                if let Some(run_info) = state.fonts.get(run.font_id.0) {
                    let range = DWRITE_TEXT_RANGE {
                        startPosition: utf16_offset,
                        length: run_utf16_len,
                    };
                    let run_family = HSTRING::from(&run_info.family);
                    let _ = layout.SetFontFamilyName(&run_family, range);
                    let _ = layout.SetFontWeight(run_info.weight, range);
                    let _ = layout.SetFontStyle(run_info.style, range);
                    let _ = layout.SetFontSize(font_size, range);
                }

                utf16_offset += run_utf16_len;
                byte_offset += run.len;
            }
        }

        // Get overall metrics
        let mut metrics = std::mem::zeroed::<DWRITE_TEXT_METRICS>();
        layout.GetMetrics(&mut metrics).context("GetMetrics")?;

        // Get line metrics for ascent/descent
        let mut line_metrics = [DWRITE_LINE_METRICS::default(); 4];
        let mut line_count = 0u32;
        layout
            .GetLineMetrics(Some(&mut line_metrics), &mut line_count)
            .context("GetLineMetrics")?;

        let (ascent, descent) = if line_count > 0 {
            let lm = &line_metrics[0];
            (lm.baseline, lm.height - lm.baseline)
        } else {
            (font_size * 0.8, font_size * 0.2)
        };

        // Build shaped runs from the font runs
        let shaped_runs = build_shaped_runs(runs, text);

        Ok(LineLayout {
            font_size,
            width: metrics.width,
            ascent,
            descent,
            runs: shaped_runs,
            len: text.len(),
        })
    }
}

/// Build ShapedRun entries from FontRun specifications
///
/// For MVP, we don't extract individual glyph positions from DirectWrite's
/// text renderer callback. Instead, we create runs with empty glyph lists.
/// The accurate width/ascent/descent from GetMetrics() is what matters for layout.
fn build_shaped_runs(runs: &[FontRun], _text: &str) -> Vec<ShapedRun> {
    runs.iter()
        .map(|run| ShapedRun {
            font_id: run.font_id,
            glyphs: Vec::new(), // Glyph extraction deferred to post-MVP
        })
        .collect()
}

// ==================== Helper Functions ====================

/// Convert FontWeight to DirectWrite DWRITE_FONT_WEIGHT
fn to_dwrite_weight(weight: FontWeight) -> DWRITE_FONT_WEIGHT {
    DWRITE_FONT_WEIGHT(weight.to_numeric() as i32)
}

/// Convert FontStyle to DirectWrite DWRITE_FONT_STYLE
fn to_dwrite_style(style: FontStyle) -> DWRITE_FONT_STYLE {
    match style {
        FontStyle::Normal => DWRITE_FONT_STYLE_NORMAL,
        FontStyle::Italic => DWRITE_FONT_STYLE_ITALIC,
        FontStyle::Oblique => DWRITE_FONT_STYLE_OBLIQUE,
    }
}

/// Get the user's default locale name (e.g., "en-US")
fn get_user_locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;

    unsafe {
        let mut buffer = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
        let len = GetUserDefaultLocaleName(&mut buffer);
        if len > 0 {
            String::from_utf16_lossy(&buffer[..len as usize - 1]) // Exclude null terminator
        } else {
            "en-US".to_string()
        }
    }
}

/// Extract a localized font name, preferring the given locale
fn get_localized_name(names: &IDWriteLocalizedStrings, preferred_locale: &str) -> Option<String> {
    unsafe {
        let count = names.GetCount();
        if count == 0 {
            return None;
        }

        // Try to find preferred locale
        let locale_hstring = HSTRING::from(preferred_locale);
        let mut index = 0u32;
        let mut exists = false.into();
        let _ = names.FindLocaleName(&locale_hstring, &mut index, &mut exists);

        // Fall back to first entry if preferred locale not found
        let use_index = if exists.as_bool() { index } else { 0 };

        // Get string length
        let length = names.GetStringLength(use_index).ok()?;

        // Get string data
        let mut buffer = vec![0u16; length as usize + 1];
        names.GetString(use_index, &mut buffer).ok()?;

        // Trim null terminator
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directwrite_creation() {
        // Requires COM to be initialized (WindowsPlatform::new() does this)
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let result = DirectWriteTextSystem::new();
        assert!(
            result.is_ok(),
            "Failed to create DirectWrite: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_all_font_names() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let ts = DirectWriteTextSystem::new().expect("Failed to create DirectWrite");
        let names = ts.all_font_names();
        assert!(!names.is_empty(), "No fonts found");
        assert!(
            names.iter().any(|n| n == "Segoe UI"),
            "Segoe UI not found in: {:?}",
            &names[..names.len().min(10)]
        );
    }

    #[test]
    fn test_font_id_resolution() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let ts = DirectWriteTextSystem::new().expect("Failed to create DirectWrite");
        let font = Font {
            family: "Segoe UI".to_string(),
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
        };
        let id = ts.font_id(&font);
        assert!(id.is_ok(), "Failed to resolve Segoe UI: {:?}", id.err());

        // Resolve same font again — should return cached ID
        let id2 = ts.font_id(&font).unwrap();
        assert_eq!(id.unwrap(), id2);
    }

    #[test]
    fn test_font_metrics() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let ts = DirectWriteTextSystem::new().expect("Failed to create DirectWrite");
        let font = Font {
            family: "Segoe UI".to_string(),
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
        };
        let id = ts.font_id(&font).unwrap();
        let metrics = ts.font_metrics(id);

        assert!(metrics.units_per_em > 0, "units_per_em should be > 0");
        assert!(metrics.ascent > 0.0, "ascent should be > 0");
        assert!(metrics.descent > 0.0, "descent should be > 0");
    }

    #[test]
    fn test_glyph_for_char() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let ts = DirectWriteTextSystem::new().expect("Failed to create DirectWrite");
        let font = Font {
            family: "Segoe UI".to_string(),
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
        };
        let id = ts.font_id(&font).unwrap();

        // 'A' should have a glyph in Segoe UI
        let glyph = ts.glyph_for_char(id, 'A');
        assert!(glyph.is_some(), "Expected glyph for 'A'");
        assert!(glyph.unwrap().0 > 0, "Glyph ID should be > 0");
    }

    #[test]
    fn test_layout_line() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let ts = DirectWriteTextSystem::new().expect("Failed to create DirectWrite");
        let font = Font {
            family: "Segoe UI".to_string(),
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
        };
        let id = ts.font_id(&font).unwrap();

        let layout = ts.layout_line(
            "Hello, World!",
            16.0,
            &[FontRun {
                font_id: id,
                len: 13,
            }],
        );

        assert!(
            layout.width > 0.0,
            "Width should be > 0, got {}",
            layout.width
        );
        assert!(layout.ascent > 0.0, "Ascent should be > 0");
        assert!(layout.descent > 0.0, "Descent should be > 0");
        assert_eq!(layout.len, 13);
        assert_eq!(layout.font_size, 16.0);
    }

    #[test]
    fn test_layout_empty_string() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let ts = DirectWriteTextSystem::new().expect("Failed to create DirectWrite");
        let layout = ts.layout_line("", 16.0, &[]);

        assert_eq!(layout.width, 0.0);
        assert_eq!(layout.len, 0);
    }

    #[test]
    fn test_font_not_found() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let ts = DirectWriteTextSystem::new().expect("Failed to create DirectWrite");
        let font = Font {
            family: "NonExistentFontFamily12345".to_string(),
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
        };
        let result = ts.font_id(&font);
        assert!(result.is_err());
    }
}
