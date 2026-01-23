//! Winit display implementation
//!
//! Wraps winit's MonitorHandle to implement PlatformDisplay.

use crate::traits::{DisplayId, PlatformDisplay};
use flui_types::geometry::{Bounds, DevicePixels, Point, Size};
use winit::monitor::MonitorHandle;

/// Winit display wrapper
///
/// Wraps `winit::monitor::MonitorHandle` to provide display information.
pub struct WinitDisplay {
    monitor: MonitorHandle,
    id: DisplayId,
    is_primary: bool,
}

impl WinitDisplay {
    /// Create a new WinitDisplay from a MonitorHandle
    pub fn new(monitor: MonitorHandle, id: u64, is_primary: bool) -> Self {
        Self {
            monitor,
            id: DisplayId(id),
            is_primary,
        }
    }

    /// Get the underlying MonitorHandle
    pub fn monitor(&self) -> &MonitorHandle {
        &self.monitor
    }
}

impl PlatformDisplay for WinitDisplay {
    fn id(&self) -> DisplayId {
        self.id
    }

    fn name(&self) -> String {
        self.monitor
            .name()
            .unwrap_or_else(|| format!("Display {}", self.id.0))
    }

    fn bounds(&self) -> Bounds<DevicePixels> {
        use flui_types::geometry::device_px;

        let position = self.monitor.position();
        let size = self.monitor.size();

        Bounds::new(
            Point::new(device_px(position.x), device_px(position.y)),
            Size::new(device_px(size.width as i32), device_px(size.height as i32)),
        )
    }

    fn usable_bounds(&self) -> Bounds<DevicePixels> {
        // winit doesn't provide usable bounds directly
        // Fall back to full bounds
        self.bounds()
    }

    fn scale_factor(&self) -> f64 {
        self.monitor.scale_factor()
    }

    fn refresh_rate(&self) -> f64 {
        self.monitor
            .refresh_rate_millihertz()
            .map(|mhz| mhz as f64 / 1000.0)
            .unwrap_or(60.0)
    }

    fn is_primary(&self) -> bool {
        self.is_primary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_id() {
        // We can't easily test with real MonitorHandle in unit tests
        // since it requires a window/event loop context.
        // This is tested via integration tests instead.
        let id = DisplayId(42);
        assert_eq!(id.0, 42);
    }
}
