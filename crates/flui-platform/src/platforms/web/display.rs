//! Web display implementation

use flui_foundation::geometry::{Bounds, Point, Size};

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
        let scale_factor = window.device_pixel_ratio();
        // Screen dimensions are CSS pixels; PlatformDisplay exposes device pixels.
        Self {
            width: (f64::from(screen.width().unwrap_or(1920)) * scale_factor).round() as i32,
            height: (f64::from(screen.height().unwrap_or(1080)) * scale_factor).round() as i32,
            scale_factor,
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

    fn bounds(&self) -> Bounds<i32> {
        Bounds::new(Point::new(0, 0), Size::new(self.width, self.height))
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn is_primary(&self) -> bool {
        true
    }
}
