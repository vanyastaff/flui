//! Input event types for cross-platform support
//!
//! This module re-exports W3C-compliant event types from the `ui-events`
//! crate and provides the conversion helpers backends use to fill them.
//!
//! # Design
//!
//! 1. **W3C compliant** - standard `ui-events` types everywhere
//! 2. **Platform agnostic** - the same types work on desktop, mobile, and web
//! 3. **No duplication** - a backend converts native events into these types
//! 4. **Concrete** - no generics in the public API
//!
//! ```text
//! OS Events (Win32, Wayland, Cocoa)
//!     ↓
//! Platform backend (converts to logical pixels)
//!     ↓
//! ui-events types (W3C PointerEvent, KeyboardEvent)
//!     ↓
//! flui_interaction (gesture recognition)
//! ```
//!
//! The `ui-events` re-exports are ADR-0089 debt: this crate's own types
//! replace them before its first release.

use flui_foundation::DataTransferId;
use flui_types::geometry::{Offset, PixelDelta, Pixels, Point};
/// Re-export scroll events
pub use ui_events::ScrollDelta;
/// Re-export W3C keyboard event from ui-events
pub use ui_events::keyboard::KeyboardEvent;
/// Re-export of the `keyboard-types` key and modifier vocabulary, through
/// `ui-events` (which re-exports that crate whole).
pub use ui_events::keyboard::{Key, Modifiers};
/// Re-export W3C pointer events
pub use ui_events::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerType, PointerUpdate,
};

/// Result of dispatching an input event through a callback
///
/// Indicates whether the event was consumed by the handler and whether
/// the platform's default behavior should be suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DispatchEventResult {
    /// If false, the event was consumed and should not propagate further
    pub propagate: bool,
    /// If true, the platform's default handling should be suppressed
    pub default_prevented: bool,
    deferred: bool,
}

impl DispatchEventResult {
    /// Create a resolved callback outcome.
    #[must_use]
    pub const fn resolved(propagate: bool, default_prevented: bool) -> Self {
        Self {
            propagate,
            default_prevented,
            deferred: false,
        }
    }

    /// Provisional outcome for an input event queued by a reentrant platform
    /// callback. FLUI will deliver the event after the current callback
    /// returns; suppressing propagation and platform defaults prevents the
    /// native backend from acting on it a second time in the meantime.
    pub const DEFERRED: Self = Self {
        propagate: false,
        default_prevented: true,
        deferred: true,
    };

    /// Whether delivery was queued behind a reentrant callback and the final
    /// user callback outcome is therefore not available yet.
    #[must_use]
    pub const fn is_deferred(self) -> bool {
        self.deferred
    }
}

impl Default for DispatchEventResult {
    fn default() -> Self {
        Self::resolved(true, false)
    }
}

/// Platform input event wrapper
///
/// This enum wraps ui-events types for platform-specific dispatching.
/// Platform implementations convert native events to these types.
///
/// # Pointer positions are logical pixels
///
/// `PointerState::position` is typed `PhysicalPosition` by `ui-events`, but
/// every backend fills it with **logical** pixels — window points, not
/// device pixels — and the framework reads it that way with no further
/// scaling (`flui-interaction`'s `PointerEventExt::position`).
/// `PointerState::scale_factor` travels alongside for a consumer that needs
/// the device-pixel value back. A backend whose OS reports device pixels
/// (winit, Android) divides before filling the field; one that reports
/// points already (AppKit, UIKit, CSS pixels on the web) passes them
/// through. Getting this wrong is invisible at scale 1 and moves every
/// touch off-screen at any other scale, which is how the Android backend
/// shipped its first emulator run (2026-09-22).
#[derive(Debug, Clone)]
pub enum PlatformInput {
    /// Pointer event (mouse, touch, pen) - W3C compliant
    Pointer(PointerEvent),

    /// Keyboard event
    Keyboard(KeyboardEvent),

    /// IME composition/commit event. See [`flui_types::ImeEvent`] for the
    /// vocabulary and [`crate::PlatformTextInput`] for the
    /// window-side capability this pairs with.
    Ime(flui_types::ImeEvent),

    /// System drag-and-drop (ADR-0038). Deliberately NOT a pointer event:
    /// during an external drag the OS owns the cursor, and the gesture-arena
    /// semantics of the pointer pipeline (capture, velocity) do not apply.
    DragDrop(DragDropEvent),
}

impl PlatformInput {
    /// Extract pointer event if this is a pointer input
    #[inline]
    pub fn as_pointer(&self) -> Option<&PointerEvent> {
        match self {
            PlatformInput::Pointer(event) => Some(event),
            PlatformInput::Keyboard(_) | PlatformInput::Ime(_) | PlatformInput::DragDrop(_) => None,
        }
    }

    /// Extract keyboard event if this is a keyboard input
    #[inline]
    pub fn as_keyboard(&self) -> Option<&KeyboardEvent> {
        match self {
            PlatformInput::Keyboard(event) => Some(event),
            PlatformInput::Pointer(_) | PlatformInput::Ime(_) | PlatformInput::DragDrop(_) => None,
        }
    }

    /// Extract the IME event if this is an IME composition/commit input.
    #[inline]
    pub fn as_ime(&self) -> Option<&flui_types::ImeEvent> {
        match self {
            PlatformInput::Ime(event) => Some(event),
            PlatformInput::Pointer(_) | PlatformInput::Keyboard(_) | PlatformInput::DragDrop(_) => {
                None
            }
        }
    }

    /// Extract the drag-and-drop event if this is a DnD input.
    #[inline]
    pub fn as_drag_drop(&self) -> Option<&DragDropEvent> {
        match self {
            PlatformInput::DragDrop(event) => Some(event),
            PlatformInput::Pointer(_) | PlatformInput::Keyboard(_) | PlatformInput::Ime(_) => None,
        }
    }
}

/// The push half of the data-transfer transport (ADR-0038): stage-1 arrival
/// and stage-2/6 progress for a drag session over one window. The target's
/// reply half flows the other way, through
/// [`crate::data_transfer::DataTransferSource::update_drop_feedback`].
#[derive(Debug, Clone)]
pub enum DragDropEvent {
    /// A drag entered the window. Carries the full offer (stage 1) and the
    /// actions the source currently permits.
    Entered {
        /// The stage-1 offer minted for this drag session.
        offer: crate::data_transfer::DataTransferOffer,
        /// Actions the source currently permits.
        allowed: crate::data_transfer::TransferActions,
        /// Logical-pixel position when the backend knows it. The winit
        /// backend stamps the last tracked cursor position — `None` before
        /// any cursor event, and possibly stale on Wayland where an external
        /// drag grabs the cursor (documented backend limitation).
        position: Option<Point<Pixels>>,
    },
    /// The drag moved while over the window. `allowed` is re-stamped on
    /// every event: modifier-driven copy/move/link changes mid-drag arrive
    /// here, and the target answers them with fresh `DropFeedback`.
    Moved {
        /// The live session's offer id.
        id: DataTransferId,
        /// Actions the source permits as of this event.
        allowed: crate::data_transfer::TransferActions,
        /// Logical-pixel hover position.
        position: Point<Pixels>,
    },
    /// The user released and the backend resolved the drop from the cached
    /// feedback. `action` is the effect reported to the OS. The payload is
    /// NOT here — a stage-3 request fetches it lazily; the target calls
    /// `conclude_drop` when done.
    Dropped {
        /// The dropped session's offer id, redeemable for the payload.
        id: DataTransferId,
        /// The drop effect resolved and reported to the OS.
        action: crate::data_transfer::TransferActions,
        /// Logical-pixel drop position when the backend knows it.
        position: Option<Point<Pixels>>,
    },
    /// The drag left the window or the source cancelled; the offer is
    /// retired.
    Exited {
        /// The retired session's offer id (now stale by construction).
        id: DataTransferId,
    },
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
}
