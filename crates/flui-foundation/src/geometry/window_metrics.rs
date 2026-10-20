//! The size and pixel ratio of a window's drawable area, valid by construction.

use crate::geometry::{DevicePixelRatio, DeviceSize, Size, device_size};

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
    /// The metrics of a drawable area `physical` device pixels large at `scale`, or `None`
    /// when an extent is below one pixel or `scale` is not finite and positive.
    #[must_use]
    pub fn new(physical: DeviceSize, scale: f64) -> Option<Self> {
        if physical.width < 1 || physical.height < 1 {
            return None;
        }
        Some(Self {
            physical,
            scale: DevicePixelRatio::new(scale)?,
        })
    }

    /// The metrics of a platform that reports a logical size: its device size is the
    /// logical size at `scale`, each extent rounded to the nearest pixel.
    ///
    /// `None` when `logical` is not finite or rounds to less than a pixel, or when `scale`
    /// is not finite and positive.
    #[must_use]
    pub fn from_logical(logical: Size<f64>, scale: f64) -> Option<Self> {
        let ratio = DevicePixelRatio::new(scale)?;
        if !(logical.width.is_finite() && logical.height.is_finite()) {
            return None;
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
