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
