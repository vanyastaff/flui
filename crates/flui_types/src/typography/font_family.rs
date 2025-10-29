//! Font family management for organizing fonts by weight and style.
//!
//! Similar to Flutter's font family system, this module allows grouping
//! multiple font files into a single family with different weights and styles.

use crate::typography::{FontData, FontProvider, FontResult, FontStyle, FontWeight};
use std::collections::HashMap;
use std::sync::Arc;

/// A collection of fonts that make up a font family.
///
/// Similar to Flutter's font family concept, where you can have multiple
/// font files for different weights (bold, regular, light) and styles (italic, normal).
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::typography::{FontFamily, MemoryFont, FontWeight, FontStyle};
///
/// let mut family = FontFamily::new("Roboto");
///
/// // Add regular variant
/// family.add_font(MemoryFont::new(regular_bytes), FontWeight::W400, FontStyle::Normal);
///
/// // Add bold variant
/// family.add_font(MemoryFont::new(bold_bytes), FontWeight::W700, FontStyle::Normal);
///
/// // Add italic variant
/// family.add_font(MemoryFont::new(italic_bytes), FontWeight::W400, FontStyle::Italic);
/// ```
#[derive(Clone)]
pub struct FontFamily {
    /// Name of the font family (e.g., "Roboto", "Open Sans")
    name: String,
    /// Map of (weight, style) -> FontProvider
    fonts: HashMap<(FontWeight, FontStyle), Arc<dyn FontProvider>>,
    /// Default font to use when specific weight/style not found
    default_font: Option<Arc<dyn FontProvider>>,
}

impl FontFamily {
    /// Creates a new font family with the given name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the font family (e.g., "Roboto")
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fonts: HashMap::new(),
            default_font: None,
        }
    }

    /// Returns the name of this font family.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Adds a font variant to this family.
    ///
    /// # Arguments
    ///
    /// * `provider` - The font provider (MemoryFont, AssetFont, FileFont)
    /// * `weight` - The font weight (W100-W900)
    /// * `style` - The font style (Normal or Italic)
    pub fn add_font(
        &mut self,
        provider: impl FontProvider + 'static,
        weight: FontWeight,
        style: FontStyle,
    ) {
        self.fonts.insert((weight, style), Arc::new(provider));
    }

    /// Sets the default font to use when a specific weight/style is not found.
    ///
    /// This is useful for fallback behavior.
    pub fn set_default(&mut self, provider: impl FontProvider + 'static) {
        self.default_font = Some(Arc::new(provider));
    }

    /// Gets a font for the specified weight and style.
    ///
    /// Returns the closest matching font, using fallback rules:
    /// 1. Exact match (weight + style)
    /// 2. Same weight, Normal style (if Italic was requested)
    /// 3. Normal weight (W400), same style
    /// 4. Normal weight (W400), Normal style
    /// 5. Default font
    /// 6. Any font in the family
    ///
    /// # Arguments
    ///
    /// * `weight` - Desired font weight
    /// * `style` - Desired font style
    pub fn font(&self, weight: FontWeight, style: FontStyle) -> Option<Arc<dyn FontProvider>> {
        // 1. Try exact match
        if let Some(font) = self.fonts.get(&(weight, style)) {
            return Some(Arc::clone(font));
        }

        // 2. Try same weight, normal style (if italic was requested)
        if style == FontStyle::Italic
            && let Some(font) = self.fonts.get(&(weight, FontStyle::Normal))
        {
            return Some(Arc::clone(font));
        }

        // 3. Try normal weight (W400), same style
        if weight != FontWeight::W400
            && let Some(font) = self.fonts.get(&(FontWeight::W400, style))
        {
            return Some(Arc::clone(font));
        }

        // 4. Try normal weight (W400), normal style
        if (weight != FontWeight::W400 || style != FontStyle::Normal)
            && let Some(font) = self.fonts.get(&(FontWeight::W400, FontStyle::Normal))
        {
            return Some(Arc::clone(font));
        }

        // 5. Try default font
        if let Some(ref default) = self.default_font {
            return Some(Arc::clone(default));
        }

        // 6. Return any font
        self.fonts.values().next().map(Arc::clone)
    }

    /// Loads font data for the specified weight and style.
    ///
    /// This is a convenience method that gets the font provider and loads it.
    ///
    /// # Arguments
    ///
    /// * `weight` - Desired font weight
    /// * `style` - Desired font style
    pub async fn load(&self, weight: FontWeight, style: FontStyle) -> FontResult<FontData> {
        let provider = self.font(weight, style).ok_or_else(|| {
            crate::typography::FontError::NotFound(format!(
                "No font found for family '{}' with weight {:?} and style {:?}",
                self.name, weight, style
            ))
        })?;

        provider.load().await
    }

    /// Returns the number of font variants in this family.
    pub fn variant_count(&self) -> usize {
        self.fonts.len()
    }

    /// Returns whether this family has a specific variant.
    pub fn has_variant(&self, weight: FontWeight, style: FontStyle) -> bool {
        self.fonts.contains_key(&(weight, style))
    }

    /// Returns all available variants as (weight, style) pairs.
    pub fn variants(&self) -> Vec<(FontWeight, FontStyle)> {
        self.fonts.keys().copied().collect()
    }
}

impl std::fmt::Debug for FontFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFamily")
            .field("name", &self.name)
            .field("variant_count", &self.fonts.len())
            .field("has_default", &self.default_font.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typography::MemoryFont;

    fn create_test_font() -> MemoryFont {
        MemoryFont::new(vec![0, 1, 0, 0]) // Minimal TTF signature
    }

    #[test]
    fn test_font_family_creation() {
        let family = FontFamily::new("Roboto");
        assert_eq!(family.name(), "Roboto");
        assert_eq!(family.variant_count(), 0);
    }

    #[test]
    fn test_add_font_variant() {
        let mut family = FontFamily::new("Roboto");

        family.add_font(create_test_font(), FontWeight::W400, FontStyle::Normal);
        family.add_font(create_test_font(), FontWeight::W700, FontStyle::Normal);

        assert_eq!(family.variant_count(), 2);
        assert!(family.has_variant(FontWeight::W400, FontStyle::Normal));
        assert!(family.has_variant(FontWeight::W700, FontStyle::Normal));
        assert!(!family.has_variant(FontWeight::W400, FontStyle::Italic));
    }

    #[test]
    fn test_get_font_exact_match() {
        let mut family = FontFamily::new("Roboto");
        family.add_font(create_test_font(), FontWeight::W400, FontStyle::Normal);

        let font = family.font(FontWeight::W400, FontStyle::Normal);
        assert!(font.is_some());
    }

    #[test]
    fn test_get_font_fallback_to_normal_style() {
        let mut family = FontFamily::new("Roboto");
        family.add_font(create_test_font(), FontWeight::W700, FontStyle::Normal);

        // Request italic, should fallback to normal
        let font = family.font(FontWeight::W700, FontStyle::Italic);
        assert!(font.is_some());
    }

    #[test]
    fn test_get_font_fallback_to_normal_weight() {
        let mut family = FontFamily::new("Roboto");
        family.add_font(create_test_font(), FontWeight::W400, FontStyle::Normal);

        // Request bold, should fallback to W400
        let font = family.font(FontWeight::W700, FontStyle::Normal);
        assert!(font.is_some());
    }

    #[test]
    fn test_get_font_with_default() {
        let mut family = FontFamily::new("Roboto");
        family.set_default(create_test_font());

        // No variants added, should return default
        let font = family.font(FontWeight::W400, FontStyle::Normal);
        assert!(font.is_some());
    }

    #[test]
    fn test_variants_list() {
        let mut family = FontFamily::new("Roboto");
        family.add_font(create_test_font(), FontWeight::W400, FontStyle::Normal);
        family.add_font(create_test_font(), FontWeight::W700, FontStyle::Normal);
        family.add_font(create_test_font(), FontWeight::W400, FontStyle::Italic);

        let variants = family.variants();
        assert_eq!(variants.len(), 3);
        assert!(variants.contains(&(FontWeight::W400, FontStyle::Normal)));
        assert!(variants.contains(&(FontWeight::W700, FontStyle::Normal)));
        assert!(variants.contains(&(FontWeight::W400, FontStyle::Italic)));
    }

    #[tokio::test]
    async fn test_load_font() {
        let mut family = FontFamily::new("Roboto");
        family.add_font(create_test_font(), FontWeight::W400, FontStyle::Normal);

        let result = family.load(FontWeight::W400, FontStyle::Normal).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_font_not_found() {
        let family = FontFamily::new("Roboto");
        // No fonts added

        let result = family.load(FontWeight::W400, FontStyle::Normal).await;
        assert!(result.is_err());
    }
}
