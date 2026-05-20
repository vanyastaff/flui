//! Bridge between flui-assets and cosmic-text FontSystem.
//!
//! Provides utilities for loading fonts into the glyphon/cosmic-text
//! [`FontSystem`] from various sources: raw bytes, file paths, and
//! directories.
//!
//! # Usage
//!
//! ```rust,ignore
//! use glyphon::FontSystem;
//! use flui_engine::wgpu::FontLoader;
//!
//! let mut font_system = FontSystem::new();
//!
//! // Load bundled font
//! FontLoader::load_bytes(&mut font_system, include_bytes!("fonts/Roboto.ttf"));
//!
//! // Load from file
//! FontLoader::load_file(&mut font_system, "assets/fonts/CustomFont.ttf")?;
//!
//! // Load all fonts in a directory
//! let count = FontLoader::load_directory(&mut font_system, "assets/fonts")?;
//! ```

use glyphon::FontSystem;

/// Utility for loading fonts into cosmic-text's [`FontSystem`].
///
/// This struct provides static methods for loading font data from
/// different sources. It acts as a bridge between asset management
/// and the text rendering subsystem.
pub struct FontLoader;

impl FontLoader {
    /// Load font data from raw bytes into the font system.
    ///
    /// The bytes should contain a valid TrueType (.ttf), OpenType (.otf),
    /// or TrueType Collection (.ttc) font file.
    ///
    /// # Arguments
    /// * `font_system` - The cosmic-text font system to load into
    /// * `bytes` - Raw font file bytes
    pub fn load_bytes(font_system: &mut FontSystem, bytes: &[u8]) {
        font_system.db_mut().load_font_data(bytes.to_vec());
        tracing::debug!(bytes = bytes.len(), "Loaded font from bytes");
    }

    /// Load font data from a file path into the font system.
    ///
    /// # Arguments
    /// * `font_system` - The cosmic-text font system to load into
    /// * `path` - Path to a .ttf, .otf, or .ttc font file
    ///
    /// # Errors
    /// Returns an error if the file cannot be read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_file(font_system: &mut FontSystem, path: &str) -> anyhow::Result<()> {
        let bytes = std::fs::read(path)?;
        font_system.db_mut().load_font_data(bytes);
        tracing::debug!(path, "Loaded font from file");
        Ok(())
    }

    /// Load all font files (.ttf, .otf, .ttc) from a directory.
    ///
    /// Non-recursively scans the given directory for font files and
    /// loads each one into the font system.
    ///
    /// # Arguments
    /// * `font_system` - The cosmic-text font system to load into
    /// * `dir` - Path to a directory containing font files
    ///
    /// # Returns
    /// The number of font files successfully loaded.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be read. Individual
    /// font files that fail to read are skipped with a warning.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_directory(font_system: &mut FontSystem, dir: &str) -> anyhow::Result<usize> {
        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "ttf" || ext_lower == "otf" || ext_lower == "ttc" {
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            font_system.db_mut().load_font_data(bytes);
                            count += 1;
                        }
                        Err(err) => {
                            tracing::warn!(
                                path = %path.display(),
                                %err,
                                "Failed to read font file, skipping"
                            );
                        }
                    }
                }
            }
        }
        tracing::info!(count, dir, "Loaded fonts from directory");
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_embedded_font_bytes() {
        let mut font_system = FontSystem::new();
        let before = font_system.db().faces().count();

        // Load the Roboto font already bundled with the engine
        let roboto_bytes = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
        FontLoader::load_bytes(&mut font_system, roboto_bytes);

        let after = font_system.db().faces().count();
        assert!(
            after > before,
            "Expected more font faces after loading Roboto (before={before}, after={after})"
        );
    }
}
