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
                name: format!("Display {}", display_id),
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
