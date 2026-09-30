//! Eager gesture recognizer
//!
//! Wins the gesture arena on the first `add_pointer` call, before any pointer
//! event arrives.
//!
//! Flutter parity: [`eager.dart:42-68`](https://github.com/flutter/flutter/blob/master/packages/flutter/lib/src/gestures/eager.dart)
//! `EagerGestureRecognizer.acceptGesture` is called from `_addPointer` (line
//! 42-68) — the recogniser declares victory during pointer-down dispatch
//! rather than waiting for the user to lift their finger.
//!
//! # When to use
//!
//! Use [`EagerGestureRecognizer`] for `AndroidView`-style or platform-view
//! hit regions that must unconditionally win the arena for a pointer:
//!
//! - `AndroidView` / `UiKitView` (HybridComposition) — the embedded platform
//!   view absorbs all input and no Flutter recogniser should compete.
//! - `TextField` / `EditableText` focus rings in pre-IME / focus-only
//!   implementations.
//! - Any opaque hit-test region that delegates input handling to a non-Flutter
//!   sink.
//!
//! For the common case of "win on release", prefer
//! [`TapGestureRecognizer`](super::TapGestureRecognizer) or
//! [`LongPressGestureRecognizer`](super::LongPressGestureRecognizer).
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::GestureRecognizer;
//! use flui_interaction::arena::GestureArena;
//! use flui_interaction::ids::PointerId;
//! use flui_foundation::geometry::Offset;
//! use flui_interaction::recognizers::EagerGestureRecognizer;
//!
//! let arena = GestureArena::new();
//! let recognizer = EagerGestureRecognizer::new(arena.clone());
//!
//! // The recogniser claims the arena immediately on `add_pointer` — no
//! // pointer event is required. The owner closes the arena after routing Down.
//! let pointer = PointerId::PRIMARY;
//! let position = Offset::new(50.0, 50.0);
//! recognizer.add_pointer(pointer, position, position);
//! assert!(arena.contains(pointer));
//! arena.close(pointer);
//! assert!(arena.contains(pointer));
//! arena.drain_deferred_resolutions();
//! assert!(arena.is_empty());
//! ```
//!
//! # Ownership
//!
//! Like the rest of the gesture graph, this recogniser is owner-local.
//! `Arc` supplies stable arena identity and cheap clones; it does not make
//! executable callbacks cross-thread.
//!
//! # Lifecycle
//!
//! Call [`GestureRecognizer::dispose`] to clean up — `dispose` rejects the
//! arena entry and clears the tracked primary pointer so a subsequent
//! `add_pointer` is a safe no-op.

use std::sync::Arc;

use flui_foundation::geometry::Offset;
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::{GestureArena, GestureArenaMember, GestureDisposition},
    ids::PointerId,
    routing::PointerDispatch,
    settings::GestureSettings,
};

/// Eager gesture recognizer — wins the arena on `add_pointer`.
///
/// See [module-level docs](self) for use cases, ownership, and Flutter parity
/// notes (`eager.dart:42-68`).
#[derive(Debug, Clone)]
pub struct EagerGestureRecognizer {
    /// Base state (arena, primary-pointer tracking, disposal).
    state: RecognizerBase,

    /// Per-device gesture settings. Stored for parity with the rest of the
    /// recogniser set; Eager does not currently consult these values (it
    /// has no timing or slop logic) but exposes them so future v2 hooks
    /// (e.g. eager-with-deadline) can read them without a breaking change.
    settings: Arc<Mutex<GestureSettings>>,
}

impl EagerGestureRecognizer {
    /// Create a new eager recognizer with default gesture settings.
    ///
    /// Use [`Self::with_settings`] to override slop / timeout values.
    pub fn new(arena: GestureArena) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            settings: Arc::new(Mutex::new(GestureSettings::default())),
        })
    }

    /// Create a new eager recognizer with custom gesture settings.
    pub fn with_settings(arena: GestureArena, settings: GestureSettings) -> Arc<Self> {
        Arc::new(Self {
            state: RecognizerBase::new(arena),
            settings: Arc::new(Mutex::new(settings)),
        })
    }

    /// Snapshot the current gesture settings.
    pub fn settings(&self) -> GestureSettings {
        self.settings.lock().clone()
    }

    /// Replace the gesture settings in place.
    pub fn set_settings(&self, settings: GestureSettings) {
        *self.settings.lock() = settings;
    }
}

impl GestureRecognizer for EagerGestureRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        pointer: PointerId,
        position: Offset<f64>,
        // Eager reports no position in any callback of its own, but the base
        // records the contact in both spaces all the same: `initial_position`
        // and `initial_global_position` are read through the
        // `PrimaryPointerGestureRecognizer` trait, and a half-recorded contact
        // there would be a trap for the next reader.
        global_position: Offset<f64>,
    ) {
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "eager.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if !self.state.assert_not_disposed("add_pointer") {
            return;
        }
        // `start_tracking` adds us to the arena under the pointer slot and
        // records the primary pointer / initial position on the base.
        // No nested lock hold — `start_tracking` returns before the
        // `accept` call below touches the arena again.
        self.state
            .start_tracking(pointer, position, global_position, self);
        // Eager accept: resolve the arena in our favor immediately. If
        // the arena is still open we register as the eager winner
        // (auto-resolves on close); if it is already closed we resolve
        // outright. Either way we win before any pointer event arrives.
        self.state.accept_tracked();
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let event = dispatch.local;
        // per-impl span (trait fn disallows `#[instrument]`).
        let _span = tracing::info_span!(
            "eager.handle_event",
            kind = %crate::observability::pointer_event_kind(event),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        // Use-after-dispose guard (lifecycle pattern).
        if !self.state.assert_not_disposed("handle_event") {
            return;
        }
        // Eager has no event-driven logic — the arena win happens entirely
        // in `add_pointer`. `handle_event` is a no-op aside from the
        // disposed-state check so a stale event stream does not panic.
        //
        // v2 may read `event.pointer_id()` + `self.state.primary_pointer()`
        // to emit per-event diagnostics; v1 is arena-side only.
        let _ = event;
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Reject arena entries + clear the tracked primary pointer
        // (Flutter parity with `recognizer.dart:485-493`: disposing a
        // recogniser clears its arena state for tracked pointers).
        self.state.reject();
    }

    fn primary_pointer(&self) -> Option<PointerId> {
        self.state.primary_pointer()
    }
}

impl crate::recognizers::OneSequenceGestureRecognizer for EagerGestureRecognizer {
    fn tracked_pointers(&self) -> Vec<PointerId> {
        self.state
            .primary_pointer()
            .map(|p| vec![p])
            .unwrap_or_default()
    }

    fn resolve_pointer(&self, _pointer: PointerId, disposition: GestureDisposition) {
        // Eager declares victory in `add_pointer` via `state.accept`. The
        // arena-side `resolve` call from `OneSequenceGestureRecognizer`
        // is therefore a no-op for the accepted branch (we already won
        // inline). For the rejected branch we clear our tracking so a
        // later `add_pointer` starts fresh — matches the Flutter
        // `EagerGestureRecognizer.rejectGesture` cleanup (eager.dart:64-67).
        if matches!(disposition, GestureDisposition::Rejected) {
            self.state.stop_tracking();
        }
    }

    fn stop_tracking_pointer(&self, _pointer: PointerId) {
        self.state.stop_tracking();
    }
}

impl crate::recognizers::PrimaryPointerGestureRecognizer for EagerGestureRecognizer {
    fn initial_position(&self) -> Option<Offset<f64>> {
        self.state.initial_position()
    }

    fn handle_primary_pointer(&self, dispatch: PointerDispatch<'_>) {
        // Eager funnels all primary-pointer events through the base
        // `handle_event` (no per-event logic). Delegate via the
        // supertrait method so the disposed-state guard runs.
        <Self as GestureRecognizer>::handle_event(self, dispatch);
    }
}

impl GestureArenaMember for EagerGestureRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {
        // v2 may mark an `accepted` flag here; for v1 the arena win is
        // declared in `add_pointer` via `state.accept`, so this hook is
        // a no-op (matches Flutter's empty `EagerGestureRecognizer.
        // acceptGesture` body at eager.dart:42-43).
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        // Mirror Flutter `EagerGestureRecognizer.rejectGesture`
        // (eager.dart:64-67): clear the tracked primary pointer and
        // initial position so the recogniser is ready for a fresh
        // sequence. We do NOT re-enter the arena here (no `state.reject`
        // call) — the dispatch path that called us is already holding
        // the arena's per-entry lock; another `arena.resolve` call would
        // re-deadlock under `parking_lot::Mutex`.
        self.state.set_primary_pointer(None);
        self.state.clear_initial_contact();
    }
}
