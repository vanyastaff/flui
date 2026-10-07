//! Eager gesture recognizer
//!
//! Wins the gesture arena on the first `add_pointer` call, before any pointer
//! event arrives.
//!
//! The recogniser declares victory during pointer-down dispatch
//! rather than waiting for the user to lift their finger.
//!
//! # When to use
//!
//! Use [`EagerGestureRecognizer`] for `AndroidView`-style or platform-view
//! hit regions that must unconditionally win the arena for a pointer:
//!
//! - `AndroidView` / `UiKitView` (HybridComposition) — the embedded platform
//!   view absorbs all input and no other recogniser should compete.
//! - `TextField` / `EditableText` focus rings in pre-IME / focus-only
//!   implementations.
//! - Any opaque hit-test region that delegates input handling to a sink
//!   outside the gesture arena.
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
    arena::{GestureArena, GestureArenaMember, GestureDisposition, SweepModel},
    events::PointerEvent,
    ids::PointerId,
    routing::PointerDispatch,
    settings::GestureSettings,
    traits::PointerEventExtTrait,
};

/// Eager gesture recognizer — wins the arena on `add_pointer`.
///
/// See [module-level docs](self) for use cases and ownership.
#[derive(Debug, Clone)]
pub struct EagerGestureRecognizer {
    /// Base state (arena, primary-pointer tracking, disposal).
    state: RecognizerBase,

    /// Per-device gesture settings. Stored for parity with the rest of the
    /// recogniser set; Eager has no timing or slop logic and does not read
    /// them.
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
        // Eager accept: while the arena is open this registers the eager
        // winner, which resolves on close when there are competitors; a lone
        // member is resolved by the arena's deferred default at the end of
        // the input. On a closed arena it resolves outright.
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
        // The arena win happens entirely in `add_pointer`. The only event
        // work is ending the tracked sequence when its pointer lifts or is
        // cancelled, so `primary_pointer` never reports a finished contact.
        if Some(event.pointer_id()) != self.state.primary_pointer() {
            return;
        }
        match event {
            PointerEvent::Up(_) => self.state.stop_tracking(),
            PointerEvent::Cancel(_) => {
                // A cancelled contact may leave its arena open and now empty; a
                // self-driven arena has no binding to sweep it, so the entry
                // sweeps it here, as the Up path does through `stop_tracking`.
                let entry = self.state.tracked_entry();
                self.state.reject();
                if self.state.arena().sweep_model() == SweepModel::SelfDriven
                    && let Some(entry) = entry
                {
                    entry.sweep();
                }
            }
            _ => {}
        }
    }

    fn dispose(&self) {
        self.state.mark_disposed();
        // Reject arena entries + clear the tracked primary pointer
        // (a disposed recogniser must not linger in the arena for tracked
        // pointers).
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
        // later `add_pointer` starts fresh.
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
        // The win was claimed in `add_pointer`; Eager has no callback to run.
    }

    fn reject_gesture(&self, pointer: PointerId) {
        // Another eager member claimed first. Forget the contact so the
        // recogniser is ready for a fresh sequence; a rejection for an older
        // contact leaves the current one alone.
        if self.state.primary_pointer() == Some(pointer) {
            self.state.set_primary_pointer(None);
            self.state.clear_initial_contact();
        }
    }
}
