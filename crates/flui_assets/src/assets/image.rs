//! Image asset implementation.

use std::path::Path;

use tokio::fs;

#[cfg(feature = "images")]
use image;

use crate::core::{Asset, AssetMetadata};
use crate::error::AssetError;
use crate::types::AssetKey;

/// Image asset for loading images from various sources.
///
/// Supports common formats: PNG, JPEG, GIF, BMP, ICO, TIFF, WebP, etc.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::ImageAsset;
///
/// // Load from file path
/// let image = ImageAsset::file("assets/logo.png");
///
/// // Load from bytes
/// let image = ImageAsset::from_bytes("logo.png", image_bytes);
/// ```
#[derive(Debug, Clone)]
pub struct ImageAsset {
    /// Source path or identifier
    path: String,

    /// Optional pre-loaded bytes (for in-memory images)
    bytes: Option<Vec<u8>>,
}

impl ImageAsset {
    /// Creates a new image asset from a file path.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let image = ImageAsset::file("logo.png");
    /// ```
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            bytes: None,
        }
    }

    /// Creates a new image asset from in-memory bytes.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let bytes = include_bytes!("logo.png");
    /// let image = ImageAsset::from_bytes("embedded_logo.png", bytes.to_vec());
    /// ```
    pub fn from_bytes(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            path: name.into(),
            bytes: Some(bytes),
        }
    }
}

impl Asset for ImageAsset {
    type Data = flui_types::painting::Image;
    type Key = AssetKey;
    type Error = AssetError;

    fn key(&self) -> AssetKey {
        AssetKey::new(&self.path)
    }

    async fn load(&self) -> Result<Self::Data, Self::Error> {
        // Get bytes either from memory or file
        let _bytes = if let Some(ref bytes) = self.bytes {
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

        #[cfg(feature = "images")]
        {
            // Decode image using image crate
            let img = image::load_from_memory(&bytes).map_err(|e| AssetError::LoadFailed {
                path: self.path.clone(),
                reason: format!("Failed to decode image: {}", e),
            })?;

            // Convert to RGBA8
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            let data = rgba.into_raw();

            Ok(flui_types::painting::Image::from_rgba8(width, height, data))
        }

        #[cfg(not(feature = "images"))]
        {
            Err(AssetError::LoadFailed {
                path: self.path.clone(),
                reason: "Image loading requires 'images' feature".to_string(),
            })
        }
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
    #[cfg(feature = "images")]
    async fn test_image_asset_from_bytes() {
        use image::{ImageBuffer, Rgba};
        use std::io::Cursor;

        // Create a 2x2 red image programmatically
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(2, 2, |_, _| {
            Rgba([255, 0, 0, 255]) // Red color
        });

        // Encode to PNG bytes
        let mut png_bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();

        let asset = ImageAsset::from_bytes("test.png", png_bytes);
        let loaded = asset.load().await.unwrap();

        assert_eq!(loaded.width(), 2);
        assert_eq!(loaded.height(), 2);

        // Verify it's RGBA format with correct data size
        assert_eq!(loaded.data().len(), 2 * 2 * 4);
    }

    #[test]
    fn test_image_asset_metadata() {
        let asset = ImageAsset::file("test.png");
        let metadata = asset.metadata().unwrap();

        assert_eq!(metadata.format, Some("PNG".to_string()));
    }

    #[test]
    fn test_image_asset_key() {
        let asset = ImageAsset::file("logo.png");
        let key = asset.key();

        assert_eq!(key.as_str(), "logo.png");
    }
}
