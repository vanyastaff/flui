//! Image provider abstraction for loading images.
//!
//! Provides the `ImageProvider` trait and common implementations like
//! `MemoryImage`, `AssetImage`, and `NetworkImage`.

use crate::geometry::Size;
use crate::painting::{Image, ImageConfiguration};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

/// Error type for image loading operations.
#[derive(Debug, Clone)]
pub enum ImageError {
    /// Failed to load the image from the source.
    LoadFailed(String),

    /// Failed to decode the image data.
    DecodeFailed(String),

    /// The image format is not supported.
    UnsupportedFormat(String),

    /// The image source was not found.
    NotFound(String),

    /// Network error occurred while fetching the image.
    NetworkError(String),

    /// Invalid image data.
    InvalidData(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageError::LoadFailed(msg) => write!(f, "Failed to load image: {}", msg),
            ImageError::DecodeFailed(msg) => write!(f, "Failed to decode image: {}", msg),
            ImageError::UnsupportedFormat(msg) => write!(f, "Unsupported format: {}", msg),
            ImageError::NotFound(msg) => write!(f, "Image not found: {}", msg),
            ImageError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            ImageError::InvalidData(msg) => write!(f, "Invalid image data: {}", msg),
        }
    }
}

impl Error for ImageError {}

/// Result type for image loading operations.
pub type ImageResult<T> = Result<T, ImageError>;

/// Identifies an image without committing to the precise final asset.
///
/// Similar to Flutter's `ImageProvider`.
///
/// An ImageProvider is a factory for Image objects. It allows you to abstract
/// over different image sources (assets, network, memory) while using the same
/// API for loading and caching.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::painting::{ImageProvider, MemoryImage, ImageConfiguration};
///
/// let provider = MemoryImage::new(vec![255; 100 * 100 * 4], 100, 100);
/// let config = ImageConfiguration::new();
///
/// // In an async context:
/// let image = provider.load(&config).await?;
/// ```
pub trait ImageProvider: Send + Sync {
    /// Converts a provider to a stream of Image objects.
    ///
    /// The `configuration` is used to determine which variant of the image
    /// to load (e.g., for different device pixel ratios).
    ///
    /// This is typically called by the framework when an image needs to be displayed.
    fn load(&self, configuration: &ImageConfiguration) -> impl std::future::Future<Output = ImageResult<Image>> + Send;

    /// Returns a key that uniquely identifies this provider.
    ///
    /// This is used for caching and comparing providers.
    fn key(&self) -> String;

    /// Returns whether this provider is equal to another provider.
    ///
    /// Two providers are equal if they will produce the same image.
    fn equals(&self, other: &dyn ImageProvider) -> bool {
        self.key() == other.key()
    }

    /// Resolves this image provider using the given configuration.
    ///
    /// This is a convenience method that creates a resolved provider.
    fn resolve(&self, configuration: ImageConfiguration) -> ResolvedImageProvider {
        ResolvedImageProvider {
            provider: Arc::new(self) as Arc<dyn ImageProvider>,
            configuration,
        }
    }
}

/// A resolved image provider with its configuration.
///
/// This struct holds both the provider and the configuration used to resolve it.
#[derive(Clone)]
pub struct ResolvedImageProvider {
    provider: Arc<dyn ImageProvider>,
    configuration: ImageConfiguration,
}

impl ResolvedImageProvider {
    /// Creates a new resolved image provider.
    pub fn new(provider: Arc<dyn ImageProvider>, configuration: ImageConfiguration) -> Self {
        Self {
            provider,
            configuration,
        }
    }

    /// Loads the image using the stored configuration.
    pub async fn load(&self) -> ImageResult<Image> {
        self.provider.load(&self.configuration).await
    }

    /// Returns the key for this resolved provider.
    pub fn key(&self) -> String {
        self.provider.key()
    }
}

/// An image provider that loads images from raw RGBA8 bytes in memory.
///
/// Similar to Flutter's `MemoryImage`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::MemoryImage;
///
/// // Create a 2x2 red image
/// let data = vec![
///     255, 0, 0, 255,  // Red pixel
///     255, 0, 0, 255,  // Red pixel
///     255, 0, 0, 255,  // Red pixel
///     255, 0, 0, 255,  // Red pixel
/// ];
///
/// let provider = MemoryImage::new(data, 2, 2);
/// ```
#[derive(Clone, Debug)]
pub struct MemoryImage {
    /// The raw RGBA8 pixel data.
    bytes: Arc<Vec<u8>>,
    /// The width of the image in pixels.
    width: u32,
    /// The height of the image in pixels.
    height: u32,
    /// Optional scale factor for the image.
    scale: f32,
}

impl MemoryImage {
    /// Creates a new memory image from RGBA8 bytes.
    ///
    /// # Arguments
    ///
    /// * `bytes` - The raw RGBA8 pixel data (4 bytes per pixel)
    /// * `width` - The width of the image in pixels
    /// * `height` - The height of the image in pixels
    ///
    /// # Panics
    ///
    /// Panics if `bytes.len() != width * height * 4`.
    #[must_use]
    pub fn new(bytes: Vec<u8>, width: u32, height: u32) -> Self {
        assert_eq!(
            bytes.len(),
            (width * height * 4) as usize,
            "Image data length must be width * height * 4"
        );

        Self {
            bytes: Arc::new(bytes),
            width,
            height,
            scale: 1.0,
        }
    }

    /// Creates a new memory image with a specific scale factor.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Returns the size of the image in logical pixels.
    #[must_use]
    pub fn size(&self) -> Size {
        Size::new(
            self.width as f32 / self.scale,
            self.height as f32 / self.scale,
        )
    }
}

impl ImageProvider for MemoryImage {
    async fn load(&self, _configuration: &ImageConfiguration) -> ImageResult<Image> {
        Ok(Image::from_rgba8(self.width, self.height, (*self.bytes).clone()))
    }

    fn key(&self) -> String {
        format!("MemoryImage({:p}, {}x{})", Arc::as_ptr(&self.bytes), self.width, self.height)
    }
}

/// An image provider that loads images from the application's asset bundle.
///
/// Similar to Flutter's `AssetImage`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::AssetImage;
///
/// let provider = AssetImage::new("icons/logo.png");
/// let provider_with_scale = AssetImage::new("icons/logo.png").with_scale(2.0);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssetImage {
    /// The path to the asset.
    asset_name: String,
    /// Optional package name for the asset.
    package: Option<String>,
    /// Scale factor for the asset.
    scale: f32,
}

impl AssetImage {
    /// Creates a new asset image provider.
    ///
    /// # Arguments
    ///
    /// * `asset_name` - The path to the asset (e.g., "images/logo.png")
    #[must_use]
    pub fn new(asset_name: impl Into<String>) -> Self {
        Self {
            asset_name: asset_name.into(),
            package: None,
            scale: 1.0,
        }
    }

    /// Sets the package name for this asset.
    #[must_use]
    pub fn with_package(mut self, package: impl Into<String>) -> Self {
        self.package = Some(package.into());
        self
    }

    /// Sets the scale factor for this asset.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
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

    /// Returns the scale factor.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }
}

impl ImageProvider for AssetImage {
    async fn load(&self, _configuration: &ImageConfiguration) -> ImageResult<Image> {
        // TODO: Implement actual asset loading
        // This would typically involve reading from a file system or embedded assets
        Err(ImageError::LoadFailed(
            "AssetImage loading not yet implemented".to_string(),
        ))
    }

    fn key(&self) -> String {
        match &self.package {
            Some(package) => format!("AssetImage({}, {}, scale={})", package, self.asset_name, self.scale),
            None => format!("AssetImage({}, scale={})", self.asset_name, self.scale),
        }
    }
}

/// An image provider that loads images from the file system.
///
/// # Examples
///
/// ```
/// use flui_types::painting::FileImage;
/// use std::path::PathBuf;
///
/// let provider = FileImage::new("/path/to/image.png");
/// let provider_with_scale = FileImage::new("image.png").with_scale(2.0);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileImage {
    /// The path to the file.
    path: PathBuf,
    /// Scale factor for the image.
    scale: f32,
}

impl FileImage {
    /// Creates a new file image provider.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the image file
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            scale: 1.0,
        }
    }

    /// Sets the scale factor for this image.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Returns the file path.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Returns the scale factor.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }
}

impl ImageProvider for FileImage {
    async fn load(&self, _configuration: &ImageConfiguration) -> ImageResult<Image> {
        // TODO: Implement actual file loading
        // This would typically involve reading from the file system and decoding
        Err(ImageError::LoadFailed(
            "FileImage loading not yet implemented".to_string(),
        ))
    }

    fn key(&self) -> String {
        format!("FileImage({}, scale={})", self.path.display(), self.scale)
    }
}

/// An image provider that loads images from a network URL.
///
/// Similar to Flutter's `NetworkImage`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::NetworkImage;
///
/// let provider = NetworkImage::new("https://example.com/image.png");
/// let provider_with_scale = NetworkImage::new("https://example.com/image.png")
///     .with_scale(2.0);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetworkImage {
    /// The URL to fetch the image from.
    url: String,
    /// Scale factor for the image.
    scale: f32,
    /// Optional headers to include in the request.
    headers: Option<Vec<(String, String)>>,
}

impl NetworkImage {
    /// Creates a new network image provider.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to fetch the image from
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            scale: 1.0,
            headers: None,
        }
    }

    /// Sets the scale factor for this image.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Adds HTTP headers to the request.
    #[must_use]
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Returns the URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the scale factor.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Returns the headers, if any.
    #[must_use]
    pub fn headers(&self) -> Option<&[(String, String)]> {
        self.headers.as_deref()
    }
}

impl ImageProvider for NetworkImage {
    async fn load(&self, _configuration: &ImageConfiguration) -> ImageResult<Image> {
        // TODO: Implement actual network loading
        // This would typically involve using reqwest or similar to fetch the image
        Err(ImageError::LoadFailed(
            "NetworkImage loading not yet implemented".to_string(),
        ))
    }

    fn key(&self) -> String {
        format!("NetworkImage({}, scale={})", self.url, self.scale)
    }
}

/// An image provider that delegates to another provider with a transformation.
///
/// This can be used to apply transformations or filters to images from other providers.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_types::painting::{TransformedImageProvider, MemoryImage};
///
/// let base = MemoryImage::new(data, 100, 100);
/// let transformed = TransformedImageProvider::new(base, |img| {
///     // Apply transformation
///     Ok(img)
/// });
/// ```
pub struct TransformedImageProvider<F>
where
    F: Fn(Image) -> ImageResult<Image> + Send + Sync,
{
    /// The base image provider.
    base: Arc<dyn ImageProvider>,
    /// The transformation function.
    transform: F,
    /// A unique key for this transformation.
    transform_key: String,
}

impl<F> TransformedImageProvider<F>
where
    F: Fn(Image) -> ImageResult<Image> + Send + Sync,
{
    /// Creates a new transformed image provider.
    ///
    /// # Arguments
    ///
    /// * `base` - The base image provider to transform
    /// * `transform` - The transformation function
    /// * `transform_key` - A unique identifier for this transformation
    pub fn new(base: impl ImageProvider + 'static, transform: F, transform_key: String) -> Self {
        Self {
            base: Arc::new(base),
            transform,
            transform_key,
        }
    }
}

impl<F> ImageProvider for TransformedImageProvider<F>
where
    F: Fn(Image) -> ImageResult<Image> + Send + Sync,
{
    async fn load(&self, configuration: &ImageConfiguration) -> ImageResult<Image> {
        let base_image = self.base.load(configuration).await?;
        (self.transform)(base_image)
    }

    fn key(&self) -> String {
        format!("TransformedImage({}, {})", self.base.key(), self.transform_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_image_creation() {
        let data = vec![255; 100 * 100 * 4];
        let provider = MemoryImage::new(data, 100, 100);

        assert_eq!(provider.width, 100);
        assert_eq!(provider.height, 100);
        assert_eq!(provider.scale, 1.0);
    }

    #[test]
    fn test_memory_image_with_scale() {
        let data = vec![255; 100 * 100 * 4];
        let provider = MemoryImage::new(data, 100, 100).with_scale(2.0);

        assert_eq!(provider.scale, 2.0);
        assert_eq!(provider.size(), Size::new(50.0, 50.0));
    }

    #[test]
    #[should_panic(expected = "Image data length must be width * height * 4")]
    fn test_memory_image_invalid_data() {
        let data = vec![255; 100]; // Wrong size
        MemoryImage::new(data, 100, 100);
    }

    #[test]
    fn test_asset_image_creation() {
        let provider = AssetImage::new("test.png");

        assert_eq!(provider.asset_name(), "test.png");
        assert_eq!(provider.package(), None);
        assert_eq!(provider.scale(), 1.0);
    }

    #[test]
    fn test_asset_image_with_package() {
        let provider = AssetImage::new("test.png")
            .with_package("my_package")
            .with_scale(2.0);

        assert_eq!(provider.asset_name(), "test.png");
        assert_eq!(provider.package(), Some("my_package"));
        assert_eq!(provider.scale(), 2.0);
    }

    #[test]
    fn test_file_image_creation() {
        let provider = FileImage::new("/path/to/image.png");

        assert_eq!(provider.path().to_str(), Some("/path/to/image.png"));
        assert_eq!(provider.scale(), 1.0);
    }

    #[test]
    fn test_network_image_creation() {
        let provider = NetworkImage::new("https://example.com/image.png");

        assert_eq!(provider.url(), "https://example.com/image.png");
        assert_eq!(provider.scale(), 1.0);
        assert!(provider.headers().is_none());
    }

    #[test]
    fn test_network_image_with_headers() {
        let headers = vec![
            ("Authorization".to_string(), "Bearer token".to_string()),
            ("User-Agent".to_string(), "FLUI/1.0".to_string()),
        ];

        let provider = NetworkImage::new("https://example.com/image.png")
            .with_scale(2.0)
            .with_headers(headers.clone());

        assert_eq!(provider.scale(), 2.0);
        assert_eq!(provider.headers(), Some(headers.as_slice()));
    }

    #[tokio::test]
    async fn test_memory_image_load() {
        let data = vec![255; 10 * 10 * 4];
        let provider = MemoryImage::new(data, 10, 10);
        let config = ImageConfiguration::new();

        let result = provider.load(&config).await;
        assert!(result.is_ok());

        let image = result.unwrap();
        assert_eq!(image.width(), 10);
        assert_eq!(image.height(), 10);
    }

    #[test]
    fn test_image_provider_key() {
        let data = vec![255; 10 * 10 * 4];
        let provider1 = MemoryImage::new(data.clone(), 10, 10);
        let provider2 = MemoryImage::new(data, 10, 10);

        // Keys should be different because they use different Arc pointers
        assert_ne!(provider1.key(), provider2.key());
    }

    #[test]
    fn test_asset_image_key() {
        let provider1 = AssetImage::new("test.png").with_scale(2.0);
        let provider2 = AssetImage::new("test.png").with_scale(2.0);

        // Keys should be the same for identical assets
        assert_eq!(provider1.key(), provider2.key());
    }
}
