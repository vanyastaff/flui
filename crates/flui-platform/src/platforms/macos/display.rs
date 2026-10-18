//! The macOS display source (`NSScreen`).
//!
//! On `objc2` / `objc2-app-kit`, like the rest of the backend. Each screen
//! becomes a [`DisplayInfo`] whose refresh period is the rate of the display's
//! current mode (`CGDisplayCopyDisplayMode`), so a 120 Hz ProMotion panel
//! reports 120 Hz and a mode the display cannot describe reports nothing.
//!
//! The screen key is the `CGDirectDisplayID`, a label that is stable for the
//! session of one attached display; the display-UUID source replaces it.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayInfo, DisplayKey, DisplaySnapshot, RefreshPeriod};

use crate::error::PlatformError;
use crate::shared::display_period::core_graphics_refresh_period;
use crate::shared::display_source::DisplaySource;

/// The `CGDirectDisplayID` a screen carries in its `deviceDescription`'s
/// `"NSScreenNumber"` entry, or `0` when the screen reports none.
fn display_id_of(screen: &NSScreen) -> u64 {
    // The `deviceDescription` value for "NSScreenNumber" is an `NSNumber`
    // holding the display id; `unsignedLongLongValue` reads it at full width.
    let key = NSString::from_str("NSScreenNumber");
    let description: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> =
        screen.deviceDescription();
    let Some(value) = description
        .objectForKey(&key)
        .and_then(|value| value.downcast::<NSNumber>().ok())
    else {
        return 0;
    };
    value.unsignedLongLongValue()
}

/// The key under which `screen` is issued a display id.
pub(super) fn display_key_of(screen: &NSScreen) -> DisplayKey {
    DisplayKey::Backend(format!("cgdisplay:{}", display_id_of(screen)))
}

/// The refresh period of the display `screen` is on, or `None` when the
/// display cannot be reached or does not report a rate.
///
/// Reads the refresh rate of the display's *current* mode
/// (`CGDisplayModeGetRefreshRate` over `CGDisplayCopyDisplayMode`), the same
/// source the winit backend reads. A rate of 0 — CoreGraphics reports 0 for
/// modes it cannot describe, and some displays do — is unknown rather than an
/// infinite period.
///
/// Call on the owner lane: this messages the screen, which is AppKit traffic
/// this backend keeps there.
pub(super) fn refresh_period_for_screen(screen: &NSScreen) -> Option<RefreshPeriod> {
    // `CGDirectDisplayID` is a `u32`; the `NSScreenNumber` number is a wider
    // CFNumber, so narrow it explicitly.
    let display_id = u32::try_from(display_id_of(screen)).ok()?;
    let mode = core_graphics::display::CGDisplay::new(display_id).display_mode()?;
    core_graphics_refresh_period(mode.refresh_rate())
}

/// Sample every screen (`+[NSScreen screens]`) and publish the result through
/// `source`. Main-thread only.
pub(super) fn sample_screens(
    mtm: MainThreadMarker,
    source: &DisplaySource,
) -> Result<DisplaySnapshot, PlatformError> {
    let screens: Retained<NSArray<NSScreen>> = NSScreen::screens(mtm);
    let mut displays = Vec::with_capacity(screens.count());
    // The first screen is always the primary (main) display.
    for (index, screen) in screens.iter().enumerate() {
        let Some(id) = source.mint(&display_key_of(&screen)) else {
            continue;
        };
        let scale = screen.backingScaleFactor();
        // NSScreen frames are in points (logical units, bottom-left origin);
        // the snapshot carries device pixels, so scale by `backingScaleFactor`
        // before converting.
        let to_device = |points: f64| (points * scale).round() as i32;
        let frame = screen.frame();
        let visible = screen.visibleFrame();
        let bounds = Bounds {
            origin: Point::new(to_device(frame.origin.x), to_device(frame.origin.y)),
            size: Size::new(to_device(frame.size.width), to_device(frame.size.height)),
        };
        let usable_bounds = Bounds {
            origin: Point::new(to_device(visible.origin.x), to_device(visible.origin.y)),
            size: Size::new(
                to_device(visible.size.width),
                to_device(visible.size.height),
            ),
        };
        let mut display = DisplayInfo::new(
            id,
            format!("Display {}", display_id_of(&screen)),
            bounds,
            scale,
        )
        .with_usable_bounds(usable_bounds)
        .with_primary(index == 0);
        display.refresh_period = refresh_period_for_screen(&screen);
        displays.push(display);
    }
    tracing::debug!("Sampled {} displays", displays.len());
    source.publish(displays)
}
