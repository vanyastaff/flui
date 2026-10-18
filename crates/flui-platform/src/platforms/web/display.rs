//! The Web display source.
//!
//! A browser tab has one screen. Its size comes from `window.screen` in CSS
//! pixels, scaled by `devicePixelRatio` to the device pixels the snapshot
//! carries. The refresh period is not read yet and stays unknown.

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform_api::{DisplayInfo, DisplayKey, DisplaySnapshot};

use crate::error::PlatformError;
use crate::shared::display_source::DisplaySource;

/// Read the browser's screen and publish the result through `source`.
pub(super) fn sample_screen(source: &DisplaySource) -> Result<DisplaySnapshot, PlatformError> {
    let window = web_sys::window().ok_or_else(|| PlatformError::Displays {
        message: "no global window".into(),
    })?;
    let screen = window.screen().map_err(|_| PlatformError::Displays {
        message: "the window has no screen".into(),
    })?;
    let scale_factor = window.device_pixel_ratio();
    let to_device = |css: Result<i32, _>, fallback: i32| {
        (f64::from(css.unwrap_or(fallback)) * scale_factor).round() as i32
    };
    let bounds = Bounds::new(
        Point::new(0, 0),
        Size::new(
            to_device(screen.width(), 1920),
            to_device(screen.height(), 1080),
        ),
    );
    let displays = source
        .mint(&DisplayKey::Web("primary".into()))
        .map(|id| {
            vec![DisplayInfo::new(id, "Browser Screen", bounds, scale_factor).with_primary(true)]
        })
        .unwrap_or_default();
    source.publish(displays)
}
