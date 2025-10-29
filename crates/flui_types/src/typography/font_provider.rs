//! Font provider abstraction for loading fonts.
//!
//! Provides the `FontProvider` trait and common implementations like
//! `MemoryFont`, `AssetFont`, and `FileFont`.
//!
//! Similar to Flutter's `FontLoader`, this module allows dynamic font loading
//! with support for different font sources.

use crate::typography::{FontStyle, FontWeight};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

/// Error type for font loading operations.
#[derive(Debug, Clone)]
pub enum FontError {
    /// Failed to load the font from the source.
    LoadFailed(String),

    /// Failed to parse the font data.
    ParseFailed(String),

    /// The font format is not supported.
    UnsupportedFormat(String),

    /// The font source was not found.
    NotFound(String),

    /// Invalid font data.
    InvalidData(String),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::LoadFailed(msg) => write!(f, "Failed to load font: {}", msg),
            FontError::ParseFailed(msg) => write!(f, "Failed to parse font: {}", msg),
            FontError::UnsupportedFormat(msg) => write!(f, "Unsupported format: {}", msg),
            FontError::NotFound(msg) => write!(f, "Font not found: {}", msg),
            FontError::InvalidData(msg) => write!(f, "Invalid font data: {}", msg),
        }
    }
}

impl Error for FontError {}

/// Result type for font loading operations.
pub type FontResult<T> = Result<T, FontError>;

/// Font data loaded from a provider.
///
/// Contains the raw font bytes (TTF/OTF format) that can be used
/// by text rendering backends.
#[derive(Clone, Debug)]
pub struct FontData {
    /// Raw font bytes (TTF/OTF format)
    pub bytes: Arc<Vec<u8>>,
    /// Font weight hint (optional, may be detected from font)
    pub weight: Option<FontWeight>,
    /// Font style hint (optional, may be detected from font)
    pub style: Option<FontStyle>,
}

impl FontData {
    /// Creates font data from raw bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::new(bytes),
            weight: None,
            style: None,
        }
    }

    /// Creates font data with weight and style hints.
    pub fn with_hints(bytes: Vec<u8>, weight: FontWeight, style: FontStyle) -> Self {
        Self {
            bytes: Arc::new(bytes),
            weight: Some(weight),
            style: Some(style),
        }
    }

    /// Returns the font data as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Identifies a font without committing to the precise final asset.
///
/// Similar to Flutter's `FontLoader`.
///
/// A FontProvider is a factory for font data. It allows you to abstract
/// over different font sources (assets, files, memory) while using the same
/// API for loading.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::typography::{FontProvider, MemoryFont};
///
/// let provider = MemoryFont::new(font_bytes);
///
/// // In an async context:
/// let font_data = provider.load().await?;
/// ```
pub trait FontProvider: Send + Sync {
    /// Loads the font data.
    ///
    /// This is typically called by the framework when a font needs to be loaded.
    fn load(&self) -> Pin<Box<dyn Future<Output = FontResult<FontData>> + Send + '_>>;

    /// Returns a key that uniquely identifies this provider.
    ///
    /// This is used for caching and comparing providers.
    fn key(&self) -> String;

    /// Returns whether this provider is equal to another provider.
    ///
    /// Two providers are equal if they will produce the same font data.
    fn equals(&self, other: &dyn FontProvider) -> bool {
        self.key() == other.key()
    }
}

/// A font provider that loads fonts from raw bytes in memory.
///
/// Similar to Flutter's byte buffer approach.
///
/// # Examples
///
/// ```
/// use flui_types::typography::MemoryFont;
///
/// // Load embedded font
/// let font_bytes = include_bytes!("../../../assets/fonts/Arial.ttf");
/// let provider = MemoryFont::new(font_bytes.to_vec());
/// ```
#[derive(Clone, Debug)]
pub struct MemoryFont {
    /// The raw font data (TTF/OTF).
    bytes: Arc<Vec<u8>>,
    /// Optional weight hint
    weight: Option<FontWeight>,
    /// Optional style hint
    style: Option<FontStyle>,
}

impl MemoryFont {
    /// Creates a new memory font from bytes.
    ///
    /// # Arguments
    ///
    /// * `bytes` - The raw font data (TTF/OTF format)
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::new(bytes),
            weight: None,
            style: None,
        }
    }

    /// Sets font weight hint.
    #[must_use]
    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Sets font style hint.
    #[must_use]
    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Returns the font bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl FontProvider for MemoryFont {
    fn load(&self) -> Pin<Box<dyn Future<Output = FontResult<FontData>> + Send + '_>> {
        let bytes = self.bytes.clone();
        let weight = self.weight;
        let style = self.style;

        Box::pin(async move {
            Ok(FontData {
                bytes,
                weight,
                style,
            })
        })
    }

    fn key(&self) -> String {
        format!("MemoryFont({:p})", Arc::as_ptr(&self.bytes))
    }
}

/// A font provider that loads fonts from the application's asset bundle.
///
/// Similar to Flutter's `AssetBundle.load()`.
///
/// # Examples
///
/// ```
/// use flui_types::typography::AssetFont;
///
/// let provider = AssetFont::new("fonts/Roboto-Regular.ttf");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct AssetFont {
    /// The path to the font asset.
    asset_name: String,
    /// Optional package name for the asset.
    package: Option<String>,
    /// Optional weight hint
    weight: Option<FontWeight>,
    /// Optional style hint
    style: Option<FontStyle>,
}

impl AssetFont {
    /// Creates a new asset font provider.
    ///
    /// # Arguments
    ///
    /// * `asset_name` - The path to the font asset (e.g., "fonts/Roboto-Regular.ttf")
    #[must_use]
    pub fn new(asset_name: impl Into<String>) -> Self {
        Self {
            asset_name: asset_name.into(),
            package: None,
            weight: None,
            style: None,
        }
    }

    /// Sets the package name for this asset.
    #[must_use]
    pub fn with_package(mut self, package: impl Into<String>) -> Self {
        self.package = Some(package.into());
        self
    }

    /// Sets font weight hint.
    #[must_use]
    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Sets font style hint.
    #[must_use]
    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Returns the asset name.
    #[must_use]
    pub fn asset_name(&self) -> &str {
        &self.asset_name
    }

    /// Returns the package name, if any.
    #[must_use]
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
}

impl FontProvider for AssetFont {
    fn load(&self) -> Pin<Box<dyn Future<Output = FontResult<FontData>> + Send + '_>> {
        let asset_name = self.asset_name.clone();
        let package = self.package.clone();
        let weight = self.weight;
        let style = self.style;

        Box::pin(async move {
            use std::path::PathBuf;
            use tokio::fs::File;
            use tokio::io::AsyncReadExt;

            // Construct asset path
            // In a real application, this would use an asset bundle system
            // For now, we assume assets are in an "assets" directory
            let mut asset_path = PathBuf::from("assets");

            if let Some(ref pkg) = package {
                asset_path.push("packages");
                asset_path.push(pkg);
            }

            asset_path.push(&asset_name);

            // Read file
            let mut file = File::open(&asset_path).await.map_err(|e| {
                FontError::NotFound(format!(
                    "Font asset not found: {} ({})",
                    asset_path.display(),
                    e
                ))
            })?;

            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .await
                .map_err(|e| FontError::LoadFailed(format!("Failed to read font asset: {}", e)))?;

            // Validate it's a font file (basic check for TTF/OTF magic numbers)
            if buffer.len() < 4 {
                return Err(FontError::InvalidData("Font file too small".to_string()));
            }

            // Check for TTF/OTF signatures
            let is_valid = buffer[0..4] == [0x00, 0x01, 0x00, 0x00] // TrueType
                || buffer[0..4] == [0x4F, 0x54, 0x54, 0x4F] // OpenType (OTTO)
                || buffer[0..4] == *b"true" // TrueType (Mac)
                || buffer[0..4] == *b"typ1"; // Type 1

            if !is_valid {
                return Err(FontError::UnsupportedFormat(
                    "Not a valid TTF/OTF font file".to_string(),
                ));
            }

            Ok(FontData {
                bytes: Arc::new(buffer),
                weight,
                style,
            })
        })
    }

    fn key(&self) -> String {
        match &self.package {
            Some(package) => format!("AssetFont({}/{})", package, self.asset_name),
            None => format!("AssetFont({})", self.asset_name),
        }
    }
}

/// A font provider that loads fonts from the file system.
///
/// # Examples
///
/// ```
/// use flui_types::typography::FileFont;
/// use std::path::PathBuf;
///
/// let provider = FileFont::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct FileFont {
    /// The path to the font file.
    path: PathBuf,
    /// Optional weight hint
    weight: Option<FontWeight>,
    /// Optional style hint
    style: Option<FontStyle>,
}

impl FileFont {
    /// Creates a new file font provider.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the font file
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            weight: None,
            style: None,
        }
    }

    /// Sets font weight hint.
    #[must_use]
    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Sets font style hint.
    #[must_use]
    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Returns the file path.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl FontProvider for FileFont {
    fn load(&self) -> Pin<Box<dyn Future<Output = FontResult<FontData>> + Send + '_>> {
        let path = self.path.clone();
        let weight = self.weight;
        let style = self.style;

        Box::pin(async move {
            use tokio::fs::File;
            use tokio::io::AsyncReadExt;

            // Read file
            let mut file = File::open(&path).await.map_err(|e| {
                FontError::NotFound(format!("Font file not found: {} ({})", path.display(), e))
            })?;

            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .await
                .map_err(|e| FontError::LoadFailed(format!("Failed to read font file: {}", e)))?;

            // Validate it's a font file
            if buffer.len() < 4 {
                return Err(FontError::InvalidData("Font file too small".to_string()));
            }

            // Check for TTF/OTF signatures
            let is_valid = buffer[0..4] == [0x00, 0x01, 0x00, 0x00] // TrueType
                || buffer[0..4] == [0x4F, 0x54, 0x54, 0x4F] // OpenType (OTTO)
                || buffer[0..4] == *b"true" // TrueType (Mac)
                || buffer[0..4] == *b"typ1"; // Type 1

            if !is_valid {
                return Err(FontError::UnsupportedFormat(
                    "Not a valid TTF/OTF font file".to_string(),
                ));
            }

            Ok(FontData {
                bytes: Arc::new(buffer),
                weight,
                style,
            })
        })
    }

    fn key(&self) -> String {
        format!("FileFont({})", self.path.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_data_creation() {
        let data = vec![0, 1, 0, 0]; // Minimal TTF signature
        let font_data = FontData::from_bytes(data.clone());

        assert_eq!(font_data.as_bytes(), &data[..]);
        assert_eq!(font_data.weight, None);
        assert_eq!(font_data.style, None);
    }

    #[test]
    fn test_font_data_with_hints() {
        let data = vec![0, 1, 0, 0];
        let font_data = FontData::with_hints(data, FontWeight::BOLD, FontStyle::Italic);

        assert_eq!(font_data.weight, Some(FontWeight::BOLD));
        assert_eq!(font_data.style, Some(FontStyle::Italic));
    }

    #[test]
    fn test_memory_font_creation() {
        let data = vec![0, 1, 0, 0];
        let provider = MemoryFont::new(data.clone());

        assert_eq!(provider.bytes(), &data[..]);
    }

    #[test]
    fn test_memory_font_with_hints() {
        let data = vec![0, 1, 0, 0];
        let provider = MemoryFont::new(data)
            .with_weight(FontWeight::W600)
            .with_style(FontStyle::Normal);

        assert_eq!(provider.weight, Some(FontWeight::W600));
        assert_eq!(provider.style, Some(FontStyle::Normal));
    }

    #[test]
    fn test_asset_font_creation() {
        let provider = AssetFont::new("fonts/Roboto-Regular.ttf");

        assert_eq!(provider.asset_name(), "fonts/Roboto-Regular.ttf");
        assert_eq!(provider.package(), None);
    }

    #[test]
    fn test_asset_font_with_package() {
        let provider = AssetFont::new("fonts/Roboto-Regular.ttf")
            .with_package("my_package")
            .with_weight(FontWeight::W400);

        assert_eq!(provider.asset_name(), "fonts/Roboto-Regular.ttf");
        assert_eq!(provider.package(), Some("my_package"));
        assert_eq!(provider.weight, Some(FontWeight::W400));
    }

    #[test]
    fn test_file_font_creation() {
        let provider = FileFont::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");

        assert_eq!(
            provider.path().to_str(),
            Some("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")
        );
    }

    #[test]
    fn test_font_provider_key() {
        let data = vec![0, 1, 0, 0];
        let provider1 = MemoryFont::new(data.clone());
        let provider2 = MemoryFont::new(data);

        // Keys should be different because they use different Arc pointers
        assert_ne!(provider1.key(), provider2.key());
    }

    #[test]
    fn test_asset_font_key() {
        let provider1 = AssetFont::new("fonts/test.ttf");
        let provider2 = AssetFont::new("fonts/test.ttf");

        // Keys should be the same for identical assets
        assert_eq!(provider1.key(), provider2.key());
    }

    #[tokio::test]
    async fn test_memory_font_load() {
        let data = vec![0, 1, 0, 0, 0, 0, 0, 0]; // Minimal valid data
        let provider = MemoryFont::new(data.clone());

        let result = provider.load().await;
        assert!(result.is_ok());

        let font_data = result.unwrap();
        assert_eq!(font_data.as_bytes(), &data[..]);
    }
}
