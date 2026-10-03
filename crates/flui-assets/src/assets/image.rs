//! Image asset implementation.

use std::path::Path;

use crate::loaders::BytesFileLoader;

use image;

use crate::core::{Asset, AssetMetadata};
use crate::error::AssetError;
use crate::types::AssetKey;

/// Image asset for loading images from various sources.
///
/// Supports PNG, JPEG and GIF through the workspace's `image` codec features.
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
    type Data = flui_painting::paint::Image;
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
            BytesFileLoader::new("").load_bytes(&self.path).await?
        };

        {
            // Decode image using image crate
            let img = image::load_from_memory(&bytes).map_err(|e| AssetError::LoadFailed {
                path: self.path.clone(),
                reason: format!("Failed to decode image: {e}"),
            })?;

            // Convert to RGBA8
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            let data = rgba.into_raw();

            flui_painting::paint::Image::try_from_rgba8(width, height, data).map_err(|e| {
                AssetError::LoadFailed {
                    path: self.path.clone(),
                    reason: format!("Decoded image is malformed: {e}"),
                }
            })
        }
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        // Extract format from file extension
        let format = Path::new(&self.path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_uppercase);

        Some(AssetMetadata {
            size_bytes: self.bytes.as_ref().map(std::vec::Vec::len),
            format,
            ..Default::default()
        })
    }
}
