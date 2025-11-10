//! Asset metadata types.

use std::time::Duration;

/// Metadata about an asset.
///
/// This struct contains optional information about an asset that can be
/// extracted without fully loading it. Useful for previews, progress indicators,
/// and preloading decisions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetMetadata {
    /// The size of the asset in bytes (if known).
    pub size_bytes: Option<usize>,

    /// The asset format/MIME type (e.g., "image/png", "audio/mp3").
    pub format: Option<String>,

    /// Dimensions for images and videos (width, height in pixels).
    pub dimensions: Option<(u32, u32)>,

    /// Duration for audio and video assets.
    pub duration: Option<Duration>,

    /// Frame rate for video assets (frames per second).
    pub frame_rate: Option<f32>,

    /// Number of frames for animated images (GIF, APNG, etc.).
    pub frame_count: Option<usize>,

    /// Sample rate for audio assets (Hz).
    pub sample_rate: Option<u32>,

    /// Number of audio channels (1 = mono, 2 = stereo, etc.).
    pub channels: Option<u8>,

    /// Custom metadata as key-value pairs.
    pub custom: Option<Vec<(String, String)>>,
}

impl AssetMetadata {
    /// Creates new empty metadata.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates metadata with just the size.
    #[inline]
    pub fn with_size(size_bytes: usize) -> Self {
        Self {
            size_bytes: Some(size_bytes),
            ..Default::default()
        }
    }

    /// Creates metadata for an image.
    #[inline]
    pub fn image(width: u32, height: u32, format: impl Into<String>) -> Self {
        Self {
            dimensions: Some((width, height)),
            format: Some(format.into()),
            ..Default::default()
        }
    }

    /// Creates metadata for audio.
    #[inline]
    pub fn audio(
        duration: Duration,
        sample_rate: u32,
        channels: u8,
        format: impl Into<String>,
    ) -> Self {
        Self {
            duration: Some(duration),
            sample_rate: Some(sample_rate),
            channels: Some(channels),
            format: Some(format.into()),
            ..Default::default()
        }
    }

    /// Creates metadata for video.
    #[inline]
    pub fn video(
        width: u32,
        height: u32,
        duration: Duration,
        frame_rate: f32,
        format: impl Into<String>,
    ) -> Self {
        Self {
            dimensions: Some((width, height)),
            duration: Some(duration),
            frame_rate: Some(frame_rate),
            format: Some(format.into()),
            ..Default::default()
        }
    }

    /// Adds a custom metadata field.
    pub fn with_custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom
            .get_or_insert_with(Vec::new)
            .push((key.into(), value.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_metadata() {
        let meta = AssetMetadata::new();
        assert!(meta.size_bytes.is_none());
        assert!(meta.format.is_none());
        assert!(meta.dimensions.is_none());
    }

    #[test]
    fn test_metadata_with_size() {
        let meta = AssetMetadata::with_size(1024);
        assert_eq!(meta.size_bytes, Some(1024));
    }

    #[test]
    fn test_image_metadata() {
        let meta = AssetMetadata::image(1920, 1080, "image/png");
        assert_eq!(meta.dimensions, Some((1920, 1080)));
        assert_eq!(meta.format, Some("image/png".to_string()));
    }

    #[test]
    fn test_audio_metadata() {
        let meta = AssetMetadata::audio(Duration::from_secs(180), 44100, 2, "audio/mp3");
        assert_eq!(meta.duration, Some(Duration::from_secs(180)));
        assert_eq!(meta.sample_rate, Some(44100));
        assert_eq!(meta.channels, Some(2));
        assert_eq!(meta.format, Some("audio/mp3".to_string()));
    }

    #[test]
    fn test_video_metadata() {
        let meta = AssetMetadata::video(1920, 1080, Duration::from_secs(120), 30.0, "video/mp4");
        assert_eq!(meta.dimensions, Some((1920, 1080)));
        assert_eq!(meta.duration, Some(Duration::from_secs(120)));
        assert_eq!(meta.frame_rate, Some(30.0));
        assert_eq!(meta.format, Some("video/mp4".to_string()));
    }

    #[test]
    fn test_custom_metadata() {
        let meta = AssetMetadata::new()
            .with_custom("author", "John Doe")
            .with_custom("license", "MIT");

        assert!(meta.custom.is_some());
        let custom = meta.custom.unwrap();
        assert_eq!(custom.len(), 2);
        assert_eq!(custom[0], ("author".to_string(), "John Doe".to_string()));
        assert_eq!(custom[1], ("license".to_string(), "MIT".to_string()));
    }
}
