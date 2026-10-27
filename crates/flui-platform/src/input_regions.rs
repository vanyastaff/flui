//! Screen areas the application claims for its own input, handed to the OS per window
//! (ADR-0217 §5).
//!
//! The framework collects [`InputRegion`]s in window-logical coordinates from layout and
//! passes them to [`PlatformWindow::set_input_regions`](crate::PlatformWindow::set_input_regions)
//! on every composited frame. The backend projects them to device pixels once, with the
//! window's current scale, and updates the OS only when the projection changed.

use flui_foundation::geometry::Rect;

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
