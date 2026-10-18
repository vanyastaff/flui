//! The iOS display source (`UIScreen`).
//!
//! `UIScreen.mainScreen` is the one display an iOS app renders to. Its
//! `bounds` are in points (logical, top-left origin), and `nativeScale` is the
//! device pixel ratio (2.0 or 3.0 on current hardware), so bounds are scaled by
//! it to the device pixels the snapshot carries. The refresh period is not read
//! yet and stays unknown.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::NSRect;
use objc2_ui_kit::UIScreen;

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayInfo, DisplayKey, DisplaySnapshot};

use crate::error::PlatformError;
use crate::shared::display_source::DisplaySource;

/// Sample the main screen and publish the result through `source`.
///
/// The main screen is a process-lifetime singleton, so its address is a stable
/// key for the session; a second screen would be keyed by the object the
/// snapshot retains instead.
pub(super) fn sample_main_screen(
    mtm: MainThreadMarker,
    source: &DisplaySource,
) -> Result<DisplaySnapshot, PlatformError> {
    let screen: Retained<UIScreen> = UIScreen::mainScreen(mtm);
    let key = DisplayKey::IosScreen(Retained::as_ptr(&screen) as usize as u64);
    let displays = source
        .mint(&key)
        .map(|id| {
            let bounds = device_bounds_from_points(screen.bounds(), screen.nativeScale());
            // iOS has no per-display "usable bounds" separate from the frame:
            // safe-area insets carve out the notch and home indicator for
            // CONTENT, not for the display itself.
            vec![
                DisplayInfo::new(id, "iOS Display", bounds, screen.nativeScale())
                    .with_primary(true),
            ]
        })
        .unwrap_or_default();
    source.publish(displays)
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
