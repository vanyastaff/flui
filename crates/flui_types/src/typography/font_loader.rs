//! Global font loader for registering and managing font families.
//!
//! Similar to Flutter's `FontLoader`, this module provides a global registry
//! for font families that can be used throughout the application.

use crate::typography::{
    FontData, FontError, FontFamily, FontProvider, FontResult, FontStyle, FontWeight,
};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Cache key for loaded fonts.
type FontCacheKey = (String, FontWeight, FontStyle); // (family_name, weight, style)

/// Global font loader instance.
///
/// This provides a singleton registry for all font families in the application.
/// Similar to Flutter's approach, fonts are registered with a family name and
/// can then be referenced in `TextStyle` via `font_family`.
///
/// Fonts are loaded once and cached in memory for performance.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::typography::{FontLoader, FontFamily, MemoryFont, FontWeight, FontStyle};
///
/// // Register a font family
/// let mut family = FontFamily::new("Roboto");
/// family.add_font(MemoryFont::new(regular_bytes), FontWeight::W400, FontStyle::Normal);
/// family.add_font(MemoryFont::new(bold_bytes), FontWeight::W700, FontStyle::Normal);
///
/// FontLoader::register_family(family);
///
/// // Later, load the font (will be cached after first load)
/// let font_data = FontLoader::load("Roboto", FontWeight::W400, FontStyle::Normal).await?;
/// ```
pub struct FontLoader {
    families: RwLock<HashMap<String, Arc<FontFamily>>>,
    /// Cache of loaded font data: (family, weight, style) -> FontData
    font_cache: RwLock<HashMap<FontCacheKey, Arc<FontData>>>,
}

impl FontLoader {
    /// Creates a new font loader.
    fn new() -> Self {
        Self {
            families: RwLock::new(HashMap::new()),
            font_cache: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the global font loader instance.
    pub fn global() -> &'static Self {
        static INSTANCE: Lazy<FontLoader> = Lazy::new(FontLoader::new);
        &INSTANCE
    }

    /// Registers a font family.
    ///
    /// # Arguments
    ///
    /// * `family` - The font family to register
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::typography::{FontLoader, FontFamily, MemoryFont, FontWeight, FontStyle};
    ///
    /// let mut family = FontFamily::new("Roboto");
    /// family.add_font(MemoryFont::new(regular_bytes), FontWeight::W400, FontStyle::Normal);
    ///
    /// FontLoader::register_family(family);
    /// ```
    pub fn register_family(family: FontFamily) {
        let name = family.name().to_string();
        Self::global()
            .families
            .write()
            .unwrap()
            .insert(name, Arc::new(family));
    }

    /// Registers a single font as a new family.
    ///
    /// This is a convenience method for registering a simple font family
    /// with only one variant.
    ///
    /// # Arguments
    ///
    /// * `family_name` - The name to use for this font family
    /// * `provider` - The font provider
    /// * `weight` - The font weight
    /// * `style` - The font style
    pub fn register_font(
        family_name: impl Into<String>,
        provider: impl FontProvider + 'static,
        weight: FontWeight,
        style: FontStyle,
    ) {
        let mut family = FontFamily::new(family_name);
        family.add_font(provider, weight, style);
        Self::register_family(family);
    }

    /// Gets a registered font family.
    ///
    /// # Arguments
    ///
    /// * `family_name` - The name of the font family
    ///
    /// # Returns
    ///
    /// The font family, or None if not found
    pub fn family(family_name: &str) -> Option<Arc<FontFamily>> {
        Self::global()
            .families
            .read()
            .unwrap()
            .get(family_name)
            .cloned()
    }

    /// Checks if a font family is registered.
    ///
    /// # Arguments
    ///
    /// * `family_name` - The name of the font family
    pub fn has_family(family_name: &str) -> bool {
        Self::global()
            .families
            .read()
            .unwrap()
            .contains_key(family_name)
    }

    /// Loads font data for a specific family, weight, and style.
    ///
    /// This is a convenience method that gets the family and loads the font.
    /// Fonts are cached after first load for performance.
    ///
    /// # Arguments
    ///
    /// * `family_name` - The name of the font family
    /// * `weight` - Desired font weight
    /// * `style` - Desired font style
    ///
    /// # Returns
    ///
    /// The loaded font data, or an error if the family is not found or loading fails
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_types::typography::{FontLoader, FontWeight, FontStyle};
    ///
    /// let font_data = FontLoader::load("Roboto", FontWeight::W400, FontStyle::Normal).await?;
    /// ```
    pub async fn load(
        family_name: &str,
        weight: FontWeight,
        style: FontStyle,
    ) -> FontResult<FontData> {
        let cache_key = (family_name.to_string(), weight, style);

        // Check cache first
        {
            let cache = Self::global().font_cache.read().unwrap();
            if let Some(cached) = cache.get(&cache_key) {
                // Return cached font data (cheap Arc clone)
                return Ok((**cached).clone());
            }
        }

        // Not in cache, load it
        let family = Self::family(family_name).ok_or_else(|| {
            FontError::NotFound(format!("Font family '{}' not registered", family_name))
        })?;

        let font_data = family.load(weight, style).await?;

        // Cache the loaded font data
        {
            let mut cache = Self::global().font_cache.write().unwrap();
            cache.insert(cache_key, Arc::new(font_data.clone()));
        }

        Ok(font_data)
    }

    /// Returns a list of all registered font family names.
    pub fn list_families() -> Vec<String> {
        Self::global()
            .families
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// Unregisters a font family.
    ///
    /// # Arguments
    ///
    /// * `family_name` - The name of the font family to unregister
    ///
    /// # Returns
    ///
    /// true if the family was found and removed, false otherwise
    pub fn unregister_family(family_name: &str) -> bool {
        Self::global()
            .families
            .write()
            .unwrap()
            .remove(family_name)
            .is_some()
    }

    /// Clears the font cache.
    ///
    /// This removes all cached font data but keeps family registrations.
    /// Useful if you want to free memory but keep fonts registered.
    pub fn clear_cache() {
        Self::global().font_cache.write().unwrap().clear();
    }

    /// Clears all registered font families and cache.
    ///
    /// This is mainly useful for testing.
    pub fn clear_all() {
        Self::global().families.write().unwrap().clear();

        Self::global().font_cache.write().unwrap().clear();
    }

    /// Returns the number of registered font families.
    pub fn family_count() -> usize {
        Self::global().families.read().unwrap().len()
    }

    /// Automatically registers all fonts from a directory.
    ///
    /// Scans the directory for .ttf and .otf files and registers them.
    /// Tries to extract family name, weight, and style from font metadata.
    ///
    /// # Arguments
    ///
    /// * `fonts_dir` - Path to the fonts directory (e.g., "assets/fonts")
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Register all fonts from assets/fonts
    /// FontLoader::register_from_directory("assets/fonts").await?;
    /// ```
    pub async fn register_from_directory(
        fonts_dir: impl AsRef<std::path::Path>,
    ) -> FontResult<usize> {
        use tokio::fs;

        let fonts_dir = fonts_dir.as_ref();
        let mut registered_count = 0;

        // Read directory
        let mut entries = fs::read_dir(fonts_dir)
            .await
            .map_err(|e| FontError::LoadFailed(format!("Failed to read fonts directory: {}", e)))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| FontError::LoadFailed(format!("Failed to read directory entry: {}", e)))?
        {
            let path = entry.path();

            // Check if it's a font file
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if ext == "ttf" || ext == "otf" {
                    // Try to register this font
                    match Self::register_font_file(&path).await {
                        Ok((family_name, weight, style)) => {
                            #[cfg(feature = "tracing")]
                            tracing::info!(
                                "Registered font: {} ({:?}, {:?}) from {}",
                                family_name,
                                weight,
                                style,
                                path.display()
                            );

                            #[cfg(not(feature = "tracing"))]
                            println!(
                                "Registered font: {} ({:?}, {:?}) from {}",
                                family_name,
                                weight,
                                style,
                                path.display()
                            );

                            registered_count += 1;
                        }
                        Err(e) => {
                            #[cfg(feature = "tracing")]
                            tracing::warn!("Failed to register font {}: {}", path.display(), e);

                            #[cfg(not(feature = "tracing"))]
                            eprintln!("Warning: Failed to register font {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        Ok(registered_count)
    }

    /// Registers a single font file, extracting metadata.
    async fn register_font_file(
        path: &std::path::Path,
    ) -> FontResult<(String, FontWeight, FontStyle)> {
        use crate::typography::FileFont;

        // Load font data to extract metadata
        let provider = FileFont::new(path);
        let font_data = provider.load().await?;

        // Parse font to extract family name, weight, style
        let face = ttf_parser::Face::parse(font_data.as_bytes(), 0)
            .map_err(|_| FontError::ParseFailed("Failed to parse font file".to_string()))?;

        // Extract family name
        let family_name = face
            .names()
            .into_iter()
            .find(|name| name.name_id == ttf_parser::name_id::FAMILY)
            .and_then(|name| name.to_string())
            .unwrap_or_else(|| {
                // Fallback to filename
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .to_string()
            });

        // Try to determine weight and style from font metadata
        let weight = font_data.weight.unwrap_or_else(|| {
            // Try to guess from filename
            let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let lower = filename.to_lowercase();

            if lower.contains("thin") || lower.contains("100") {
                FontWeight::W100
            } else if lower.contains("extralight") || lower.contains("200") {
                FontWeight::W200
            } else if lower.contains("light") || lower.contains("300") {
                FontWeight::W300
            } else if lower.contains("medium") || lower.contains("500") {
                FontWeight::W500
            } else if lower.contains("semibold") || lower.contains("600") {
                FontWeight::W600
            } else if lower.contains("bold") || lower.contains("700") {
                FontWeight::W700
            } else if lower.contains("extrabold") || lower.contains("800") {
                FontWeight::W800
            } else if lower.contains("black") || lower.contains("900") {
                FontWeight::W900
            } else {
                FontWeight::W400 // Default to normal
            }
        });

        let style = font_data.style.unwrap_or_else(|| {
            // Try to guess from filename
            let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let lower = filename.to_lowercase();

            if lower.contains("italic") || lower.contains("oblique") {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            }
        });

        // Check if this family already exists
        let family_exists = Self::has_family(&family_name);

        if family_exists {
            // Add variant to existing family
            let mut families = Self::global().families.write().unwrap();
            if let Some(family) = families.get_mut(&family_name) {
                // Create mutable clone
                let mut family_clone = (**family).clone();
                family_clone.add_font(FileFont::new(path), weight, style);
                *family = Arc::new(family_clone);
            }
        } else {
            // Create new family
            let mut family = FontFamily::new(&family_name);
            family.add_font(FileFont::new(path), weight, style);
            Self::register_family(family);
        }

        Ok((family_name, weight, style))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typography::MemoryFont;

    fn create_test_font() -> MemoryFont {
        MemoryFont::new(vec![0, 1, 0, 0]) // Minimal TTF signature
    }

    // Helper to ensure tests don't interfere with each other
    fn with_clean_loader<F>(f: F)
    where
        F: FnOnce(),
    {
        FontLoader::clear_all();
        f();
        FontLoader::clear_all();
    }

    // Helper for async tests
    async fn with_clean_loader_async<F, Fut>(f: F)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        FontLoader::clear_all();
        f().await;
        FontLoader::clear_all();
    }

    #[test]
    fn test_register_and_get_family() {
        with_clean_loader(|| {
            let mut family = FontFamily::new("TestFont");
            family.add_font(create_test_font(), FontWeight::W400, FontStyle::Normal);

            FontLoader::register_family(family);

            assert!(FontLoader::has_family("TestFont"));
            assert!(FontLoader::family("TestFont").is_some());
        });
    }

    #[test]
    fn test_register_font() {
        with_clean_loader(|| {
            FontLoader::register_font(
                "SimpleFont",
                create_test_font(),
                FontWeight::W400,
                FontStyle::Normal,
            );

            assert!(FontLoader::has_family("SimpleFont"));

            let family = FontLoader::family("SimpleFont").unwrap();
            assert!(family.has_variant(FontWeight::W400, FontStyle::Normal));
        });
    }

    #[test]
    fn test_get_nonexistent_family() {
        with_clean_loader(|| {
            assert!(!FontLoader::has_family("NonExistent"));
            assert!(FontLoader::family("NonExistent").is_none());
        });
    }

    #[test]
    fn test_list_families() {
        with_clean_loader(|| {
            FontLoader::register_font(
                "Font1",
                create_test_font(),
                FontWeight::W400,
                FontStyle::Normal,
            );
            FontLoader::register_font(
                "Font2",
                create_test_font(),
                FontWeight::W400,
                FontStyle::Normal,
            );

            let families = FontLoader::list_families();
            assert_eq!(families.len(), 2);
            assert!(families.contains(&"Font1".to_string()));
            assert!(families.contains(&"Font2".to_string()));
        });
    }

    #[test]
    fn test_unregister_family() {
        with_clean_loader(|| {
            FontLoader::register_font(
                "TempFont",
                create_test_font(),
                FontWeight::W400,
                FontStyle::Normal,
            );

            assert!(FontLoader::has_family("TempFont"));
            assert!(FontLoader::unregister_family("TempFont"));
            assert!(!FontLoader::has_family("TempFont"));
        });
    }

    #[test]
    fn test_unregister_nonexistent() {
        with_clean_loader(|| {
            assert!(!FontLoader::unregister_family("NonExistent"));
        });
    }

    #[test]
    fn test_family_count() {
        with_clean_loader(|| {
            assert_eq!(FontLoader::family_count(), 0);

            FontLoader::register_font(
                "Font1",
                create_test_font(),
                FontWeight::W400,
                FontStyle::Normal,
            );
            assert_eq!(FontLoader::family_count(), 1);

            FontLoader::register_font(
                "Font2",
                create_test_font(),
                FontWeight::W400,
                FontStyle::Normal,
            );
            assert_eq!(FontLoader::family_count(), 2);

            FontLoader::clear_all();
            assert_eq!(FontLoader::family_count(), 0);
        });
    }

    #[tokio::test]
    async fn test_load_font() {
        with_clean_loader_async(|| async {
            FontLoader::register_font(
                "LoadTest",
                create_test_font(),
                FontWeight::W400,
                FontStyle::Normal,
            );

            let result =
                FontLoader::load("LoadTest", FontWeight::W400, FontStyle::Normal).await;
            assert!(result.is_ok());
        })
        .await;
    }

    #[tokio::test]
    async fn test_load_nonexistent_family() {
        with_clean_loader_async(|| async {
            let result =
                FontLoader::load("NonExistent", FontWeight::W400, FontStyle::Normal).await;
            assert!(result.is_err());
        })
        .await;
    }
}
