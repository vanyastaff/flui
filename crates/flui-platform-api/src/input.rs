//! Input event types for cross-platform support
//!
//! Platform dispatch carries the owned pointer and keyboard vocabulary
//! (ADR-0143), alongside IME and external drag-and-drop events.
//!
//! # Design
//!
//! 1. **Owned contracts** - validated FLUI input values at the public boundary
//! 2. **Platform agnostic** - the same types work on desktop, mobile, and web
//! 3. **No duplication** - a backend converts native events into these types
//! 4. **Concrete** - no generics in the public API
//!
//! ```text
//! OS Events (Win32, Wayland, Cocoa)
//!     ↓
//! Platform backend (converts to logical pixels)
//!     ↓
//! pointer::PointerEvent / keyboard::KeyEvent
//!     ↓
//! flui_interaction (gesture recognition)
//! ```
//!
use crate::{keyboard::KeyEvent, pointer::PointerEvent};
use flui_foundation::DataTransferId;
use flui_foundation::geometry::{Offset, Point};

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
/// This enum carries FLUI-owned types for platform-specific dispatching.
/// Platform implementations convert native events to these types.
///
/// # Pointer positions are logical pixels
///
/// Pointer samples use validated `PointerPosition` values in logical pixels.
/// A backend whose OS reports device pixels divides by the window scale before
/// constructing those values; one reporting points or CSS pixels passes them
/// through. Geometry and device metadata remain distinct typed values.
#[derive(Debug, Clone)]
pub enum PlatformInput {
    /// Pointer, scrolling, trackpad or device-lifecycle event.
    Pointer(PointerEvent),

    /// Keyboard event
    Keyboard(KeyEvent),

    /// IME composition/commit event. See [`ImeEvent`](crate::ImeEvent) for the
    /// vocabulary and [`crate::PlatformTextInput`] for the
    /// window-side capability this pairs with.
    Ime(crate::ImeEvent),

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
    pub fn as_keyboard(&self) -> Option<&KeyEvent> {
        match self {
            PlatformInput::Keyboard(event) => Some(event),
            PlatformInput::Pointer(_) | PlatformInput::Ime(_) | PlatformInput::DragDrop(_) => None,
        }
    }

    /// Extract the IME event if this is an IME composition/commit input.
    #[inline]
    pub fn as_ime(&self) -> Option<&crate::ImeEvent> {
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
        position: Option<Point<f64>>,
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
        position: Point<f64>,
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
        position: Option<Point<f64>>,
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
///     (device_to_logical(physical_x as f64, scale_factor)),
///     (device_to_logical(physical_y as f64, scale_factor))
/// );
/// // Result: (960, 540) logical pixels
/// ```
#[inline]
pub fn device_to_logical(device_pixels: f64, scale_factor: f64) -> f64 {
    device_pixels / scale_factor
}

/// Convert logical pixels to device (physical) pixels
#[inline]
pub fn logical_to_device(logical_pixels: f64, scale_factor: f64) -> f64 {
    logical_pixels * scale_factor
}

/// Helper to create an Offset from raw coordinates
#[inline]
pub fn offset_from_coords(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

/// Helper to create a delta Offset from raw coordinates
#[inline]
pub fn delta_offset_from_coords(dx: f64, dy: f64) -> Offset<f64> {
    Offset::new(dx, dy)
}
