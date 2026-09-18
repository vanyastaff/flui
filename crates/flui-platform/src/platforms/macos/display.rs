//! macOS display (NSScreen) implementation

use std::sync::Arc;

use cocoa::{
    base::{id, nil},
    foundation::NSRect,
};
use flui_types::geometry::{Bounds, DevicePixels, Point, Size};
use objc::{class, msg_send, sel, sel_impl};

use crate::traits::{DisplayId, PlatformDisplay};

/// macOS display wrapper around NSScreen
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
    /// Create a MacOSDisplay from NSScreen
    ///
    /// # Safety
    ///
    /// `screen` must be a valid, live `NSScreen*` for the duration of the
    /// call.
    pub unsafe fn new(screen: id, is_primary: bool) -> Self {
        // SAFETY: caller guarantees `screen` is a live NSScreen*; all
        // messages are documented NSScreen getters, and `deviceDescription`
        // values are read before the autorelease pool drains.
        unsafe {
            // Get screen frame (full bounds including menu bar)
            let frame: NSRect = msg_send![screen, frame];

            // Get visible frame (excluding menu bar and dock)
            let visible_frame: NSRect = msg_send![screen, visibleFrame];

            // Get backing scale factor (1.0 for non-Retina, 2.0 for Retina)
            let scale: f64 = msg_send![screen, backingScaleFactor];

            // Get device description for display ID
            let description: id = msg_send![screen, deviceDescription];
            let display_id_key: id =
                msg_send![class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
            let display_id_value: id = msg_send![description, objectForKey: display_id_key];
            let display_id: u64 = msg_send![display_id_value, unsignedLongLongValue];

            // NSScreen frames are in points (logical units, bottom-left
            // origin); the PlatformDisplay contract wants device pixels, so
            // scale by backingScaleFactor before converting.
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
/// # Safety
///
/// `screen` must be a valid, live `NSScreen*`. Call on the owner lane: this
/// messages the screen, which is AppKit traffic this backend keeps there.
pub(super) unsafe fn refresh_period_for_screen(screen: id) -> Option<std::time::Duration> {
    // SAFETY: the caller guarantees `screen` is live; `deviceDescription` and
    // its values are read before the autorelease pool drains, and
    // `CGDisplay::new`/`display_mode` are safe wrappers whose `CGDisplayMode`
    // is released on drop (create rule).
    unsafe {
        if screen == nil {
            return None;
        }

        let description: id = msg_send![screen, deviceDescription];
        let display_id_key: id =
            msg_send![class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
        let display_id_value: id = msg_send![description, objectForKey: display_id_key];
        if display_id_value == nil {
            return None;
        }

        // `unsignedIntValue`, not the `unsignedLongLongValue` the bounds read
        // above uses: `NSScreenNumber` holds a `CGDirectDisplayID`, which is a
        // `u32`, so this is the CFNumber's actual width.
        let display_id: u32 = msg_send![display_id_value, unsignedIntValue];

        let mode = core_graphics::display::CGDisplay::new(display_id).display_mode()?;
        period_from_refresh_hz(mode.refresh_rate())
    }
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

/// Enumerate all displays using NSScreen
pub fn enumerate_displays() -> Vec<Arc<dyn PlatformDisplay>> {
    // SAFETY: `+[NSScreen screens]` returns an autoreleased NSArray of live
    // NSScreen objects; all elements are consumed before the pool drains.
    unsafe {
        let screens: id = msg_send![class!(NSScreen), screens];
        if screens == nil {
            tracing::warn!("NSScreen.screens returned nil");
            return Vec::new();
        }

        let count: usize = msg_send![screens, count];
        let mut displays: Vec<Arc<dyn PlatformDisplay>> = Vec::with_capacity(count);

        // First screen is always primary (main display)
        for i in 0..count {
            let screen: id = msg_send![screens, objectAtIndex: i];
            if screen != nil {
                let is_primary = i == 0;
                let display = Arc::new(MacOSDisplay::new(screen, is_primary));
                displays.push(display);
            }
        }

        tracing::debug!("Enumerated {} displays", displays.len());
        displays
    }
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
