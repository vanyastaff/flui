//! Screen areas the application claims for its own input, handed to the OS per window
//! (ADR-0217 §5).
//!
//! The framework collects [`InputRegion`]s in window-logical coordinates from layout and
//! passes them to [`PlatformWindow::set_input_regions`](crate::PlatformWindow::set_input_regions)
//! on every composited frame. The backend projects them to device pixels once, with the
//! window's current scale, and updates the OS only when the projection changed;
//! [`ProjectedInputRegions`] is that projection and comparison, shared by every backend so
//! the rule is written once.

use flui_foundation::geometry::{DevicePixelRatio, DeviceRect, Rect, device_rect_covering};

use crate::DisplayId;

/// What the OS should do with an input region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
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

impl DeviceInputRegion {
    /// A `kind` region covering the device pixels of `rect`.
    #[must_use]
    pub const fn new(kind: InputRegionKind, rect: DeviceRect) -> Self {
        Self { kind, rect }
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

/// The device-pixel regions a backend last handed the OS for one window, and the scale and
/// display they were projected for.
///
/// [`update`](Self::update) projects a frame's logical regions and answers whether the OS
/// needs them: the comparison key is the projected device-pixel set together with the scale
/// and the window's display, so a scale or display change re-sends an otherwise equal
/// logical set, and a logical change too small to move a pixel sends nothing. A window
/// starts with nothing claimed, so an empty first set sends nothing either. Storage is a
/// `Vec` scanned linearly: the OS takes a handful of rectangles.
#[derive(Debug, Default)]
pub struct ProjectedInputRegions {
    sent: Vec<DeviceInputRegion>,
    /// The scale (`f64::to_bits`) and display `sent` was projected for; `None` until the
    /// first projection.
    key: Option<(u64, Option<DisplayId>)>,
    /// Reused for each projection.
    scratch: Vec<DeviceInputRegion>,
}

impl ProjectedInputRegions {
    /// Projects `regions` (window-logical) to device pixels at `scale` for a window on
    /// `display`, and returns the set to hand the OS when it differs from the last one, or
    /// `None` when the OS already holds it.
    ///
    /// Each rectangle rounds outward (floor of the minima, ceiling of the maxima), so a thin
    /// region at a fractional scale keeps at least one pixel. A region whose device edges
    /// are not finite, or whose rounded edges or width and height are not representable as
    /// `i32`, is dropped with a diagnostic rather than saturated or wrapped; an empty one is
    /// dropped silently. A `scale` that is zero, negative or not finite projects nothing:
    /// the call reports it and keeps what the OS holds.
    pub fn update(
        &mut self,
        regions: &[InputRegion],
        scale: f64,
        display: Option<DisplayId>,
    ) -> Option<&[DeviceInputRegion]> {
        let Some(ratio) = DevicePixelRatio::new(scale) else {
            tracing::warn!(
                target: "flui_platform_api::input_regions",
                scale,
                "input regions not projected: the window scale is not a positive finite number"
            );
            return None;
        };
        self.scratch.clear();
        self.scratch
            .extend(regions.iter().filter_map(|region| project(*region, ratio)));
        let key = Some((scale.to_bits(), display));
        let unchanged = if self.key.is_none() {
            self.scratch.is_empty()
        } else {
            self.key == key && self.scratch == self.sent
        };
        self.key = key;
        if unchanged {
            return None;
        }
        std::mem::swap(&mut self.sent, &mut self.scratch);
        Some(&self.sent)
    }

    /// The device-pixel regions the OS was last handed.
    #[must_use]
    pub fn sent(&self) -> &[DeviceInputRegion] {
        &self.sent
    }
}

/// `region` in device pixels, or `None` when it is empty or not representable.
fn project(region: InputRegion, ratio: DevicePixelRatio) -> Option<DeviceInputRegion> {
    let rect = region.rect;
    if rect.is_empty() {
        return None;
    }
    let edges = [rect.left(), rect.top(), rect.right(), rect.bottom()].map(|e| ratio.to_device(e));
    if edges.iter().any(|edge| !edge.is_finite()) {
        tracing::warn!(
            target: "flui_platform_api::input_regions",
            kind = ?region.kind,
            rect = ?rect,
            scale = ratio.get(),
            "input region dropped: its device-pixel edges are not finite"
        );
        return None;
    }
    let [left, top, right, bottom] = edges;
    let [left, top, right, bottom] = [left.floor(), top.floor(), right.ceil(), bottom.ceil()];
    let in_range = |value: f64| (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value);
    let representable = [left, top, right, bottom].into_iter().all(in_range)
        && right - left <= f64::from(i32::MAX)
        && bottom - top <= f64::from(i32::MAX);
    if !representable {
        tracing::warn!(
            target: "flui_platform_api::input_regions",
            kind = ?region.kind,
            rect = ?rect,
            scale = ratio.get(),
            "input region dropped: its device-pixel edges or size do not fit in i32"
        );
        return None;
    }
    Some(DeviceInputRegion::new(
        region.kind,
        device_rect_covering(rect, ratio),
    ))
}
