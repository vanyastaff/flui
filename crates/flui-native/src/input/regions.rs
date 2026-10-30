//! What a backend hands the OS of a window's input regions (ADR-0217 §5).
//!
//! The framework passes each composited frame's [`InputRegion`]s, in
//! window-logical coordinates, to
//! [`PlatformWindow::set_input_regions`](flui_platform::PlatformWindow::set_input_regions).
//! The backend projects them to device pixels once, with the window's current
//! scale, and updates the OS only when the projection changed;
//! [`ProjectedInputRegions`] is that projection and comparison, shared by every
//! backend; the projection of one region is the contract's
//! ([`DeviceInputRegion::project`]).

use flui_foundation::geometry::DevicePixelRatio;
use flui_platform::DisplayId;
use flui_platform::input_regions::{DeviceInputRegion, InputRegion, NotProjected};

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
pub(crate) struct ProjectedInputRegions {
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
    pub(crate) fn update(
        &mut self,
        regions: &[InputRegion],
        scale: f64,
        display: Option<DisplayId>,
    ) -> Option<&[DeviceInputRegion]> {
        let Some(ratio) = DevicePixelRatio::try_new(scale) else {
            tracing::warn!(
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
    #[cfg(test)]
    #[must_use]
    pub(crate) fn sent(&self) -> &[DeviceInputRegion] {
        &self.sent
    }
}

/// `region` in device pixels, or `None` when it is empty (dropped silently) or not
/// representable (dropped with a diagnostic).
fn project(region: InputRegion, ratio: DevicePixelRatio) -> Option<DeviceInputRegion> {
    match DeviceInputRegion::project(region, ratio) {
        Ok(projected) => Some(projected),
        Err(NotProjected::Empty) => None,
        Err(reason) => {
            tracing::warn!(
                kind = ?region.kind(),
                rect = ?region.rect(),
                scale = ratio.get(),
                "input region dropped: {reason}"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests;
