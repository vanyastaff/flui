//! iOS display (UIScreen) implementation.
//!
//! `UIScreen.mainScreen` is the one display an iOS app renders to. Its
//! `bounds` are in points (logical, top-left origin), and `scale` is the
//! device pixel ratio (2.0 or 3.0 on current hardware), so bounds are scaled
//! by `nativeScale` for the `PlatformDisplay` device-pixel contract.

use std::sync::Arc;

use objc2::MainThreadMarker;
use objc2_foundation::NSRect;
use objc2_ui_kit::UIScreen;

use flui_foundation::geometry::{Bounds, Point, Size};

use crate::traits::{DisplayId, PlatformDisplay};

/// iOS display wrapping the process's `UIScreen`.
#[derive(Debug)]
pub struct IOSDisplay {
    id: DisplayId,
    name: String,
    bounds: Bounds<i32>,
    usable_bounds: Bounds<i32>,
    scale_factor: f64,
    is_primary: bool,
}

impl IOSDisplay {
    /// Read the main screen's metrics. Must run on the main thread
    /// (`UIScreen.mainScreen` is main-thread-only in objc2's model).
    pub fn new(mtm: MainThreadMarker, is_primary: bool) -> Self {
        let screen = UIScreen::mainScreen(mtm);
        Self::from_screen(&screen, is_primary)
    }

    /// Build from an already-obtained screen. Separate from [`Self::new`] so
    /// the conversion below is reachable from a test with a handed-in screen.
    pub fn from_screen(screen: &UIScreen, is_primary: bool) -> Self {
        let bounds = screen.bounds();
        let scale = screen.nativeScale();
        let bounds_dp = device_bounds_from_points(bounds, scale);

        // iOS has no per-display "usable bounds" concept separate from the
        // frame — safe-area insets carve out the notch/home-indicator region
        // for CONTENT, not for the display itself, and no `PlatformDisplay`
        // method reports them. The frame is the honest answer here.
        let usable_bounds = bounds_dp;

        Self {
            id: DisplayId(0),
            name: "iOS Display".to_string(),
            bounds: bounds_dp,
            usable_bounds,
            scale_factor: scale,
            is_primary,
        }
    }

    /// The main screen as a `PlatformDisplay`, for `Platform::displays`/
    /// `primary_display`.
    pub fn main_display(mtm: MainThreadMarker) -> Arc<dyn PlatformDisplay> {
        Arc::new(Self::new(mtm, true))
    }
}

/// The `(points, scale)` → device-pixel conversion, free-standing so it can
/// be tested without a display. `CGRect`/`CGSize` are plain C structs.
pub(super) fn device_bounds_from_points(bounds: NSRect, scale: f64) -> Bounds<i32> {
    let to_device = |points: f64| (points * scale).round() as i32;
    Bounds {
        origin: Point::new(to_device(bounds.origin.x), to_device(bounds.origin.y)),
        size: Size::new(to_device(bounds.size.width), to_device(bounds.size.height)),
    }
}

impl PlatformDisplay for IOSDisplay {
    fn id(&self) -> DisplayId {
        self.id
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn bounds(&self) -> Bounds<i32> {
        self.bounds
    }

    fn usable_bounds(&self) -> Bounds<i32> {
        self.usable_bounds
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn is_primary(&self) -> bool {
        self.is_primary
    }
}
