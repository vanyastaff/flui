//! Input event types for cross-platform support
//!
//! This module re-exports W3C-compliant event types from `ui-events` crate
//! and provides platform-specific utilities for event conversion.
//!
//! # Design Philosophy (Option A: W3C Events)
//!
//! 1. **W3C Compliant** - Use standard `ui-events` types everywhere
//! 2. **Platform Agnostic** - Same types work on desktop, mobile, and web
//! 3. **No Duplication** - Platform converts native events → ui-events
//! 4. **Type Safe** - Concrete types (no generics in public API)
//!
//! # Architecture
//!
//! ```text
//! OS Events (Win32, Wayland, Cocoa)
//!     ↓
//! Platform Layer (converts to logical pixels)
//!     ↓
//! ui-events types (W3C PointerEvent, KeyboardEvent)
//!     ↓
//! flui_interaction (gesture recognition)
//! ```
//!
//! # Migration from GPUI-style events
//!
//! This file previously contained custom `PointerEvent`, `Velocity`, etc.
//! Those have been removed to avoid duplication with `ui-events` crate.
//!
//! **Before (custom types):**
//! ```rust,ignore
//! pub struct PointerEvent {
//!     pub position: Point<Pixels>,
//!     pub delta: Point<Pixels>,  // ❌ Wrong! Should be PixelDelta
//!     // ...
//! }
//! ```
//!
//! **After (W3C types):**
//! ```rust,ignore
//! use ui_events::pointer::PointerEvent;  // ✅ Standard W3C type
//! ```

// ============================================================================
// Re-exports from ui-events (W3C compliant)
// ============================================================================

/// Re-export W3C pointer events
pub use ui_events::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerType, PointerUpdate,
};

/// Re-export keyboard types from keyboard-types crate
pub use keyboard_types::{Key, Modifiers};

/// Re-export scroll events
pub use ui_events::ScrollDelta;

// ============================================================================
// Platform-specific utilities
// ============================================================================

use flui_types::geometry::{Offset, PixelDelta, Pixels};
use std::time::Instant;

/// Simple keyboard event (wrapper since ui-events 0.3 doesn't have KeyboardEvent)
#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    pub key: Key,
    pub modifiers: Modifiers,
    pub is_down: bool,
    pub is_repeat: bool,
}

/// Platform input event wrapper
///
/// This enum wraps ui-events types for platform-specific dispatching.
/// Platform implementations convert native events to these types.
#[derive(Debug, Clone)]
pub enum PlatformInput {
    /// Pointer event (mouse, touch, pen) - W3C compliant
    Pointer(PointerEvent),

    /// Keyboard event
    Keyboard(KeyboardEvent),
}

impl PlatformInput {
    /// Extract pointer event if this is a pointer input
    pub fn as_pointer(&self) -> Option<&PointerEvent> {
        match self {
            PlatformInput::Pointer(event) => Some(event),
            _ => None,
        }
    }

    /// Extract keyboard event if this is a keyboard input
    pub fn as_keyboard(&self) -> Option<&KeyboardEvent> {
        match self {
            PlatformInput::Keyboard(event) => Some(event),
            _ => None,
        }
    }
}

// ============================================================================
// Platform conversion utilities
// ============================================================================

/// Convert device (physical) pixels to logical pixels
///
/// Platform implementations should use this to convert native coordinates
/// to framework coordinates (logical pixels).
///
/// # Example
///
/// ```rust,ignore
/// // Windows: WM_MOUSEMOVE gives physical pixels
/// let physical_x = 1920; // On 2x DPI display
/// let physical_y = 1080;
/// let scale_factor = 2.0;
///
/// let logical_pos = Offset::new(
///     Pixels(device_to_logical(physical_x as f32, scale_factor)),
///     Pixels(device_to_logical(physical_y as f32, scale_factor))
/// );
/// // Result: (960, 540) logical pixels
/// ```
#[inline]
pub fn device_to_logical(device_pixels: f32, scale_factor: f32) -> f32 {
    device_pixels / scale_factor
}

/// Convert logical pixels to device (physical) pixels
#[inline]
pub fn logical_to_device(logical_pixels: f32, scale_factor: f32) -> f32 {
    logical_pixels * scale_factor
}

/// Helper to create an Offset from raw coordinates
#[inline]
pub fn offset_from_coords(x: f32, y: f32) -> Offset<Pixels> {
    Offset::new(Pixels(x), Pixels(y))
}

/// Helper to create a delta Offset from raw coordinates
#[inline]
pub fn delta_offset_from_coords(dx: f32, dy: f32) -> Offset<PixelDelta> {
    Offset::new(PixelDelta(dx), PixelDelta(dy))
}

// ============================================================================
// Velocity tracking (moved from custom implementation)
// ============================================================================

/// Velocity tracker for gesture recognition
///
/// **Note:** This used to be a custom implementation. Now it should use
/// types from `flui_types::gestures::Velocity`. We keep this minimal
/// version for platform layer only.
///
/// For full velocity tracking, use `flui_interaction::processing::VelocityTracker`.
#[derive(Debug, Clone)]
pub struct BasicVelocityTracker {
    samples: Vec<VelocitySample>,
    max_samples: usize,
}

#[derive(Debug, Clone, Copy)]
struct VelocitySample {
    timestamp: Instant,
    position: Offset<Pixels>,
}

impl BasicVelocityTracker {
    /// Create a new velocity tracker
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(20),
            max_samples: 20,
        }
    }

    /// Add a sample
    pub fn add_sample(&mut self, timestamp: Instant, position: Offset<Pixels>) {
        self.samples.push(VelocitySample {
            timestamp,
            position,
        });

        if self.samples.len() > self.max_samples {
            self.samples.remove(0);
        }
    }

    /// Calculate velocity (pixels per second)
    pub fn velocity(&self) -> Option<Offset<Pixels>> {
        use flui_types::geometry::px;

        if self.samples.len() < 2 {
            return None;
        }

        let first = self.samples.first()?;
        let last = self.samples.last()?;

        let dt = last.timestamp.duration_since(first.timestamp);
        if dt.as_secs_f32() < 0.001 {
            return None;
        }

        let dx = last.position.dx.0 - first.position.dx.0;
        let dy = last.position.dy.0 - first.position.dy.0;
        let dt_secs = dt.as_secs_f32();

        Some(Offset::new(px(dx / dt_secs), px(dy / dt_secs)))
    }

    /// Clear samples
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl Default for BasicVelocityTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Platform helpers
// ============================================================================

/// Timestamp provider for platform events
pub trait TimestampProvider {
    fn now() -> Instant {
        Instant::now()
    }
}

/// Default timestamp provider using std::time::Instant
pub struct SystemTimestamp;

impl TimestampProvider for SystemTimestamp {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_to_logical_conversion() {
        assert_eq!(device_to_logical(100.0, 1.0), 100.0);
        assert_eq!(device_to_logical(200.0, 2.0), 100.0);
        assert_eq!(device_to_logical(150.0, 1.5), 100.0);
    }

    #[test]
    fn test_logical_to_device_conversion() {
        assert_eq!(logical_to_device(100.0, 1.0), 100.0);
        assert_eq!(logical_to_device(100.0, 2.0), 200.0);
        assert_eq!(logical_to_device(100.0, 1.5), 150.0);
    }

    #[test]
    fn test_offset_helpers() {
        let offset = offset_from_coords(10.0, 20.0);
        assert_eq!(offset.dx.0, 10.0);
        assert_eq!(offset.dy.0, 20.0);

        let delta = delta_offset_from_coords(5.0, -3.0);
        assert_eq!(delta.dx.0, 5.0);
        assert_eq!(delta.dy.0, -3.0);
    }

    #[test]
    fn test_velocity_tracker() {
        let mut tracker = BasicVelocityTracker::new();
        let t0 = Instant::now();

        tracker.add_sample(t0, offset_from_coords(0.0, 0.0));

        // Simulate 100ms later, moved 50 pixels
        std::thread::sleep(std::time::Duration::from_millis(100));
        let t1 = Instant::now();
        tracker.add_sample(t1, offset_from_coords(50.0, 0.0));

        if let Some(vel) = tracker.velocity() {
            // Should be ~500 pixels/sec (50px in 0.1s)
            assert!(vel.dx > 400.0 && vel.dx < 600.0);
        }
    }
}
