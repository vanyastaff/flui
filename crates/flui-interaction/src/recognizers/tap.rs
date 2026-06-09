//! Tap gesture recognizer
//!
//! Recognizes tap gestures (pointer down + up within slop tolerance).
//!
//! A tap is defined as:
//! - Pointer down
//! - Pointer stays within touch_slop of initial position
//! - Pointer up within timeout
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/TapGestureRecognizer-class.html>
//!
//! # Button support
//!
//! The recogniser is button-aware: callers can register
//! separate callbacks for [`TapButton::Primary`],
//! [`TapButton::Secondary`], and [`TapButton::Tertiary`] clicks. The
//! primary path keeps the legacy `on_tap*` callbacks and fires when
//! the down event's `button` mask includes `Primary`. The secondary
//! path fires the `on_secondary_tap*` callbacks on
//! [`PointerButton::Secondary`] events; the tertiary path fires on
//! [`PointerButton::Auxiliary`]
//! (Flutter maps "tertiary" to the middle / auxiliary mouse button).
//! If no button-specific callback is registered, the event is
//! silently dropped (the recogniser stays a no-op for that button).

use std::sync::Arc;

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;
use ui_events::pointer::PointerButton;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::GestureArenaMember,
    events::{PointerEvent, PointerType},
    ids::PointerId,
    settings::GestureSettings,
    traits::PointerEventExtTrait,
};

/// Tap button slot — matches Flutter's `kPrimaryButton` / `kSecondaryButton`
/// / `kTertiaryButton` separation.
///
/// Button mapping:
/// - [`TapButton::Primary`]   ↔ `ui_events::pointer::PointerButton::Primary`
/// - [`TapButton::Secondary`] ↔ `ui_events::pointer::PointerButton::Secondary`
/// - [`TapButton::Tertiary`]  ↔ `ui_events::pointer::PointerButton::Auxiliary`
///   (Flutter convention — middle mouse button).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TapButton {
    /// Primary (default left mouse button / touch contact).
    Primary,
    /// Secondary (right mouse button).
    Secondary,
    /// Tertiary (auxiliary / middle mouse button).
    Tertiary,
}

impl TryFrom<PointerButton> for TapButton {
    /// The unsupported button (outside the three Flutter-tracked slots).
    type Error = PointerButton;

    /// Map a raw [`PointerButton`] event payload to a [`TapButton`] slot.
    ///
    /// Errors for buttons outside the three Flutter-tracked slots
    /// (X1/X2/pen-eraser/etc.) — those events are ignored by the tap
    /// recogniser entirely.
    fn try_from(button: PointerButton) -> Result<Self, Self::Error> {
        match button {
            PointerButton::Primary => Ok(Self::Primary),
            PointerButton::Secondary => Ok(Self::Secondary),
            PointerButton::Auxiliary => Ok(Self::Tertiary),
            other => Err(other),
        }
    }
}

impl TapButton {
    /// Map a raw [`PointerButton`] to a [`TapButton`] slot, or `None` for the
    /// untracked buttons. Convenience wrapper over the [`TryFrom`] impl for the
    /// `Option`-combinator call sites.
    #[inline]
    pub fn from_pointer_button(button: PointerButton) -> Option<Self> {
        Self::try_from(button).ok()
    }
}

/// Callback for tap events
pub type TapCallback = Arc<dyn Fn(TapDetails) + Send + Sync>;

/// Details about a tap gesture
#[derive(Debug, Clone, PartialEq)]
pub struct TapDetails {
    /// Global position where tap occurred
    pub global_position: Offset<Pixels>,
    /// Local position (relative to widget)
    pub local_position: Offset<Pixels>,
    /// Pointer device kind
    pub kind: PointerType,
}

/// Recognizes tap gestures
///
/// A tap is a quick press-and-release within a small movement tolerance.
///
/// # Example
///
/// ```rust
/// use flui_interaction::arena::GestureArena;
/// use flui_interaction::recognizers::TapGestureRecognizer;
///
/// let arena = GestureArena::new();
/// // The recogniser is shared via `Arc`; clone the inner `Arc` to
/// // register callbacks (`on_tap` fires on Primary button up).
/// let recognizer = TapGestureRecognizer::new(arena)
///     .with_on_tap(|details| {
///         // The callback fires AFTER the arena confirms this
///         // recogniser won; the `pending_up` deferral guarantees
///         // only the arena winner receives the user callback.
///         let _pos = details.global_position;
///     });
/// // `add_pointer` and `handle_event` are wired by
/// // `flui_interaction::GestureBinding` at runtime.
#[derive(Clone)]
pub struct TapGestureRecognizer {
    /// Base state (arena, tracking, etc.)
    state: RecognizerBase,

    /// Callbacks
    callbacks: Arc<Mutex<TapCallbacks>>,

    /// Current gesture state
    gesture_state: Arc<Mutex<TapState>>,

    /// Gesture settings (device-specific tolerances)
    settings: Arc<Mutex<GestureSettings>>,

    /// Pending tap-down details captured at add_pointer, fired on
    /// arena accept (Flutter parity at tap.dart — `on_tap_down` callback
    /// only fires after `BaseTapGestureRecognizer._checkDown` resolves
    /// `_sentTapDown = true` post-arena). Cleared on accept or reject.
    pending_down: Arc<Mutex<Option<PendingDown>>>,

    /// Pending tap-up details captured at handle_event Up; fired by
    /// handle_tap_up *after* arena resolution confirms acceptance.
    /// Pre-fix code fired on_tap_up + on_tap
    /// during handle_tap_up unconditionally, but `handle_event::Up` is
    /// dispatched to every arena member; only the eventual arena winner
    /// should fire user callbacks.
    pending_up: Arc<Mutex<Option<PendingDown>>>,

    /// Arena-resolution outcome flag set by `accept_gesture` /
    /// `reject_gesture`. Read by `handle_tap_up` *after*
    /// `state.stop_tracking()` returns (which triggers arena.sweep).
    /// `Some(true)` = won, `Some(false)` = lost, `None` = pending.
    /// Reset to None on each new add_pointer cycle.
    accepted: Arc<Mutex<Option<bool>>>,
}

impl std::fmt::Debug for TapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapGestureRecognizer")
            .field("state", &self.state)
            .field("gesture_state", &*self.gesture_state.lock())
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct TapCallbacks {
    on_tap_down: Option<TapCallback>,
    on_tap_move: Option<TapCallback>,
    on_tap_up: Option<TapCallback>,
    on_tap: Option<TapCallback>,
    on_tap_cancel: Option<TapCallback>,

    // Secondary-button callbacks (right mouse).
    on_secondary_tap_down: Option<TapCallback>,
    on_secondary_tap_up: Option<TapCallback>,
    on_secondary_tap: Option<TapCallback>,
    on_secondary_tap_cancel: Option<TapCallback>,

    // Tertiary-button callbacks (auxiliary / middle mouse).
    on_tertiary_tap_down: Option<TapCallback>,
    on_tertiary_tap_up: Option<TapCallback>,
    on_tertiary_tap: Option<TapCallback>,
    on_tertiary_tap_cancel: Option<TapCallback>,
}

impl TapCallbacks {
    /// Per-button down-callback lookup.
    #[inline]
    fn down(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_down.as_ref(),
            TapButton::Secondary => self.on_secondary_tap_down.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap_down.as_ref(),
        }
    }

    /// Per-button up-callback lookup.
    #[inline]
    fn up(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_up.as_ref(),
            TapButton::Secondary => self.on_secondary_tap_up.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap_up.as_ref(),
        }
    }

    /// Per-button tap-callback lookup.
    #[inline]
    fn tap(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap.as_ref(),
            TapButton::Secondary => self.on_secondary_tap.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap.as_ref(),
        }
    }

    /// Per-button cancel-callback lookup.
    #[inline]
    fn cancel(&self, button: TapButton) -> Option<&TapCallback> {
        match button {
            TapButton::Primary => self.on_tap_cancel.as_ref(),
            TapButton::Secondary => self.on_secondary_tap_cancel.as_ref(),
            TapButton::Tertiary => self.on_tertiary_tap_cancel.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TapState {
    Ready,
    Down,
    Cancelled,
}

/// Per-pointer button tracking for the active tap sequence.
///
/// Stored alongside the recogniser's `gesture_state` so a switch from
/// primary to non-primary mid-sequence is observable. Reset on every
/// transition into `Ready` and on cancel.
#[derive(Debug, Clone, PartialEq)]
struct PendingDown {
    details: TapDetails,
    button: TapButton,
}

impl TapGestureRecognizer {
    /// Create a new tap recognizer with gesture arena
    pub fn new(arena: crate::arena::GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Arc::new(Mutex::new(TapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(TapState::Ready)),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
            pending_down: Arc::new(Mutex::new(None)),
            pending_up: Arc::new(Mutex::new(None)),
            accepted: Arc::new(Mutex::new(None)),
        })
    }

    /// Create a new tap recognizer with custom settings
    pub fn with_settings(
        arena: crate::arena::GestureArena,
        settings: GestureSettings,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            callbacks: Arc::new(Mutex::new(TapCallbacks::default())),
            gesture_state: Arc::new(Mutex::new(TapState::Ready)),
            settings: Arc::new(Mutex::new(settings)),
            pending_down: Arc::new(Mutex::new(None)),
            pending_up: Arc::new(Mutex::new(None)),
            accepted: Arc::new(Mutex::new(None)),
        })
    }

    /// Get the current gesture settings
    pub fn settings(&self) -> GestureSettings {
        self.settings.lock().clone()
    }

    /// Update gesture settings
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }

    /// Check if distance exceeds touch slop from settings
    fn exceeds_touch_slop(&self, distance: Pixels) -> bool {
        self.settings.lock().exceeds_touch_slop(distance)
    }

    /// Set the tap down callback
    pub fn with_on_tap_down(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_down = Some(Arc::new(callback));
        self
    }

    /// Set the tap move callback (called when pointer moves during tap)
    ///
    /// This callback is triggered when a pointer that initiated a tap moves
    /// but stays within the slop tolerance.
    pub fn with_on_tap_move(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_move = Some(Arc::new(callback));
        self
    }

    /// Set the tap up callback
    pub fn with_on_tap_up(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_up = Some(Arc::new(callback));
        self
    }

    /// Set the tap callback (called on successful tap)
    pub fn with_on_tap(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap = Some(Arc::new(callback));
        self
    }

    /// Set the tap cancel callback
    pub fn with_on_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tap_cancel = Some(Arc::new(callback));
        self
    }

    // ========================================================================
    // Secondary-button builders (right mouse).
    // ========================================================================

    /// Set the secondary-button tap-down callback.
    pub fn with_on_secondary_tap_down(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_secondary_tap_down = Some(Arc::new(callback));
        self
    }

    /// Set the secondary-button tap-up callback.
    pub fn with_on_secondary_tap_up(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_secondary_tap_up = Some(Arc::new(callback));
        self
    }

    /// Set the secondary-button tap callback (fires on successful up).
    pub fn with_on_secondary_tap(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_secondary_tap = Some(Arc::new(callback));
        self
    }

    /// Set the secondary-button tap-cancel callback.
    pub fn with_on_secondary_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_secondary_tap_cancel = Some(Arc::new(callback));
        self
    }

    // ========================================================================
    // Tertiary-button builders (auxiliary / middle mouse).
    // ========================================================================

    /// Set the tertiary-button tap-down callback.
    pub fn with_on_tertiary_tap_down(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tertiary_tap_down = Some(Arc::new(callback));
        self
    }

    /// Set the tertiary-button tap-up callback.
    pub fn with_on_tertiary_tap_up(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tertiary_tap_up = Some(Arc::new(callback));
        self
    }

    /// Set the tertiary-button tap callback (fires on successful up).
    pub fn with_on_tertiary_tap(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tertiary_tap = Some(Arc::new(callback));
        self
    }

    /// Set the tertiary-button tap-cancel callback.
    pub fn with_on_tertiary_tap_cancel(
        self: Arc<Self>,
        callback: impl Fn(TapDetails) + Send + Sync + 'static,
    ) -> Arc<Self> {
        self.callbacks.lock().on_tertiary_tap_cancel = Some(Arc::new(callback));
        self
    }

    /// Handle tap down event — records pending down details + transitions
    /// state. The per-button down callback is NOT fired here; per Flutter
    /// parity at `tap.dart::_BaseTapGestureRecognizer::_checkDown`, the
    /// callback fires only after arena accept (see [`Self::accept_gesture`]).
    ///
    /// `button` is locked at down-time; a primary-button down that
    /// later receives a secondary up is treated as cancel (button
    /// mismatch), matching Flutter's `_route` rejection path.
    fn handle_tap_down(&self, position: Offset<Pixels>, kind: PointerType, button: TapButton) {
        *self.gesture_state.lock() = TapState::Down;
        *self.pending_down.lock() = Some(PendingDown {
            details: TapDetails {
                global_position: position,
                local_position: position,
                kind,
            },
            button,
        });
    }

    /// Fire pending per-button `on_*_tap_down` callback, if any. Called
    /// from `accept_gesture` once arena resolves us as the winner. Matches
    /// Flutter `tap.dart::_checkDown`'s `_sentTapDown` guard — fires
    /// exactly once per gesture sequence.
    fn fire_pending_tap_down(&self) {
        let Some(pending) = self.pending_down.lock().take() else {
            return;
        };
        let callback = self.callbacks.lock().down(pending.button).cloned();
        if let Some(cb) = callback {
            cb(pending.details);
        }
    }

    /// Handle tap up event.
    ///
    /// Review-driven restructure: records pending_up, initiates
    /// arena resolution via `state.stop_tracking()`, then fires user
    /// callbacks ONLY if arena confirmed acceptance. Eliminates the prior
    /// assumption that pointer-up implies victory (some competing
    /// recognizers also receive Up events without winning).
    ///
    /// Button mismatch (down was Primary, up is Secondary) cancels
    /// the tap rather than firing the secondary slot — Flutter
    /// `tap.dart::_checkUp` routes the up to whichever button stream
    /// initiated the down.
    fn handle_tap_up(&self, position: Offset<Pixels>, kind: PointerType, button: TapButton) {
        let current_state = *self.gesture_state.lock();

        if current_state != TapState::Down {
            return;
        }

        // Button-mismatch → cancel the in-flight primary tap.
        let down_btn = self.pending_down.lock().as_ref().map(|p| p.button);
        if let Some(down_btn) = down_btn
            && down_btn != button
        {
            *self.gesture_state.lock() = TapState::Cancelled;
            // Notify the down-button cancel slot if any was wired.
            let cancel_cb = self.callbacks.lock().cancel(down_btn).cloned();
            if let Some(cb) = cancel_cb {
                let details = TapDetails {
                    global_position: position,
                    local_position: position,
                    kind,
                };
                cb(details);
            }
            self.state.stop_tracking();
            *self.pending_down.lock() = None;
            return;
        }

        *self.gesture_state.lock() = TapState::Ready;
        let details = TapDetails {
            global_position: position,
            local_position: position,
            kind,
        };
        // Record pending Up — fired only after arena confirms accept.
        *self.pending_up.lock() = Some(PendingDown { details, button });

        // Trigger arena resolution. This synchronously dispatches to
        // either `accept_gesture` or `reject_gesture` below, which sets
        // `self.accepted`.
        self.state.stop_tracking();

        // After arena resolution: fire callbacks if we won.
        if self.accepted.lock().unwrap_or(false) {
            // Fire per-button tap-down first (Flutter ordering), then up + tap.
            self.fire_pending_tap_down();
            let Some(pending_up) = self.pending_up.lock().take() else {
                return;
            };
            let (up_cb, tap_cb) = {
                let cbs = self.callbacks.lock();
                (
                    cbs.up(pending_up.button).cloned(),
                    cbs.tap(pending_up.button).cloned(),
                )
            };
            if let Some(cb) = up_cb {
                cb(pending_up.details.clone());
            }
            if let Some(cb) = tap_cb {
                cb(pending_up.details);
            }
        }
    }

    /// Handle tap cancel event.
    fn handle_tap_cancel(&self, position: Offset<Pixels>, kind: PointerType) {
        let current_state = *self.gesture_state.lock();

        if current_state == TapState::Down {
            *self.gesture_state.lock() = TapState::Cancelled;

            // Call per-button cancel callback for the button that initiated
            // the down.
            let button = self.pending_down.lock().as_ref().map(|p| p.button);
            if let Some(btn) = button {
                let cancel_cb = self.callbacks.lock().cancel(btn).cloned();
                if let Some(cb) = cancel_cb {
                    let details = TapDetails {
                        global_position: position,
                        local_position: position,
                        kind,
                    };
                    cb(details);
                }
            }

            // Reject in arena
            self.state.reject();
            *self.pending_down.lock() = None;
        }
    }

    /// Handle tap move event (pointer moved within slop tolerance)
    fn handle_tap_move(&self, position: Offset<Pixels>, kind: PointerType) {
        let current_state = *self.gesture_state.lock();

        if current_state == TapState::Down {
            // Call on_tap_move callback (primary-only — Flutter has no
            // secondary/tertiary move; a primary-button tap that moves is
            // still observed by `on_tap_move`).
            if let Some(callback) = self.callbacks.lock().on_tap_move.clone() {
                let details = TapDetails {
                    global_position: position,
                    local_position: position,
                    kind,
                };
                callback(details);
            }
        }
    }

    /// Check if pointer moved too far (beyond slop tolerance)
    fn check_slop(&self, current_position: Offset<Pixels>) -> bool {
        if let Some(initial_pos) = self.state.initial_position() {
            let delta = current_position - initial_pos;
            let distance = delta.distance();

            if self.exceeds_touch_slop(distance) {
                return true; // Moved too far
            }
        }
        false
    }

    /// Extract the [`TapButton`] slot for a `PointerEvent::Down` payload.
    fn down_button(event: &PointerEvent) -> TapButton {
        if let PointerEvent::Down(data) = event {
            data.button
                .and_then(TapButton::from_pointer_button)
                .unwrap_or(TapButton::Primary)
        } else {
            TapButton::Primary
        }
    }

    /// Extract the [`TapButton`] slot for a `PointerEvent::Up` payload.
    fn up_button(event: &PointerEvent) -> TapButton {
        if let PointerEvent::Up(data) = event {
            data.button
                .and_then(TapButton::from_pointer_button)
                .unwrap_or(TapButton::Primary)
        } else {
            TapButton::Primary
        }
    }
}

impl GestureRecognizer for TapGestureRecognizer {
    fn add_pointer(&self, pointer: PointerId, position: Offset<Pixels>) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // Reset accepted flag + pending_up for the new sequence (flags
        // from a prior gesture must not bleed into the new one).
        *self.accepted.lock() = None;
        *self.pending_up.lock() = None;
        // Start tracking this pointer
        // Create Arc from self for arena tracking
        let recognizer = Arc::new(self.clone());
        self.state.start_tracking(pointer, position, &recognizer);

        // Stage the down so the documented `add_pointer` = "pointer is down"
        // contract holds: a subsequent up fires the tap even when no separate
        // `Down` event is routed afterwards. The button/kind are provisional —
        // the API surface carries only a position — and a real `Down` in
        // `handle_event` refines them before any up. `handle_tap_down` only
        // records pending state (it fires no callback), so this cannot
        // accidentally win the arena or double-fire `on_tap_down`.
        self.handle_tap_down(position, PointerType::Touch, TapButton::Primary);
    }

    fn handle_event(&self, event: &PointerEvent) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "tap.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Only process if we're tracking a pointer
        let Some(primary) = self.state.primary_pointer() else {
            return;
        };
        // Filter to the primary pointer we are tracking (ignore
        // secondary-pointer events in single-pointer recognisers).
        if event.pointer_id() != primary {
            return;
        }

        match event {
            PointerEvent::Down(data) => {
                let pos = data.state.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                let button = Self::down_button(event);
                self.handle_tap_down(position, data.pointer.pointer_type, button);
            }
            PointerEvent::Move(data) => {
                let pos = data.current.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                let pointer_type = data.pointer.pointer_type;
                // Check if moved too far (slop detection)
                if self.check_slop(position) {
                    self.handle_tap_cancel(position, pointer_type);
                } else {
                    // Still within slop - call tap move callback
                    self.handle_tap_move(position, pointer_type);
                }
            }
            PointerEvent::Up(data) => {
                let pos = data.state.position;
                let position = Offset::new(Pixels(pos.x as f32), Pixels(pos.y as f32));
                let button = Self::up_button(event);
                self.handle_tap_up(position, data.pointer.pointer_type, button);
            }
            PointerEvent::Cancel(info) => {
                // Cancel doesn't have position, use initial position
                if let Some(pos) = self.state.initial_position() {
                    self.handle_tap_cancel(pos, info.pointer_type);
                }
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Reject arena entries + clear tracked pointer (Flutter parity:
        // gestures/recognizer.dart:485-493 disposing GestureRecognizer
        // clears arena state for tracked pointers).
        self.state.reject();
        let mut callbacks = self.callbacks.lock();
        callbacks.on_tap_down = None;
        callbacks.on_tap_move = None;
        callbacks.on_tap_up = None;
        callbacks.on_tap = None;
        callbacks.on_tap_cancel = None;
        // Secondary / tertiary slots.
        callbacks.on_secondary_tap_down = None;
        callbacks.on_secondary_tap_up = None;
        callbacks.on_secondary_tap = None;
        callbacks.on_secondary_tap_cancel = None;
        callbacks.on_tertiary_tap_down = None;
        callbacks.on_tertiary_tap_up = None;
        callbacks.on_tertiary_tap = None;
        callbacks.on_tertiary_tap_cancel = None;
        *self.pending_down.lock() = None;
        *self.pending_up.lock() = None;
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

// =============================================================================
// Canonical trait hierarchy adoption
// =============================================================================
//
// Flutter parity: `tap.dart:202 BaseTapGestureRecognizer extends
// PrimaryPointerGestureRecognizer`. The trait infrastructure
// at one_sequence.rs + primary_pointer.rs is now implemented for Tap.

impl crate::recognizers::OneSequenceGestureRecognizer for TapGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    fn resolve_pointer(&self, _pointer: PointerId, disposition: crate::arena::GestureDisposition) {
        match disposition {
            crate::arena::GestureDisposition::Accepted => {
                // Arena accepted us — same path as accept_gesture below.
                *self.accepted.lock() = Some(true);
            }
            crate::arena::GestureDisposition::Rejected => {
                self.state.reject();
            }
        }
    }

    fn stop_tracking_pointer(&self, _pointer: PointerId) {
        self.state.stop_tracking();
    }
}

impl crate::recognizers::PrimaryPointerGestureRecognizer for TapGestureRecognizer {
    fn initial_position(&self) -> Option<Offset<Pixels>> {
        self.state.initial_position()
    }

    fn handle_primary_pointer(&self, event: &PointerEvent) {
        // Tap dispatches all primary-pointer events through handle_event;
        // delegate via the supertrait method.
        <Self as GestureRecognizer>::handle_event(self, event);
    }
}

impl GestureArenaMember for TapGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // Do NOT invoke user callbacks here. The
        // arena holds its internal lock while dispatching accept_gesture
        // → calling user code from this site is a lock-during-callback
        // hazard (user callback may re-enter arena, panic, or block).
        // Instead just record acceptance — the handle_tap_up path (or
        // dispose path) reads this flag after arena resolve returns.
        *self.accepted.lock() = Some(true);
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // Same lock-during-callback concern as accept_gesture. Record
        // rejection; let the gesture-up / dispose path fire on_tap_cancel
        // outside the arena lock.
        *self.accepted.lock() = Some(false);
        *self.pending_down.lock() = None;
        *self.pending_up.lock() = None;
        // Do NOT call handle_tap_cancel here — it calls self.state.reject(),
        // which re-enters the arena while the arena is still dispatching
        // reject_gesture, causing a deadlock (parking_lot::Mutex is not
        // reentrant). The cancel callback was already fired in
        // handle_tap_up on the button-mismatch path; the slop-exceeded
        // path fires cancel before reject(), so no duplicate is needed.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;
    use ui_events::pointer::PointerButton;

    fn pos(x: f32, y: f32) -> Offset<Pixels> {
        Offset::new(Pixels(x), Pixels(y))
    }

    fn primary_down(p: Offset<Pixels>) -> PointerEvent {
        crate::events::make_down_event_with_button(p, PointerType::Touch, PointerButton::Primary)
    }
    fn secondary_down(p: Offset<Pixels>) -> PointerEvent {
        crate::events::make_down_event_with_button(p, PointerType::Touch, PointerButton::Secondary)
    }
    fn tertiary_down(p: Offset<Pixels>) -> PointerEvent {
        crate::events::make_down_event_with_button(p, PointerType::Touch, PointerButton::Auxiliary)
    }
    fn primary_up(p: Offset<Pixels>) -> PointerEvent {
        crate::events::make_up_event_with_button(p, PointerType::Touch, PointerButton::Primary)
    }
    fn secondary_up(p: Offset<Pixels>) -> PointerEvent {
        crate::events::make_up_event_with_button(p, PointerType::Touch, PointerButton::Secondary)
    }
    fn tertiary_up(p: Offset<Pixels>) -> PointerEvent {
        crate::events::make_up_event_with_button(p, PointerType::Touch, PointerButton::Auxiliary)
    }

    #[test]
    fn test_tap_recognizer_creation() {
        let arena = GestureArena::new();
        let recognizer = TapGestureRecognizer::new(arena);

        assert_eq!(recognizer.primary_pointer(), None);
    }

    /// Sanity: `TapButton::from_pointer_button` correctly routes
    /// Primary / Secondary / Auxiliary to the three slots and returns
    /// `None` for X1 / X2.
    #[test]
    fn tap_button_from_pointer_button_routes_three_slots() {
        assert_eq!(
            TapButton::from_pointer_button(PointerButton::Primary),
            Some(TapButton::Primary)
        );
        assert_eq!(
            TapButton::from_pointer_button(PointerButton::Secondary),
            Some(TapButton::Secondary)
        );
        assert_eq!(
            TapButton::from_pointer_button(PointerButton::Auxiliary),
            Some(TapButton::Tertiary)
        );
        assert_eq!(TapButton::from_pointer_button(PointerButton::X1), None);
    }

    /// Legacy primary path: down + up with the Primary button
    /// fires `on_tap` (`add_pointer` no longer
    /// pre-stages the down; the down event itself does).
    #[test]
    fn test_tap_recognizer_with_callback() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena).with_on_tap(move |_details| {
            *tapped_clone.lock() = true;
        });

        let pointer = PointerId::PRIMARY;
        let position = pos(100.0, 100.0);

        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&primary_down(position));
        recognizer.handle_event(&primary_up(position));

        assert!(*tapped.lock());
    }

    #[test]
    fn add_pointer_then_up_without_down_fires_tap() {
        // `add_pointer` is the documented pointer-down entry point. A
        // subsequent up with no separately-routed `Down` event must still fire
        // the tap, because `add_pointer` pre-stages the provisional down.
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena).with_on_tap(move |_details| {
            *tapped_clone.lock() = true;
        });

        let pointer = PointerId::PRIMARY;
        let position = pos(4.0, 4.0);

        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&primary_up(position));

        assert!(
            *tapped.lock(),
            "tap should fire from add_pointer + up with no separate down event"
        );
    }

    #[test]
    fn test_tap_recognizer_slop_detection() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let cancelled = Arc::new(Mutex::new(false));

        let tapped_clone = tapped.clone();
        let cancelled_clone = cancelled.clone();

        let recognizer = TapGestureRecognizer::new(arena)
            .with_on_tap(move |_details| {
                *tapped_clone.lock() = true;
            })
            .with_on_tap_cancel(move |_details| {
                *cancelled_clone.lock() = true;
            });

        let pointer = PointerId::PRIMARY;
        let start_pos = pos(100.0, 100.0);

        recognizer.add_pointer(pointer, start_pos);
        recognizer.handle_event(&primary_down(start_pos));

        // 30px away — beyond TAP_SLOP = 18px.
        let moved_pos = pos(100.0, 130.0);
        recognizer.handle_event(&crate::events::make_move_event(
            moved_pos,
            PointerType::Touch,
        ));

        assert!(*cancelled.lock());
        assert!(!*tapped.lock());
    }

    #[test]
    fn test_tap_within_slop() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let tapped_clone = tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena).with_on_tap(move |_details| {
            *tapped_clone.lock() = true;
        });

        let pointer = PointerId::PRIMARY;
        let start_pos = pos(100.0, 100.0);

        recognizer.add_pointer(pointer, start_pos);
        recognizer.handle_event(&primary_down(start_pos));

        // ~7px — within slop.
        let moved_pos = pos(105.0, 105.0);
        recognizer.handle_event(&crate::events::make_move_event(
            moved_pos,
            PointerType::Touch,
        ));

        recognizer.handle_event(&primary_up(moved_pos));

        assert!(*tapped.lock());
    }

    // ========================================================================
    // Secondary / tertiary button routing.
    // ========================================================================

    /// Right-click down + up fires `on_secondary_tap`; primary slot
    /// stays silent.
    #[test]
    fn secondary_button_routes_to_secondary_callbacks() {
        let arena = GestureArena::new();
        let secondary_tapped = Arc::new(Mutex::new(false));
        let primary_tapped = Arc::new(Mutex::new(false));
        let s_clone = secondary_tapped.clone();
        let p_clone = primary_tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena)
            .with_on_tap(move |_| *p_clone.lock() = true)
            .with_on_secondary_tap(move |_| *s_clone.lock() = true);

        let pointer = PointerId::PRIMARY;
        let position = pos(50.0, 50.0);

        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&secondary_down(position));
        recognizer.handle_event(&secondary_up(position));

        assert!(*secondary_tapped.lock());
        assert!(!*primary_tapped.lock());
    }

    /// Middle-click down + up fires `on_tertiary_tap`; primary slot
    /// stays silent.
    #[test]
    fn tertiary_button_routes_to_tertiary_callbacks() {
        let arena = GestureArena::new();
        let tertiary_tapped = Arc::new(Mutex::new(false));
        let primary_tapped = Arc::new(Mutex::new(false));
        let t_clone = tertiary_tapped.clone();
        let p_clone = primary_tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena)
            .with_on_tap(move |_| *p_clone.lock() = true)
            .with_on_tertiary_tap(move |_| *t_clone.lock() = true);

        let pointer = PointerId::PRIMARY;
        let position = pos(60.0, 60.0);

        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&tertiary_down(position));
        recognizer.handle_event(&tertiary_up(position));

        assert!(*tertiary_tapped.lock());
        assert!(!*primary_tapped.lock());
    }

    /// Down with Primary then Up with Secondary must cancel the
    /// primary tap (button mismatch) — mirrors Flutter
    /// `tap.dart::_checkUp` rejection. Primary `on_tap_cancel`
    /// fires; neither `on_tap` nor `on_secondary_tap` does.
    #[test]
    fn button_mismatch_cancels_primary_tap() {
        let arena = GestureArena::new();
        let tapped = Arc::new(Mutex::new(false));
        let cancelled = Arc::new(Mutex::new(false));
        let secondary_tapped = Arc::new(Mutex::new(false));
        let t_clone = tapped.clone();
        let c_clone = cancelled.clone();
        let s_clone = secondary_tapped.clone();

        let recognizer = TapGestureRecognizer::new(arena)
            .with_on_tap(move |_| *t_clone.lock() = true)
            .with_on_tap_cancel(move |_| *c_clone.lock() = true)
            .with_on_secondary_tap(move |_| *s_clone.lock() = true);

        let pointer = PointerId::PRIMARY;
        let position = pos(70.0, 70.0);

        recognizer.add_pointer(pointer, position);
        recognizer.handle_event(&primary_down(position));
        // Up carries a different button — cancels the primary tap.
        recognizer.handle_event(&secondary_up(position));

        assert!(*cancelled.lock());
        assert!(!*tapped.lock());
        assert!(!*secondary_tapped.lock());
    }

    /// Slop-exceeded cancel on a secondary-button tap must fire
    /// `on_secondary_tap_cancel` (not the primary cancel slot).
    #[test]
    fn secondary_slop_cancel_routes_to_secondary_cancel() {
        let arena = GestureArena::new();
        let primary_cancelled = Arc::new(Mutex::new(false));
        let secondary_cancelled = Arc::new(Mutex::new(false));
        let p_clone = primary_cancelled.clone();
        let s_clone = secondary_cancelled.clone();

        let recognizer = TapGestureRecognizer::new(arena)
            .with_on_tap_cancel(move |_| *p_clone.lock() = true)
            .with_on_secondary_tap_cancel(move |_| *s_clone.lock() = true);

        let pointer = PointerId::PRIMARY;
        let start_pos = pos(80.0, 80.0);

        recognizer.add_pointer(pointer, start_pos);
        recognizer.handle_event(&secondary_down(start_pos));

        // 30px move — past TAP_SLOP. Slop detection routes to the
        // secondary cancel slot because the down button is Secondary.
        let moved_pos = pos(80.0, 110.0);
        recognizer.handle_event(&crate::events::make_move_event(
            moved_pos,
            PointerType::Touch,
        ));

        assert!(*secondary_cancelled.lock());
        assert!(!*primary_cancelled.lock());
    }
}
