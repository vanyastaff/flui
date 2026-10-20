//! Captures of what a window shows. The pixels never travel inside a JSON line: the result
//! announces a [`BlobRef`], and the bytes follow as a length-prefixed frame on byte transports
//! or a base64 string over BiDi (ADR-0238 §2).

use crate::geometry::LogicalRect;
use crate::tree::WindowId;

vocabulary! {
    /// Where a capture's pixels came from.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum CaptureSource {
        /// The surface texture a present put on screen.
        Presented => "presented",
        /// The committed layer tree rendered again offscreen: the headless source, and the
        /// fallback when a surface cannot be copied.
        Rendered => "rendered",
        /// The OS's own screenshot of the window, through a lane.
        Os => "os",
    }
}

/// `capture.presented` and `capture.rendered`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct CaptureParams {
    /// The window.
    pub window: WindowId,
    /// Only this part of it, in the window's surface; the whole surface when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub region: Option<LogicalRect>,
}

impl CaptureParams {
    /// The whole of `window`.
    #[must_use]
    pub const fn new(window: WindowId) -> Self {
        Self {
            window,
            region: None,
        }
    }
}

/// A binary payload that follows the result line.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct BlobRef {
    /// The frame's number on this connection.
    pub id: u64,
    /// Its length.
    pub bytes: u64,
    /// Its media type, such as `image/png`.
    pub media: String,
}

impl BlobRef {
    /// Blob `id` of `bytes` bytes of `media`.
    #[must_use]
    pub fn new(id: u64, bytes: u64, media: impl Into<String>) -> Self {
        Self {
            id,
            bytes,
            media: media.into(),
        }
    }
}

/// What a capture answers; the pixels are the blob.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct CaptureMeta {
    /// Where the pixels came from: `rendered` whenever they were not the presented texture.
    pub source: CaptureSource,
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
    /// Device pixels per logical pixel.
    pub scale: f64,
    /// The frame the pixels are of, when known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub frame: Option<u64>,
    /// The region captured, when not the whole surface.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub region: Option<LogicalRect>,
    /// The pixels.
    pub blob: BlobRef,
}
