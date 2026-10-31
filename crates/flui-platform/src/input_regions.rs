//! Screen areas the application claims for its own input, handed to the OS per window
//! (ADR-0217 §5).
//!
//! The framework collects [`InputRegion`]s in window-logical coordinates from layout and
//! passes them to [`PlatformWindow::set_input_regions`](crate::PlatformWindow::set_input_regions)
//! on every composited frame. The backend projects them to device pixels once, with the
//! window's current scale ([`DeviceInputRegion::project`]), and updates the OS only when
//! the projection changed.

use flui_foundation::geometry::{DevicePixelRatio, DeviceRect, Rect, device_rect_covering};

/// What the OS should do with an input region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[derive(flui_foundation::inspect::Inspect)]
pub enum InputRegionKind {
    /// The OS does not start its own edge gestures (Android back and home swipes, iOS
    /// screen-edge gestures) inside the region, so the application's gesture there wins.
    SystemGestureExclusion,
}

/// One region in window-logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputRegion {
    kind: InputRegionKind,
    rect: Rect,
}

impl InputRegion {
    /// A `kind` region covering `rect`, in window-logical pixels.
    #[must_use]
    pub const fn new(kind: InputRegionKind, rect: Rect) -> Self {
        Self { kind, rect }
    }

    /// What the region is for.
    #[must_use]
    pub const fn kind(&self) -> InputRegionKind {
        self.kind
    }

    /// Where it is, in window-logical pixels.
    #[must_use]
    pub const fn rect(&self) -> Rect {
        self.rect
    }
}

/// One region in device pixels, as a backend hands it to the OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceInputRegion {
    kind: InputRegionKind,
    rect: DeviceRect,
}

/// Why a region has no device-pixel form ([`DeviceInputRegion::project`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NotProjected {
    /// The region covers nothing.
    #[error("the region is empty")]
    Empty,
    /// An edge is not finite in device pixels.
    #[error("its device-pixel edges are not finite")]
    NotFinite,
    /// A rounded edge, or the width or height, does not fit in `i32`.
    #[error("its device-pixel edges or size do not fit in i32")]
    OutOfRange,
}

impl DeviceInputRegion {
    /// A `kind` region covering the device pixels of `rect`.
    #[must_use]
    pub const fn new(kind: InputRegionKind, rect: DeviceRect) -> Self {
        Self { kind, rect }
    }

    /// `region` (window-logical) in device pixels at `ratio`: each edge rounds outward
    /// (floor of the minima, ceiling of the maxima), so a thin region at a fractional scale
    /// keeps at least one pixel.
    ///
    /// # Errors
    ///
    /// [`NotProjected`] for an empty region, and for one whose device edges are not finite
    /// or whose rounded edges, width or height do not fit in `i32`: such a region is
    /// dropped, never saturated or wrapped.
    pub fn project(region: InputRegion, ratio: DevicePixelRatio) -> Result<Self, NotProjected> {
        let rect = region.rect;
        if rect.is_empty() {
            return Err(NotProjected::Empty);
        }
        let edges =
            [rect.left(), rect.top(), rect.right(), rect.bottom()].map(|e| ratio.to_device(e));
        if edges.iter().any(|edge| !edge.is_finite()) {
            return Err(NotProjected::NotFinite);
        }
        let [left, top, right, bottom] = edges;
        let [left, top, right, bottom] = [left.floor(), top.floor(), right.ceil(), bottom.ceil()];
        let in_range = |value: f64| (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value);
        let representable = [left, top, right, bottom].into_iter().all(in_range)
            && right - left <= f64::from(i32::MAX)
            && bottom - top <= f64::from(i32::MAX);
        if !representable {
            return Err(NotProjected::OutOfRange);
        }
        Ok(Self::new(region.kind, device_rect_covering(rect, ratio)))
    }

    /// What the region is for.
    #[must_use]
    pub const fn kind(&self) -> InputRegionKind {
        self.kind
    }

    /// Where it is, in window device pixels.
    #[must_use]
    pub const fn rect(&self) -> DeviceRect {
        self.rect
    }
}
