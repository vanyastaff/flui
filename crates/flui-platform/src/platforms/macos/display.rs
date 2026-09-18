//! macOS display (NSScreen) implementation.
//!
//! Migrated to `objc2` / `objc2-app-kit`. [`refresh_period_for_screen`] keeps a
//! raw-pointer parameter on purpose: the window module (still on the older
//! `cocoa`/`objc` stack mid-migration) calls it with an `id`, and the two macro
//! systems cannot coexist in one file. The pointer is cast to a typed
//! `&NSScreen` inside, which is the only unsafe step and is covered by this
//! module's own contract.

use std::sync::Arc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use flui_types::geometry::{Bounds, DevicePixels, Point, Size};

use crate::traits::{DisplayId, PlatformDisplay};

/// macOS display wrapper around `NSScreen`.
#[derive(Debug)]
pub struct MacOSDisplay {
    id: DisplayId,
    name: String,
    bounds: Bounds<DevicePixels>,
    usable_bounds: Bounds<DevicePixels>,
    scale_factor: f64,
    is_primary: bool,
}

impl MacOSDisplay {
    /// Create a `MacOSDisplay` from a live `NSScreen`.
    ///
    /// Must run on the main thread (`NSScreen` is main-thread-only in objc2's
    /// model).
    pub fn new(screen: &NSScreen, is_primary: bool) -> Self {
        let frame = screen.frame();
        let visible_frame = screen.visibleFrame();
        let scale = screen.backingScaleFactor();

        // NSScreen frames are in points (logical units, bottom-left origin);
        // the PlatformDisplay contract wants device pixels, so scale by
        // `backingScaleFactor` before converting.
        let to_device =
            |points: f64| flui_types::geometry::device_px((points * scale).round() as i32);

        let bounds = Bounds {
            origin: Point::new(to_device(frame.origin.x), to_device(frame.origin.y)),
            size: Size::new(to_device(frame.size.width), to_device(frame.size.height)),
        };
        let usable_bounds = Bounds {
            origin: Point::new(
                to_device(visible_frame.origin.x),
                to_device(visible_frame.origin.y),
            ),
            size: Size::new(
                to_device(visible_frame.size.width),
                to_device(visible_frame.size.height),
            ),
        };

        let display_id = display_id_of(screen);

        Self {
            id: DisplayId(display_id),
            name: format!("Display {display_id}"),
            bounds,
            usable_bounds,
            scale_factor: scale,
            is_primary,
        }
    }
}

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

impl PlatformDisplay for MacOSDisplay {
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

/// The inter-frame period of the display `screen` is on, or `None` when the
/// display cannot be reached or does not report a rate.
///
/// Reads the refresh rate of the display's *current* mode
/// (`CGDisplayModeGetRefreshRate` over `CGDisplayCopyDisplayMode`), which is
/// the same source the winit backend reads, so both backends answer
/// `PlatformWindow::refresh_period` the same way. A rate of 0 — CoreGraphics
/// reports 0 for modes it cannot describe, and some displays do — is treated
/// as unknown rather than turned into an infinite period.
///
/// Takes a raw pointer because the caller is `window.rs`, still on the
/// `cocoa`/`objc` stack: the two macro systems cannot coexist in one file, so
/// this module's boundary accepts the pointer and casts it here.
///
/// # Safety
///
/// `screen` must be a live `NSScreen*` or null. Call on the owner lane: this
/// messages the screen, which is AppKit traffic this backend keeps there.
pub(super) unsafe fn refresh_period_for_screen(
    screen: *mut std::ffi::c_void,
) -> Option<std::time::Duration> {
    if screen.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `screen` points to a live `NSScreen`; the
    // borrow is scoped to this call.
    let screen: &NSScreen = unsafe { &*(screen as *const NSScreen) };

    let display_id = display_id_of(screen);
    // `CGDirectDisplayID` is a `u32`; the `NSScreenNumber` number is a wider
    // CFNumber, so narrow it explicitly.
    let display_id = u32::try_from(display_id).ok()?;

    let mode = core_graphics::display::CGDisplay::new(display_id).display_mode()?;
    period_from_refresh_hz(mode.refresh_rate())
}

/// Convert a refresh rate in Hz into the interval between two frames, or
/// `None` for a value that does not describe a cadence (zero, negative, NaN,
/// infinite).
///
/// Free-standing and AppKit-free so the arithmetic is testable without a
/// display: it is the part that can be wrong for inputs this machine cannot
/// produce, and neither this backend's path nor the winit implementation it
/// mirrors had a test for it.
pub(super) fn period_from_refresh_hz(hz: f64) -> Option<std::time::Duration> {
    if !hz.is_finite() || hz <= 0.0 {
        return None;
    }
    Some(std::time::Duration::from_secs_f64(1.0 / hz))
}

/// Enumerate all displays using `+[NSScreen screens]`. Main-thread only.
pub fn enumerate_displays() -> Vec<Arc<dyn PlatformDisplay>> {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("NSScreen enumeration requested off the main thread; returning none");
        return Vec::new();
    };

    let screens: Retained<NSArray<NSScreen>> = NSScreen::screens(mtm);
    let mut displays: Vec<Arc<dyn PlatformDisplay>> = Vec::with_capacity(screens.count());

    // First screen is always primary (main display).
    for (i, screen) in screens.iter().enumerate() {
        let display = Arc::new(MacOSDisplay::new(&screen, i == 0));
        displays.push(display);
    }

    tracing::debug!("Enumerated {} displays", displays.len());
    displays
}

#[cfg(test)]
mod tests {
    use super::period_from_refresh_hz;
    use std::time::Duration;

    /// The panel this backend's pacing was measured on: 100 Hz → 10 ms, the
    /// value `refresh_period()` exists to report in place of the runner's
    /// 60 Hz default.
    #[test]
    fn refresh_rate_becomes_its_period() {
        assert_eq!(
            period_from_refresh_hz(100.0),
            Some(Duration::from_millis(10))
        );
    }

    /// A fractional rate must survive the conversion: rounding 59.94 up to
    /// 60 would pace a broadcast-rate panel against the wrong interval.
    #[test]
    fn a_fractional_rate_keeps_its_precision() {
        let period = period_from_refresh_hz(59.94)
            .expect("59.94 Hz is a cadence")
            .as_secs_f64();
        assert!((period - 0.016_683_35).abs() < 1e-9, "period={period}");
        assert_ne!(
            period_from_refresh_hz(59.94),
            period_from_refresh_hz(60.0),
            "a fractional rate was rounded to its neighbour"
        );
    }

    /// CoreGraphics reports 0 for modes it cannot describe, and some displays
    /// do; a NaN or negative could only come from a corrupted mode. All of
    /// them mean "unknown", never an infinite or negative period.
    #[test]
    fn a_rate_that_is_not_a_cadence_is_unknown() {
        for hz in [
            0.0,
            -1.0,
            -100.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(period_from_refresh_hz(hz), None, "hz={hz}");
        }
    }
}
