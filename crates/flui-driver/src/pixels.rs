//! Pixels (ADR-0238 §6): captures of a window, compared with a baseline or checked by a predicate.
//!
//! - **Sources.** A [`Capture`] names where its pixels came from ([`CaptureSource`]): `os` is the
//!   OS compositor's capture through the session's lane ([`Lane::capture_window`]); `presented`
//!   and `rendered` are the app's own, read through its agent. Each records its size, its scale
//!   factor, the frame it is of when known, and the backend that produced it.
//! - **Stable capture.** A comparison captures until two consecutive captures, a frame apart, are
//!   equal, or its timeout passes ([`Window::capture_stable`]).
//! - **Masks.** Each [`Mask`] (the elements a locator matches, or a region) is painted
//!   [`MASK_COLOR`] in both images before they are compared, so a clock or a caret does not fail
//!   a baseline.
//! - **Metric.** Per-pixel YIQ distance, pixelmatch's ([`metric`]), with a threshold and a budget
//!   of differing pixels. Anti-aliased pixels count unless the tolerance excuses them: FLUI's own
//!   rasterizer is under test, and its anti-aliasing is part of what a baseline pins.
//! - **Baselines** ([`Baselines`]) are PNG files keyed by source, backend and OS. A missing or
//!   differing one fails and leaves `.new.png` (and `.diff.png`) beside it; an update mode
//!   rewrites them, never implicitly in CI, and a check that wrote a baseline still fails.
//! - **Predicates** check one capture without a baseline: a colour at a point, a region equal to
//!   or different from an earlier capture's, a uniform region, a window stable for N frames.
//!
//! Every image a check reads (a baseline file, a capture) is untrusted: its size is checked
//! before it is decoded or copied ([`codec`]).
//!
//! [`Lane::capture_window`]: crate::Lane::capture_window
//! [`Window::capture_stable`]: crate::Window::capture_stable

mod baseline;
pub(crate) mod capture;
pub(crate) mod codec;
pub mod metric;

use std::fmt;

pub use baseline::{BaselineKey, BaselineStatus, Baselines, Comparison, Environment, UpdateMode};
pub use capture::{BaselineOptions, MAX_STABLE_CAPTURES, MAX_STABLE_FRAMES};
pub use codec::{MAX_DECODED_BYTES, MAX_PNG_FILE_BYTES, read as read_png};
pub use flui_protocol::capture::CaptureSource;
use flui_protocol::geometry::{LogicalPoint, LogicalRect};
use flui_protocol::locator::Locator;
pub use metric::{Difference, Tolerance};

use crate::error::{Error, Result};

/// The colour a mask paints over both images before they are compared: opaque magenta, which no
/// theme draws by accident.
pub const MASK_COLOR: [u8; 4] = [255, 0, 255, 255];

/// Tightly packed RGBA rows, top row first: four bytes a pixel, red first, `width * 4` bytes a
/// row. The length always matches the size.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// A `width` by `height` image of `rgba`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when `rgba` is not four bytes for each of its pixels.
    pub fn try_new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let expected = byte_len(width, height)?;
        if rgba.len() != expected {
            return Err(Error::invalid_argument(format!(
                "a {width} x {height} image holds {expected} bytes, not {}",
                rgba.len()
            )));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// A `width` by `height` image of one colour.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when the image does not fit in memory.
    pub fn filled(width: u32, height: u32, color: [u8; 4]) -> Result<Self> {
        let pixels = byte_len(width, height)? / 4;
        let rgba = color.repeat(pixels);
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Width in device pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in device pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The pixels: four bytes each, red first, rows top first.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The pixel at (`x`, `y`) as red, green, blue and alpha; `None` past the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        let at = self.offset(x, y)?;
        let bytes = self.rgba.get(at..at + 4)?;
        Some([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Set the pixel at (`x`, `y`); nothing past the image.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        if let Some(at) = self.offset(x, y)
            && let Some(bytes) = self.rgba.get_mut(at..at + 4)
        {
            bytes.copy_from_slice(&color);
        }
    }

    /// Paint the part of `region` inside the image `color`.
    pub fn fill(&mut self, region: DeviceRect, color: [u8; 4]) {
        let Some(region) = region.clipped(self.width, self.height) else {
            return;
        };
        for y in region.y..region.bottom() {
            for x in region.x..region.right() {
                self.set_pixel(x, y, color);
            }
        }
    }

    /// The part of the image `region` covers.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when `region` reaches past the image.
    pub fn crop(&self, region: DeviceRect) -> Result<Self> {
        if region.clipped(self.width, self.height) != Some(region) {
            return Err(Error::invalid_argument(format!(
                "the region {region} reaches past the {} x {} image",
                self.width, self.height
            )));
        }
        let mut rgba = Vec::with_capacity(byte_len(region.width, region.height)?);
        for y in region.y..region.bottom() {
            let start = self
                .offset(region.x, y)
                .ok_or_else(|| Error::invalid_argument("a region row is past the image"))?;
            let len = usize::try_from(u64::from(region.width) * 4)
                .map_err(|_| Error::invalid_argument("a region row does not fit in memory"))?;
            let row = self
                .rgba
                .get(start..start + len)
                .ok_or_else(|| Error::invalid_argument("a region row is past the image"))?;
            rgba.extend_from_slice(row);
        }
        Self::try_new(region.width, region.height, rgba)
    }

    fn offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        usize::try_from((u64::from(y) * u64::from(self.width) + u64::from(x)) * 4).ok()
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// The bytes a `width` by `height` image holds.
fn byte_len(width: u32, height: u32) -> Result<usize> {
    usize::try_from(u64::from(width) * u64::from(height) * 4)
        .map_err(|_| Error::invalid_argument(format!("a {width} x {height} image does not fit")))
}

/// A rectangle of device pixels from the top-left of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DeviceRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl DeviceRect {
    /// The `width` by `height` rectangle at (`x`, `y`).
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The device pixels a logical `rect` at `scale` touches: its edges rounded outward, so a
    /// mask covers every pixel the element paints into. The part left of or above the surface is
    /// cut off.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a rectangle or a scale that is not finite, a negative size,
    /// or a scale that is not positive.
    pub fn of_logical(rect: LogicalRect, scale: f64) -> Result<Self> {
        let finite = [rect.x, rect.y, rect.width, rect.height, scale]
            .iter()
            .all(|value| value.is_finite());
        if !finite || rect.width < 0.0 || rect.height < 0.0 || scale <= 0.0 {
            return Err(Error::invalid_argument(format!(
                "the region {} x {} at ({}, {}) at scale {scale} names no device pixels",
                rect.width, rect.height, rect.x, rect.y
            )));
        }
        let edge = |value: f64, round: fn(f64) -> f64| -> Result<u32> {
            let device = round(value * scale).max(0.0);
            if device > f64::from(u32::MAX) {
                return Err(Error::invalid_argument(format!(
                    "the region's edge at {value} lies past any surface"
                )));
            }
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "whole and non-negative (`round`, then `max(0.0)`), within u32 (checked \
                          above)"
            )]
            let whole = device as u32;
            Ok(whole)
        };
        let left = edge(rect.x, f64::floor)?;
        let top = edge(rect.y, f64::floor)?;
        let right = edge(rect.x + rect.width, f64::ceil)?;
        let bottom = edge(rect.y + rect.height, f64::ceil)?;
        Ok(Self::new(
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ))
    }

    /// The pixel `point` at `scale` falls in.
    ///
    /// # Errors
    ///
    /// As [`Self::of_logical`].
    pub fn pixel_of(point: LogicalPoint, scale: f64) -> Result<(u32, u32)> {
        let rect = Self::of_logical(LogicalRect::new(point.x, point.y, 0.0, 0.0), scale)?;
        Ok((rect.x, rect.y))
    }

    /// The column past the right edge.
    #[must_use]
    pub const fn right(&self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// The row past the bottom edge.
    #[must_use]
    pub const fn bottom(&self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// The part inside a `width` by `height` image; `None` when nothing is.
    #[must_use]
    pub fn clipped(self, width: u32, height: u32) -> Option<Self> {
        let right = self.right().min(width);
        let bottom = self.bottom().min(height);
        (self.x < right && self.y < bottom)
            .then(|| Self::new(self.x, self.y, right - self.x, bottom - self.y))
    }
}

impl fmt::Display for DeviceRect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} x {} at ({}, {})",
            self.width, self.height, self.x, self.y
        )
    }
}

/// What a comparison paints over before it compares: see the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Mask {
    /// Every element the locator matches in the captured window, by its bounds.
    Locator(Locator),
    /// A region of the window's surface, in logical pixels.
    Region(LogicalRect),
}

impl From<Locator> for Mask {
    fn from(locator: Locator) -> Self {
        Self::Locator(locator)
    }
}

impl From<LogicalRect> for Mask {
    fn from(region: LogicalRect) -> Self {
        Self::Region(region)
    }
}

/// One capture of a window: its pixels and where they came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    image: Image,
    source: CaptureSource,
    backend: String,
    scale: f64,
    frame: Option<u64>,
}

impl Capture {
    /// `image`, from `source`, produced by `backend` (a lane's name, an app's renderer) at
    /// `scale` device pixels per logical pixel.
    #[must_use]
    pub fn new(
        image: Image,
        source: CaptureSource,
        backend: impl Into<String>,
        scale: f64,
    ) -> Self {
        Self {
            image,
            source,
            backend: backend.into(),
            scale,
            frame: None,
        }
    }

    /// This capture, of the presented frame `frame`.
    #[must_use]
    pub fn frame(mut self, frame: u64) -> Self {
        self.frame = Some(frame);
        self
    }

    /// The pixels.
    #[must_use]
    pub fn image(&self) -> &Image {
        &self.image
    }

    /// Where they came from.
    #[must_use]
    pub fn source(&self) -> CaptureSource {
        self.source
    }

    /// What produced them: the lane for an `os` capture, the app's renderer for its own.
    #[must_use]
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Device pixels per logical pixel.
    #[must_use]
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// The presented frame the pixels are of, when known.
    #[must_use]
    pub fn frame_id(&self) -> Option<u64> {
        self.frame
    }

    /// Whether the pixel at the logical `at` is `color`, each channel within `tolerance`.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] naming the colour found when it is not; [`Error::OutsideTarget`]
    /// for a point past the capture.
    pub fn expect_color_at(&self, at: LogicalPoint, color: [u8; 4], tolerance: u8) -> Result<()> {
        let (x, y) = DeviceRect::pixel_of(at, self.scale)?;
        let found = self.image.pixel(x, y).ok_or_else(|| {
            Error::outside_target(format!(
                "({}, {}) is past the {} x {} capture",
                at.x,
                at.y,
                self.image.width(),
                self.image.height()
            ))
        })?;
        if channels_within(found, color, tolerance) {
            return Ok(());
        }
        Err(Error::Expectation {
            expected: format!("{} within {tolerance} at ({}, {})", hex(color), at.x, at.y),
            actual: hex(found),
        })
    }

    /// Whether `region` (the whole capture when `None`) is one colour, each channel within
    /// `tolerance` of its first pixel.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] naming the first pixel that is not; as [`Self::region`].
    pub fn expect_uniform(&self, region: Option<LogicalRect>, tolerance: u8) -> Result<()> {
        let pixels = self.region(region)?;
        let Some(first) = pixels.pixel(0, 0) else {
            return Err(Error::invalid_argument("an empty region has no colour"));
        };
        for y in 0..pixels.height() {
            for x in 0..pixels.width() {
                let found = pixels.pixel(x, y).unwrap_or(first);
                if !channels_within(found, first, tolerance) {
                    return Err(Error::Expectation {
                        expected: format!("the region uniformly {} within {tolerance}", hex(first)),
                        actual: format!("{} at device pixel ({x}, {y}) of the region", hex(found)),
                    });
                }
            }
        }
        Ok(())
    }

    /// Whether `region` (the whole capture when `None`) is the same as in `earlier`, within
    /// `tolerance`.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] naming how many pixels differ when it is not; as [`Self::region`].
    pub fn expect_same_as(
        &self,
        earlier: &Self,
        region: Option<LogicalRect>,
        tolerance: &Tolerance,
    ) -> Result<()> {
        let difference = self.difference(earlier, region, tolerance)?;
        if difference.within(tolerance) {
            return Ok(());
        }
        Err(Error::Expectation {
            expected: "the region as the earlier capture shows it".into(),
            actual: difference.to_string(),
        })
    }

    /// Whether `region` (the whole capture when `None`) differs from `earlier` by more than
    /// `tolerance` allows.
    ///
    /// # Errors
    ///
    /// [`Error::Expectation`] when it is the same within the tolerance; as [`Self::region`].
    pub fn expect_differs_from(
        &self,
        earlier: &Self,
        region: Option<LogicalRect>,
        tolerance: &Tolerance,
    ) -> Result<()> {
        let difference = self.difference(earlier, region, tolerance)?;
        if !difference.within(tolerance) {
            return Ok(());
        }
        Err(Error::Expectation {
            expected: "the region changed since the earlier capture".into(),
            actual: difference.to_string(),
        })
    }

    fn difference(
        &self,
        earlier: &Self,
        region: Option<LogicalRect>,
        tolerance: &Tolerance,
    ) -> Result<Difference> {
        let now = self.region(region)?;
        let before = earlier.region(region)?;
        metric::compare(&before, &now, tolerance)
    }

    /// The pixels `region` covers, the whole capture when `None`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a region that names no pixel or reaches past the capture.
    pub fn region(&self, region: Option<LogicalRect>) -> Result<Image> {
        match region {
            None => Ok(self.image.clone()),
            Some(region) => self.image.crop(DeviceRect::of_logical(region, self.scale)?),
        }
    }
}

/// Whether each channel of `found` is within `tolerance` of `expected`'s.
fn channels_within(found: [u8; 4], expected: [u8; 4], tolerance: u8) -> bool {
    found
        .iter()
        .zip(expected)
        .all(|(found, expected)| found.abs_diff(expected) <= tolerance)
}

/// `color` as `#rrggbbaa`.
fn hex(color: [u8; 4]) -> String {
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        color[0], color[1], color[2], color[3]
    )
}
