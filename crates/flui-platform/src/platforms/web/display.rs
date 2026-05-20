//! Web display implementation

use flui_types::geometry::{Bounds, DevicePixels, Point, Size, device_px};

use crate::traits::{DisplayId, PlatformDisplay};

pub struct WebDisplay {
    width: i32,
    height: i32,
    scale_factor: f64,
}

unsafe impl Send for WebDisplay {}
unsafe impl Sync for WebDisplay {}

impl WebDisplay {
    pub fn from_browser() -> Self {
        let window = web_sys::window().expect("no global window");
        let screen = window.screen().expect("no screen");
        Self {
            width: screen.width().unwrap_or(1920),
            height: screen.height().unwrap_or(1080),
            scale_factor: window.device_pixel_ratio(),
        }
    }
}

impl PlatformDisplay for WebDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(0)
    }

    fn name(&self) -> String {
        "Browser Screen".to_string()
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        Bounds::new(
            Point::new(device_px(0), device_px(0)),
            Size::new(device_px(self.width), device_px(self.height)),
        )
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn is_primary(&self) -> bool {
        true
    }
}
