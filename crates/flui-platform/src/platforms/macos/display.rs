//! macOS display (NSScreen) implementation

use crate::traits::{DisplayId, PlatformDisplay};
use flui_types::geometry::{Bounds, DevicePixels, Point, Size};
use std::sync::Arc;

use cocoa::appkit::NSScreen;
use cocoa::base::{id, nil};
use cocoa::foundation::{NSArray, NSRect};

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
    pub fn new(screen: id, is_primary: bool) -> Self {
        unsafe {
            // Get screen frame (full bounds including menu bar)
            let frame: NSRect = msg_send![screen, frame];

            // Get visible frame (excluding menu bar and dock)
            let visible_frame: NSRect = msg_send![screen, visibleFrame];

            // Get backing scale factor (1.0 for non-Retina, 2.0 for Retina)
            let scale: f64 = msg_send![screen, backingScaleFactor];

            // Get device description for display ID
            let description: id = msg_send![screen, deviceDescription];
            let display_id_key: id = msg_send![class!(NSString), stringWithUTF8String: "NSScreenNumber".as_ptr()];
            let display_id_value: id = msg_send![description, objectForKey: display_id_key];
            let display_id: u64 = msg_send![display_id_value, unsignedLongLongValue];

            // macOS coordinates are bottom-left origin, convert to top-left
            let bounds = Bounds {
                origin: Point::new(
                    flui_types::geometry::device_px(frame.origin.x as f32),
                    flui_types::geometry::device_px(frame.origin.y as f32),
                ),
                size: Size::new(
                    flui_types::geometry::device_px(frame.size.width as f32),
                    flui_types::geometry::device_px(frame.size.height as f32),
                ),
            };

            let usable_bounds = Bounds {
                origin: Point::new(
                    flui_types::geometry::device_px(visible_frame.origin.x as f32),
                    flui_types::geometry::device_px(visible_frame.origin.y as f32),
                ),
                size: Size::new(
                    flui_types::geometry::device_px(visible_frame.size.width as f32),
                    flui_types::geometry::device_px(visible_frame.size.height as f32),
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
