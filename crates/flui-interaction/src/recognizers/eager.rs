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
//! use flui_types::geometry::{Offset, Pixels};
//! use flui_interaction::recognizers::EagerGestureRecognizer;
//!
//! let arena = GestureArena::new();
//! let recognizer = EagerGestureRecognizer::new(arena.clone());
//!
//! // The recogniser claims the arena immediately on `add_pointer` — no
//! // pointer event is required. The owner closes the arena after routing Down.
//! let pointer = PointerId::PRIMARY;
//! let position = Offset::new(Pixels(50.0), Pixels(50.0));
//! recognizer.add_pointer(pointer, position);
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

use flui_types::{Offset, geometry::Pixels};
use parking_lot::Mutex;

use super::recognizer::{GestureRecognizer, RecognizerBase};
use crate::{
    arena::{GestureArena, GestureArenaMember, GestureDisposition},
    events::PointerEvent,
    ids::PointerId,
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
    fn add_pointer(self: &Arc<Self>, pointer: PointerId, position: Offset<Pixels>) {
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
        self.state.start_tracking(pointer, position, self);
        // Eager accept: resolve the arena in our favor immediately. If
        // the arena is still open we register as the eager winner
        // (auto-resolves on close); if it is already closed we resolve
        // outright. Either way we win before any pointer event arrives.
        self.state.accept_tracked();
    }

    fn handle_event(&self, event: &PointerEvent) {
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
    fn initial_position(&self) -> Option<Offset<Pixels>> {
        self.state.initial_position()
    }

    fn handle_primary_pointer(&self, event: &PointerEvent) {
        // Eager funnels all primary-pointer events through the base
        // `handle_event` (no per-event logic). Delegate via the
        // supertrait method so the disposed-state guard runs.
        <Self as GestureRecognizer>::handle_event(self, event);
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
        self.state.set_initial_position(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::GestureArena;
    use crate::events::PointerType;
    use std::sync::Arc;

    fn pos(x: f32, y: f32) -> Offset<Pixels> {
        Offset::new(Pixels(x), Pixels(y))
    }

    /// Minimal arena-member stand-in for arena-conflict tests. The real
    /// `arena::tests::MockMember` is `#[cfg(test)]` inside `arena/mod.rs`
    /// and not re-exported; duplicating the four-line stub here keeps
    /// `eager` self-contained.
    struct MockMember {
        accepted: Arc<Mutex<bool>>,
        rejected: Arc<Mutex<bool>>,
    }

    // The arena's `GestureArenaMember` supertrait requires the sealed
    // marker; the arena's own test stub wires this explicitly.
    impl crate::sealed::arena_member::Sealed for MockMember {}

    impl MockMember {
        fn new() -> Self {
            Self {
                accepted: Arc::new(Mutex::new(false)),
                rejected: Arc::new(Mutex::new(false)),
            }
        }
        fn was_accepted(&self) -> bool {
            *self.accepted.lock()
        }
        fn was_rejected(&self) -> bool {
            *self.rejected.lock()
        }
    }

    impl GestureArenaMember for MockMember {
        fn accept_gesture(&self, _pointer: PointerId) {
            *self.accepted.lock() = true;
        }
        fn reject_gesture(&self, _pointer: PointerId) {
            *self.rejected.lock() = true;
        }
    }

    /// `new(arena)` produces a live recogniser with no primary pointer.
    #[test]
    fn eager_construction() {
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena);

        assert!(!recognizer.state.is_disposed());
        assert_eq!(recognizer.primary_pointer(), None);
    }

    /// `add_pointer` registers us as the eager winner without settling the
    /// open arena. Closing the arena then resolves the claim synchronously.
    #[test]
    fn eager_add_pointer_claims_then_wins_when_the_arena_closes() {
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena.clone());

        let pointer = PointerId::PRIMARY;
        let position = pos(50.0, 50.0);

        recognizer.add_pointer(pointer, position);

        assert!(arena.contains(pointer));
        assert!(arena.is_open(pointer));

        arena.close(pointer);

        assert!(arena.contains(pointer));
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert!(arena.is_empty());
    }

    /// When Eager joins a contest that already has a competing member,
    /// Eager's eager-winner slot still wins when the arena closes —
    /// matching Flutter `EagerGestureRecognizer.acceptGesture` priority
    /// (eager.dart:42-68, the recogniser outranks all other members).
    #[test]
    fn eager_wins_against_competing_recognizer_added_first() {
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena.clone());

        let pointer = PointerId::PRIMARY;
        let position = pos(75.0, 75.0);

        // Competing member joins first.
        let competitor = Arc::new(MockMember::new());
        arena.add(pointer, competitor.clone());

        // Eager joins + immediately claims eager-winner.
        recognizer.add_pointer(pointer, position);

        // Close the arena — the eager winner takes the contest.
        arena.close(pointer);

        assert!(arena.is_empty());
        assert!(competitor.was_rejected());
        assert!(!competitor.was_accepted());
    }

    /// `primary_pointer()` is populated from the `add_pointer` payload.
    #[test]
    fn eager_primary_pointer_set_after_add() {
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena);

        recognizer.add_pointer(PointerId::PRIMARY, pos(10.0, 20.0));

        assert_eq!(recognizer.primary_pointer(), Some(PointerId::PRIMARY));
    }

    /// `PrimaryPointerGestureRecognizer::initial_position()` reflects the
    /// `add_pointer` position — `RecognizerBase::start_tracking` already
    /// records this, so Eager inherits it for free via the
    /// `PrimaryPointerGestureRecognizer` shim.
    #[test]
    fn eager_initial_position_set_after_add() {
        use crate::recognizers::PrimaryPointerGestureRecognizer;

        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena);

        let position = pos(33.0, 44.0);
        recognizer.add_pointer(PointerId::PRIMARY, position);

        assert_eq!(recognizer.initial_position(), Some(position));
    }

    /// `handle_event` is a no-op for Down / Move / Up — the arena win
    /// has already happened, and Eager has no event-driven side effects.
    #[test]
    fn eager_handle_event_is_noop() {
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena);

        let pointer = PointerId::PRIMARY;
        let position = pos(5.0, 5.0);
        recognizer.add_pointer(pointer, position);

        // Down / Move / Up should all run without panic or state change.
        recognizer.handle_event(&crate::events::make_down_event(
            position,
            PointerType::Touch,
        ));
        recognizer.handle_event(&crate::events::make_move_event(
            position,
            PointerType::Touch,
        ));
        recognizer.handle_event(&crate::events::make_up_event(position, PointerType::Touch));

        // Primary pointer / initial position are still set (Eager does
        // not auto-stop on Up — the caller decides when to dispose).
        assert_eq!(recognizer.primary_pointer(), Some(pointer));
    }

    /// `dispose` flips the disposed flag, rejects arena entries, and
    /// clears the tracked primary pointer.
    #[test]
    fn eager_dispose_clears_state() {
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena.clone());

        recognizer.add_pointer(PointerId::PRIMARY, pos(0.0, 0.0));
        assert_eq!(recognizer.primary_pointer(), Some(PointerId::PRIMARY));

        recognizer.dispose();

        assert!(recognizer.state.is_disposed());
        assert_eq!(recognizer.primary_pointer(), None);

        // Subsequent `add_pointer` is a no-op (use-after-dispose guard).
        // The shared `RecognizerBase::assert_not_disposed` helper
        // `debug_assert!`s in debug builds (by design — catches misuse
        // at test time) and soft-warns in release. The spec's "no panic"
        // guarantee holds in release; we gate the live call on that
        // condition so this test runs cleanly in both profiles.
        if !cfg!(debug_assertions) {
            recognizer.add_pointer(PointerId::PRIMARY, pos(1.0, 1.0));
            assert_eq!(recognizer.primary_pointer(), None);
            assert!(!arena.contains(PointerId::PRIMARY));
        }
    }

    /// `with_settings` stores the caller-provided settings verbatim.
    /// The public `pan_slop()` getter lets us inspect the stored value
    /// without reaching into the private mutex.
    #[test]
    fn eager_with_settings_uses_provided_settings() {
        let arena = GestureArena::new();
        let custom = GestureSettings::default().with_pan_slop(42.0);
        let recognizer = EagerGestureRecognizer::with_settings(arena, custom);

        assert_eq!(recognizer.settings().pan_slop(), 42.0);
    }

    /// `reject_gesture` clears the tracked primary pointer / initial
    /// position (Flutter parity at eager.dart:64-67).
    #[test]
    fn eager_reject_gesture_clears_state() {
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena);

        recognizer.add_pointer(PointerId::PRIMARY, pos(10.0, 10.0));
        assert_eq!(recognizer.primary_pointer(), Some(PointerId::PRIMARY));

        <EagerGestureRecognizer as GestureArenaMember>::reject_gesture(
            &recognizer,
            PointerId::PRIMARY,
        );

        assert_eq!(recognizer.primary_pointer(), None);
        assert_eq!(
            <EagerGestureRecognizer as crate::recognizers::PrimaryPointerGestureRecognizer>::initial_position(&recognizer),
            None
        );
    }

    /// `handle_event` after `dispose` is a no-op (no panic, state stays
    /// cleared). Mirrors the tap `dispose` test pattern. Gated on
    /// release-mode — see `eager_dispose_clears_state` for the
    /// debug-assert rationale (shared `RecognizerBase::assert_not_disposed`
    /// helper panics in debug, soft-warns in release).
    #[test]
    fn eager_handle_event_after_dispose_is_safe() {
        if cfg!(debug_assertions) {
            return;
        }
        let arena = GestureArena::new();
        let recognizer = EagerGestureRecognizer::new(arena);

        recognizer.dispose();
        let position = pos(0.0, 0.0);

        // Should not panic, should not resurrect cleared state.
        recognizer.handle_event(&crate::events::make_down_event(
            position,
            PointerType::Touch,
        ));
        recognizer.handle_event(&crate::events::make_move_event(
            position,
            PointerType::Touch,
        ));
        recognizer.handle_event(&crate::events::make_up_event(position, PointerType::Touch));

        assert!(recognizer.state.is_disposed());
        assert_eq!(recognizer.primary_pointer(), None);
    }
}
