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

use flui_types::geometry::{Bounds, DevicePixels, Point, Size};

use crate::traits::{DisplayId, PlatformDisplay};

/// iOS display wrapping the process's `UIScreen`.
#[derive(Debug)]
pub struct IOSDisplay {
    id: DisplayId,
    name: String,
    bounds: Bounds<DevicePixels>,
    usable_bounds: Bounds<DevicePixels>,
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
pub(super) fn device_bounds_from_points(bounds: NSRect, scale: f64) -> Bounds<DevicePixels> {
    let to_device = |points: f64| flui_types::geometry::device_px((points * scale).round() as i32);
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

    fn bounds(&self) -> Bounds<DevicePixels> {
        self.bounds
    }

    fn usable_bounds(&self) -> Bounds<DevicePixels> {
        self.usable_bounds
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn is_primary(&self) -> bool {
        self.is_primary
    }
}

#[cfg(test)]
mod tests {
    use super::device_bounds_from_points;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect {
            origin: NSPoint { x, y },
            size: NSSize {
                width: w,
                height: h,
            },
        }
    }

    /// Retina 3x: a 390x844 point screen becomes 1170x2532 device pixels.
    #[test]
    fn points_scale_to_device_pixels() {
        let b = device_bounds_from_points(rect(0.0, 0.0, 390.0, 844.0), 3.0);
        assert_eq!(b.size.width.0, 1170);
        assert_eq!(b.size.height.0, 2532);
    }

    /// A non-integer product must round rather than truncate, or a
    /// fractional point size would shave a pixel off the render target.
    #[test]
    fn fractional_points_round_to_the_nearest_pixel() {
        let b = device_bounds_from_points(rect(0.0, 0.0, 10.5, 10.5), 1.5);
        assert_eq!(b.size.width.0, 16);
        assert_eq!(b.size.height.0, 16);
    }

    #[test]
    fn a_nonzero_origin_survives_the_conversion() {
        let b = device_bounds_from_points(rect(10.0, 20.0, 30.0, 40.0), 2.0);
        assert_eq!(b.origin.x.0, 20);
        assert_eq!(b.origin.y.0, 40);
    }
}
