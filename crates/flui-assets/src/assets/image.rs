//! Image asset implementation.

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

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
    path: Arc<str>,

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
    pub fn file(path: impl Into<Arc<str>>) -> Self {
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
    pub fn from_bytes(name: impl Into<Arc<str>>, bytes: Vec<u8>) -> Self {
        Self {
            path: name.into(),
            bytes: Some(bytes),
        }
    }
}

/// Decodes encoded image bytes into RGBA8 pixels, synchronously.
///
/// CPU-bound: the registry's bridged loads run it on a blocking thread, never
/// on an async worker.
pub(crate) fn decode_rgba8(
    path: &str,
    bytes: &[u8],
) -> Result<flui_painting::paint::Image, AssetError> {
    let img = image::load_from_memory(bytes).map_err(|e| AssetError::LoadFailed {
        path: path.to_owned(),
        reason: format!("Failed to decode image: {e}"),
    })?;

    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    let data = rgba.into_raw();

    flui_painting::paint::Image::try_from_rgba8(width, height, data).map_err(|e| {
        AssetError::LoadFailed {
            path: path.to_owned(),
            reason: format!("Decoded image is malformed: {e}"),
        }
    })
}

impl Asset for ImageAsset {
    type Data = flui_painting::paint::Image;
    type Key = AssetKey;
    type Error = AssetError;

    fn key(&self) -> AssetKey {
        AssetKey::from(Arc::clone(&self.path))
    }

    async fn load(&self) -> Result<Self::Data, Self::Error> {
        // Get bytes either from memory or file
        let bytes = if let Some(ref bytes) = self.bytes {
            Cow::Borrowed(bytes.as_slice())
        } else {
            // Load from file
            Cow::Owned(
                BytesFileLoader::new("")
                    .load_bytes(self.path.as_ref())
                    .await?,
            )
        };

        decode_rgba8(&self.path, &bytes)
    }

    fn metadata(&self) -> Option<AssetMetadata> {
        // Extract format from file extension
        let format = Path::new(self.path.as_ref())
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
