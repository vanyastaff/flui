//! The details of a smart zoom, whichever way it arrived.

use flui_foundation::geometry::Offset;

use crate::events::PointerKind;

/// Where a smart zoom came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SmartZoomSource {
    /// The platform's own gesture: a two-finger double-tap on a macOS
    /// trackpad (`SmartZoomEvent`).
    Platform,
}

/// A smart zoom: zoom to fit what lies under `local_position`, or back out.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SmartZoomDetails {
    /// The point to zoom about, in the root's space.
    pub global_position: Offset<f64>,
    /// The point to zoom about, in the receiving widget's space.
    pub local_position: Offset<f64>,
    /// The device the zoom came from.
    pub kind: PointerKind,
    /// How the zoom arrived.
    pub source: SmartZoomSource,
}

impl SmartZoomDetails {
    /// A smart zoom about `local_position` (`global_position` in the root's
    /// space) from a `kind` device, arrived by `source`.
    #[must_use]
    pub const fn new(
        global_position: Offset<f64>,
        local_position: Offset<f64>,
        kind: PointerKind,
        source: SmartZoomSource,
    ) -> Self {
        Self {
            global_position,
            local_position,
            kind,
            source,
        }
    }

    /// The details of the platform's smart zoom, from its dispatch.
    #[must_use]
    pub fn from_platform(dispatch: crate::pointer::SmartZoomDispatch) -> Self {
        let offset = |event: &crate::events::SmartZoomEvent| {
            let point = event.position.get();
            Offset::new(point.x, point.y)
        };
        Self::new(
            offset(&dispatch.global),
            offset(&dispatch.local),
            dispatch.local.pointer().kind,
            SmartZoomSource::Platform,
        )
    }
}
