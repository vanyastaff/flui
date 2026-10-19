//! The screen edges a set of system-gesture exclusion regions claims, for an OS that
//! defers its edge gestures per edge rather than per rectangle (UIKit's
//! `preferredScreenEdgesDeferringSystemGestures`).
//!
//! Cfg-free, so Linux CI runs the rule every backend that needs it uses.

use flui_foundation::geometry::Size;
use flui_platform_api::input_regions::{DeviceInputRegion, InputRegionKind};

/// The window edges whose system gestures the application asks the OS to defer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeferredEdges {
    /// The top edge.
    pub top: bool,
    /// The left edge.
    pub left: bool,
    /// The bottom edge.
    pub bottom: bool,
    /// The right edge.
    pub right: bool,
}

impl DeferredEdges {
    /// The edges that `regions` (device pixels) reach in a window `window` device pixels
    /// in size: a system-gesture exclusion region touching or crossing an edge defers that
    /// edge's gestures. Regions of other kinds, and regions clear of every edge, defer
    /// nothing; an OS without per-rectangle exclusion cannot honour them.
    #[must_use]
    pub fn of(regions: &[DeviceInputRegion], window: Size<i32>) -> Self {
        regions
            .iter()
            .filter(|region| region.kind() == InputRegionKind::SystemGestureExclusion)
            .fold(Self::default(), |edges, region| {
                let rect = region.rect();
                Self {
                    top: edges.top || rect.top() <= 0,
                    left: edges.left || rect.left() <= 0,
                    bottom: edges.bottom || rect.bottom() >= window.height,
                    right: edges.right || rect.right() >= window.width,
                }
            })
    }
}
