//! The size and pixel ratio of a window's drawable area, valid by construction.

use crate::geometry::{DevicePixelRatio, DeviceSize, Size, device_size};

/// Why a platform report makes no [`WindowMetrics`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidMetrics {
    /// A side is below one device pixel: a minimised or collapsed window.
    #[error("window extent {width}x{height} has a side below one device pixel")]
    EmptyExtent {
        /// Reported width in device pixels.
        width: i32,
        /// Reported height in device pixels.
        height: i32,
    },
    /// The logical size is not finite.
    #[error("logical window size {width}x{height} is not finite")]
    NonFiniteSize {
        /// Reported logical width.
        width: f64,
        /// Reported logical height.
        height: f64,
    },
    /// The ratio is not finite and positive.
    #[error("device pixel ratio {0} is not finite and positive")]
    InvalidScale(f64),
    /// The logical size at the ratio does not fit in device pixels.
    #[error("logical window size {width}x{height} at ratio {scale} overflows device pixels")]
    Overflow {
        /// Reported logical width.
        width: f64,
        /// Reported logical height.
        height: f64,
        /// The ratio it was scaled by.
        scale: f64,
    },
    /// The ratio is so small that the logical size is not finite.
    #[error(
        "device pixel ratio {scale} makes the logical size of {width}x{height} device pixels not finite"
    )]
    LogicalOverflow {
        /// Device width in pixels.
        width: i32,
        /// Device height in pixels.
        height: i32,
        /// The ratio.
        scale: f64,
    },
}

/// A window's drawable area: its size in device pixels and the ratio to logical pixels.
///
/// The device size is the source of truth. The surface is configured from it as it is, and
/// the logical size layout runs against is derived from it, so the two cannot disagree by a
/// rounding step the way `logical × scale` truncated back to pixels did. Both extents are at
/// least one pixel and the ratio is finite and positive; a platform report that cannot make
/// such a value (a minimised window's zero size, a NaN scale) is not a metrics change and
/// never reaches layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowMetrics {
    physical: DeviceSize,
    scale: DevicePixelRatio,
}

impl WindowMetrics {
    /// The metrics of a drawable area `physical` device pixels large at `scale`.
    ///
    /// # Errors
    ///
    /// [`InvalidMetrics::InvalidScale`] when `scale` is not finite and positive,
    /// [`InvalidMetrics::LogicalOverflow`] when it is so small that the logical size is not
    /// finite, and
    /// [`InvalidMetrics::EmptyExtent`] when a side is below one pixel.
    pub fn new(physical: DeviceSize, scale: f64) -> Result<Self, InvalidMetrics> {
        let scale = DevicePixelRatio::new(scale).ok_or(InvalidMetrics::InvalidScale(scale))?;
        if physical.width < 1 || physical.height < 1 {
            return Err(InvalidMetrics::EmptyExtent {
                width: physical.width,
                height: physical.height,
            });
        }
        let metrics = Self { physical, scale };
        let logical = metrics.logical();
        if !(logical.width.is_finite() && logical.height.is_finite()) {
            return Err(InvalidMetrics::LogicalOverflow {
                width: physical.width,
                height: physical.height,
                scale: scale.get(),
            });
        }
        Ok(metrics)
    }

    /// The metrics of a platform that reports a logical size: its device size is the
    /// logical size at `scale`, each extent rounded to the nearest pixel.
    ///
    /// # Errors
    ///
    /// [`InvalidMetrics::InvalidScale`] for a ratio that is not finite and positive,
    /// [`InvalidMetrics::NonFiniteSize`] for a size that is not finite,
    /// [`InvalidMetrics::Overflow`] when a side does not fit in device pixels,
    /// [`InvalidMetrics::LogicalOverflow`] when the ratio is too small to keep the size finite, and
    /// [`InvalidMetrics::EmptyExtent`] when a side rounds to less than a pixel.
    pub fn from_logical(logical: Size<f64>, scale: f64) -> Result<Self, InvalidMetrics> {
        let ratio = DevicePixelRatio::new(scale).ok_or(InvalidMetrics::InvalidScale(scale))?;
        if !(logical.width.is_finite() && logical.height.is_finite()) {
            return Err(InvalidMetrics::NonFiniteSize {
                width: logical.width,
                height: logical.height,
            });
        }
        let limit = f64::from(i32::MAX);
        if ratio.to_device(logical.width).abs() >= limit
            || ratio.to_device(logical.height).abs() >= limit
        {
            return Err(InvalidMetrics::Overflow {
                width: logical.width,
                height: logical.height,
                scale,
            });
        }
        Self::new(device_size(logical, ratio), scale)
    }

    /// The drawable area in device pixels, at least one pixel in each extent.
    #[must_use]
    pub const fn physical(self) -> DeviceSize {
        self.physical
    }

    /// Device pixels per logical pixel.
    #[must_use]
    pub const fn scale(self) -> DevicePixelRatio {
        self.scale
    }

    /// The drawable area in logical pixels, the size layout runs against.
    #[must_use]
    pub fn logical(self) -> Size<f64> {
        Size::new(
            self.scale.to_logical(f64::from(self.physical.width)),
            self.scale.to_logical(f64::from(self.physical.height)),
        )
    }
}
