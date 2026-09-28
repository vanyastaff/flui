//! Platform display abstraction
//!
//! Provides information about physical displays (monitors, screens).

use flui_types::geometry::{Bounds, Size};

/// Display identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayId(pub u64);

/// Platform display trait
///
/// Represents a physical display (monitor, screen). Provides information
/// about size, position, scale factor, and refresh rate.
///
/// # Example
///
/// ```rust,ignore
/// for display in platform.displays() {
///     println!("Display {}: {}x{} @ {:.1}Hz",
///         display.name(),
///         display.bounds().width(),
///         display.bounds().height(),
///         display.refresh_rate());
/// }
/// ```
pub trait PlatformDisplay: Send + Sync {
    /// Get unique identifier for this display
    fn id(&self) -> DisplayId;

    /// Get display name (e.g., "Built-in Display", "Dell U2720Q")
    fn name(&self) -> String;

    /// Get the display's bounds in global screen coordinates (device pixels)
    ///
    /// For the primary display, this usually starts at (0, 0).
    /// Secondary displays are positioned relative to the primary.
    ///
    /// Uses `Bounds<i32>` to represent physical pixel coordinates,
    /// following GPUI's type-safe approach.
    fn bounds(&self) -> Bounds<i32>;

    /// Get the display's usable bounds (excluding taskbars, menu bars, etc.)
    ///
    /// This is the area where windows can be placed without being obscured
    /// by system UI elements.
    ///
    /// Uses `Bounds<i32>` for physical pixel coordinates.
    fn usable_bounds(&self) -> Bounds<i32> {
        self.bounds() // Default: same as full bounds
    }

    /// Get the scale factor (DPI scaling)
    ///
    /// - 1.0 = standard DPI (96 DPI on Windows, 72 DPI on macOS)
    /// - 2.0 = retina/HiDPI (192 DPI on Windows, 144 DPI on macOS)
    fn scale_factor(&self) -> f64;

    /// Get the refresh rate in Hz
    fn refresh_rate(&self) -> f64 {
        60.0 // Default: assume 60Hz
    }

    /// Check if this is the primary display
    fn is_primary(&self) -> bool;

    /// Get logical size (bounds.size / scale_factor)
    ///
    /// Converts device pixels to logical pixels by dividing by the scale
    /// factor.
    fn logical_size(&self) -> Size<f64> {
        let bounds = self.bounds();
        let scale = self.scale_factor();

        // Device pixels to logical pixels: divide by the scale factor
        let device_width: i32 = bounds.size.width;
        let device_height: i32 = bounds.size.height;

        Size::new(device_width as f64 / scale, device_height as f64 / scale)
    }
}
