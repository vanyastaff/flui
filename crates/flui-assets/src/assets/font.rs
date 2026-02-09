//! Font asset implementation.

use std::path::Path;

use tokio::fs;

use crate::core::{Asset, AssetMetadata};
use crate::error::AssetError;
use crate::types::AssetKey;

/// Font asset for loading fonts from various sources.
///
/// Supports TrueType (TTF) and OpenType (OTF) formats.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::FontAsset;
///
/// // Load from file path
/// let font = FontAsset::file("assets/Roboto-Regular.ttf");
///
/// // Load from bytes
/// let font_bytes = include_bytes!("Roboto-Regular.ttf");
/// let font = FontAsset::from_bytes("Roboto-Regular.ttf", font_bytes.to_vec());
/// ```
#[derive(Debug, Clone)]
pub struct FontAsset {
    /// Source path or identifier
    path: String,

    /// Optional pre-loaded bytes (for embedded fonts)
    bytes: Option<Vec<u8>>,
}

impl FontAsset {
    /// Creates a new font asset from a file path.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let font = FontAsset::file("fonts/Roboto-Regular.ttf");
    /// ```
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            bytes: None,
        }
    }

    /// Creates a new font asset from in-memory bytes.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let bytes = include_bytes!("Roboto-Regular.ttf");
    /// let font = FontAsset::from_bytes("Roboto-Regular.ttf", bytes.to_vec());
    /// ```
    pub fn from_bytes(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            path: name.into(),
            bytes: Some(bytes),
        }
    }
}

impl Asset for FontAsset {
    type Data = crate::types::FontData;
    type Key = AssetKey;
    type Error = AssetError;

    fn key(&self) -> AssetKey {
        AssetKey::new(&self.path)
    }

    async fn load(&self) -> Result<Self::Data, Self::Error> {
        // Get bytes either from memory or file
        let bytes = if let Some(ref bytes) = self.bytes {
            bytes.clone()
        } else {
            // Load from file
            fs::read(&self.path)
                .await
                .map_err(|e| AssetError::LoadFailed {
                    path: self.path.clone(),
                    reason: format!("Failed to read file: {}", e),
                })?
        };

        // Validate it's a valid font by checking magic bytes
        if bytes.len() < 4 {
            return Err(AssetError::LoadFailed {
                path: self.path.clone(),
                reason: "File too small to be a valid font".to_string(),
            });
        }

        // Check for TTF/OTF magic bytes
        let magic = &bytes[0..4];
        let is_valid = matches!(
            magic,
            [0x00, 0x01, 0x00, 0x00] | // TrueType 1.0
            [0x74, 0x72, 0x75, 0x65] | // TrueType with 'true' type
            b"OTTO" | // OpenType with CFF data
            b"ttcf" // TrueType Collection
        );

        if !is_valid {
            return Err(AssetError::LoadFailed {
                path: self.path.clone(),
                reason: "Invalid font format (not TTF/OTF)".to_string(),
            });
        }

        Ok(crate::types::FontData::from_bytes(bytes))
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        // Extract format from file extension
        let format = Path::new(&self.path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_uppercase());

        Some(AssetMetadata {
            size_bytes: self.bytes.as_ref().map(|b| b.len()),
            format,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_font_asset_from_bytes() {
        // Minimal TrueType font header (just for testing)
        let ttf_bytes = vec![
            0x00, 0x01, 0x00, 0x00, // TrueType version
            0x00, 0x00, // numTables (would normally be > 0)
            0x00, 0x00, // searchRange
            0x00, 0x00, // entrySelector
            0x00, 0x00, // rangeShift
        ];

        let asset = FontAsset::from_bytes("test.ttf", ttf_bytes);
        let font = asset.load().await.unwrap();

        // Verify we got a FontData back
        assert!(font.bytes.len() >= 10);
    }

    #[test]
    fn test_font_asset_metadata() {
        let asset = FontAsset::file("Roboto-Regular.ttf");
        let metadata = asset.metadata().unwrap();

        assert_eq!(metadata.format, Some("TTF".to_string()));
    }

    #[test]
    fn test_font_asset_key() {
        let asset = FontAsset::file("Roboto-Regular.ttf");
        let key = asset.key();

        assert_eq!(key.as_str(), "Roboto-Regular.ttf");
    }

    #[tokio::test]
    async fn test_font_asset_invalid_format() {
        let invalid_bytes = vec![0xFF, 0xFF, 0xFF, 0xFF];

        let asset = FontAsset::from_bytes("invalid.ttf", invalid_bytes);
        let result = asset.load().await;

        assert!(result.is_err());
        if let Err(AssetError::LoadFailed { reason, .. }) = result {
            assert!(reason.contains("Invalid font format"));
        }
    }
}
