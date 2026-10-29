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
use flui_foundation::geometry::Point;

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

    /// Provisional outcome for a reentrant event that needs a synchronous
    /// answer — a key, whose platform default (a system shortcut, AppKit's
    /// key-equivalent fallback) cannot wait for the running callback. It is
    /// answered "not handled", so the default runs now, and delivered once
    /// after the running callback returns (ADR-0207 §7).
    pub const DEFERRED_UNHANDLED: Self = Self {
        propagate: true,
        default_prevented: false,
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

/// The pointer events a backend sampled at one instant, in the order it
/// sampled them: the contacts a touch screen reported together, or the motion
/// of one display frame.
///
/// A recognizer that measures the relation between contacts (a pinch) is
/// settled once, after the last event, so it never sees one finger moved and
/// the other not. A backend reports a lone event as a frame of one
/// ([`PointerFrame::single`]) or as [`PlatformInput::Pointer`]; the two are
/// the same input.
#[derive(Debug, Clone)]
pub struct PointerFrame(Vec<PointerEvent>);

/// A [`PointerFrame`] needs at least one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a pointer frame holds at least one event")]
pub struct EmptyPointerFrame;

impl PointerFrame {
    /// The frame of `events`, which must not be empty.
    ///
    /// # Errors
    ///
    /// [`EmptyPointerFrame`] when `events` is empty.
    pub fn new(events: Vec<PointerEvent>) -> Result<Self, EmptyPointerFrame> {
        if events.is_empty() {
            return Err(EmptyPointerFrame);
        }
        Ok(Self(events))
    }

    /// A frame of one event.
    #[must_use]
    pub fn single(event: PointerEvent) -> Self {
        Self(vec![event])
    }

    /// The events, in the order they were sampled.
    #[must_use]
    pub fn events(&self) -> &[PointerEvent] {
        &self.0
    }

    /// The events, in the order they were sampled.
    #[must_use]
    pub fn into_events(self) -> Vec<PointerEvent> {
        self.0
    }

    /// How many events the frame holds; at least one.
    #[must_use]
    #[expect(clippy::len_without_is_empty, reason = "a frame is never empty")]
    pub fn len(&self) -> usize {
        self.0.len()
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
#[allow(
    clippy::exhaustive_enums,
    reason = "every consumer routes every kind of input, so a new kind is a change each of them makes"
)]
pub enum PlatformInput {
    /// Pointer, scrolling, trackpad or device-lifecycle event.
    Pointer(PointerEvent),

    /// Pointer events sampled at one instant; see [`PointerFrame`].
    PointerFrame(PointerFrame),

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

    /// The surface's input capabilities ([`InputCapabilities`](crate::input_capabilities::InputCapabilities),
    /// ADR-0213). The first is delivered before any other input once the input callback is
    /// installed; each later one is a change (a device attached or removed, a session change),
    /// in order with the input around it.
    Capabilities(crate::input_capabilities::InputCapabilities),
}

impl PlatformInput {
    /// Extract pointer event if this is a pointer input
    #[inline]
    pub fn as_pointer(&self) -> Option<&PointerEvent> {
        match self {
            PlatformInput::Pointer(event) => Some(event),
            PlatformInput::Keyboard(_)
            | PlatformInput::PointerFrame(_)
            | PlatformInput::Ime(_)
            | PlatformInput::DragDrop(_)
            | PlatformInput::Capabilities(_) => None,
        }
    }

    /// Extract keyboard event if this is a keyboard input
    #[inline]
    pub fn as_keyboard(&self) -> Option<&KeyEvent> {
        match self {
            PlatformInput::Keyboard(event) => Some(event),
            PlatformInput::Pointer(_)
            | PlatformInput::PointerFrame(_)
            | PlatformInput::Ime(_)
            | PlatformInput::DragDrop(_)
            | PlatformInput::Capabilities(_) => None,
        }
    }

    /// Extract the IME event if this is an IME composition/commit input.
    #[inline]
    pub fn as_ime(&self) -> Option<&crate::ImeEvent> {
        match self {
            PlatformInput::Ime(event) => Some(event),
            PlatformInput::Pointer(_)
            | PlatformInput::PointerFrame(_)
            | PlatformInput::Keyboard(_)
            | PlatformInput::DragDrop(_)
            | PlatformInput::Capabilities(_) => None,
        }
    }

    /// Extract the drag-and-drop event if this is a DnD input.
    #[inline]
    pub fn as_drag_drop(&self) -> Option<&DragDropEvent> {
        match self {
            PlatformInput::DragDrop(event) => Some(event),
            PlatformInput::Pointer(_)
            | PlatformInput::PointerFrame(_)
            | PlatformInput::Keyboard(_)
            | PlatformInput::Ime(_)
            | PlatformInput::Capabilities(_) => None,
        }
    }
}

/// The push half of the data-transfer transport (ADR-0038): stage-1 arrival
/// and stage-2/6 progress for a drag session over one window. The target's
/// reply half flows the other way, through
/// [`crate::data_transfer::DataTransferSource::update_drop_feedback`].
#[derive(Debug, Clone)]
#[allow(
    clippy::exhaustive_enums,
    reason = "the stages of the transport protocol (ADR-0038), each of which a target answers"
)]
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
