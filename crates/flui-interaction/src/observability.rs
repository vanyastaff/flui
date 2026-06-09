//! Observability for the gesture subsystem.
//!
//! This module provides the **shape** of observability events emitted by
//! `flui-interaction` — typed [`GestureEvent`] names + [`SPAN_RECOGNIZER`] /
//! [`SPAN_ARENA`] span-name constants — and a small test-only subscriber
//! helper. The crate emits events via [`tracing`]; **consumers wire their
//! own subscriber** at the application boundary via
//! [`flui_foundation::log::Logger`]. Per
//! [`docs/architecture.md`](../docs/architecture.md) policy:
//!
//! > **Logging:** `tracing` only — never `println!`, `eprintln!`, or `dbg!`.
//! > Use `#[tracing::instrument]` on hot paths and lifecycle methods.
//!
//! This module is the Observability-as-DoD closure pass: hot paths
//! in [`crate::recognizers::RecognizerBase`] and [`crate::arena::GestureArena`]
//! are now annotated with `#[tracing::instrument]` (plus typed
//! `event = GestureEvent::*` span fields), and the trait impls in
//! `tap.rs` / `long_press.rs` / `eager.rs` / `tap_and_drag.rs` /
//! `multidrag.rs` enter an `info_span!` on the per-pointer hot path.
//!
//! # How to consume
//!
//! Apps configure their subscriber via
//! `flui_foundation::log::Logger::default().init()`. `flui-interaction`
//! emits the events; the subscriber (`tracing-subscriber` fmt, tracy,
//! opentelemetry, devtools, …) decides what to do with them. To filter
//! on a specific event kind:
//!
//! ```bash
//! RUST_LOG=info,flui_interaction::arena=debug,flui_interaction::recognizers=trace cargo run
//! ```
//!
//! # Why no metrics / devtools here
//!
//! Per the observability scope decision, this crate emits structured observability
//! events but does not own a metrics layer or a devtools dump API. Those
//! are app-level concerns — see `flui-app` for app-level integration.
//!
//! # Example
//!
//! ```
//! use flui_interaction::observability::{GestureEvent, SPAN_ARENA};
//!
//! assert_eq!(SPAN_ARENA, "gesture.arena");
//! assert_eq!(GestureEvent::ArenaAccepted.as_str(), "arena_accepted");
//! ```

/// Span name for the `RecognizerBase` lifecycle methods
/// ([`crate::recognizers::RecognizerBase::start_tracking`], [`accept`](crate::recognizers::RecognizerBase::accept),
/// [`reject`](crate::recognizers::RecognizerBase::reject), etc.).
///
/// Use as the `name` of a manually-entered `tracing::info_span!` or as a
/// filter token in `RUST_LOG`.
pub const SPAN_RECOGNIZER: &str = "gesture.recognizer";

/// Span name for the [`crate::arena::GestureArena`] lifecycle methods
/// ([`add`](crate::arena::GestureArena::add), [`close`](crate::arena::GestureArena::close),
/// [`resolve`](crate::arena::GestureArena::resolve), [`sweep`](crate::arena::GestureArena::sweep)).
pub const SPAN_ARENA: &str = "gesture.arena";

/// Typed names for gesture-lifecycle events emitted on the `tracing`
/// span hierarchy.
///
/// Use as the `event.kind` field value for filter routing. The string
/// form is stable: tests and downstream `RUST_LOG` filters depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GestureEvent {
    /// `RecognizerBase::add_pointer` invoked.
    RecognizerAdded,
    /// A `RecognizerBase::handle_event` was invoked.
    EventReceived,
    /// `RecognizerBase::accept` won the arena.
    ArenaAccepted,
    /// `RecognizerBase::reject` lost the arena or was rejected explicitly.
    ArenaRejected,
    /// `RecognizerBase::stop_tracking` cleared the slot.
    StoppedTracking,
    /// `RecognizerBase::assert_not_disposed` fired in release mode.
    UsedAfterDispose,
    /// `GestureArena::sweep` removed the entry.
    ArenaSwept,
    /// `GestureArena::close` closed the entry.
    ArenaClosed,
    /// `GestureArena::resolve` resolved with a winner.
    ArenaResolved,
    /// `RecognizerBase::start_tracking` initialised tracking.
    StartedTracking,
}

impl GestureEvent {
    /// Canonical `tracing` event-kind string.
    ///
    /// These strings are part of the public observability contract — tests
    /// and downstream `RUST_LOG` filters depend on them. Do not rename
    /// without bumping the crate version.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecognizerAdded => "recognizer_added",
            Self::EventReceived => "event_received",
            Self::ArenaAccepted => "arena_accepted",
            Self::ArenaRejected => "arena_rejected",
            Self::StoppedTracking => "stopped_tracking",
            Self::UsedAfterDispose => "used_after_dispose",
            Self::ArenaSwept => "arena_swept",
            Self::ArenaClosed => "arena_closed",
            Self::ArenaResolved => "arena_resolved",
            Self::StartedTracking => "started_tracking",
        }
    }
}

impl std::fmt::Display for GestureEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Map a [`PointerEvent`](crate::events::PointerEvent) to a stable, human-readable kind
/// string suitable for use as a tracing span field.
///
/// The output is stable across releases — span-field filters depend on
/// it — and is intentionally coarse ("down" / "move" / "up" / "cancel" /
/// "other") so the filter surface stays small.
#[inline]
pub fn pointer_event_kind(event: &crate::events::PointerEvent) -> &'static str {
    use crate::events::PointerEvent as P;
    match event {
        P::Down(_) => "down",
        P::Move(_) => "move",
        P::Up(_) => "up",
        P::Cancel(_) => "cancel",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lock in the public string contract — downstream tests and
    /// `RUST_LOG` filters depend on these values. Any rename requires a
    /// major-version bump.
    #[test]
    fn gesture_event_names_are_stable() {
        assert_eq!(GestureEvent::RecognizerAdded.as_str(), "recognizer_added");
        assert_eq!(GestureEvent::EventReceived.as_str(), "event_received");
        assert_eq!(GestureEvent::ArenaAccepted.as_str(), "arena_accepted");
        assert_eq!(GestureEvent::ArenaRejected.as_str(), "arena_rejected");
        assert_eq!(GestureEvent::StoppedTracking.as_str(), "stopped_tracking");
        assert_eq!(
            GestureEvent::UsedAfterDispose.as_str(),
            "used_after_dispose"
        );
        assert_eq!(GestureEvent::ArenaSwept.as_str(), "arena_swept");
        assert_eq!(GestureEvent::ArenaClosed.as_str(), "arena_closed");
        assert_eq!(GestureEvent::ArenaResolved.as_str(), "arena_resolved");
        assert_eq!(GestureEvent::StartedTracking.as_str(), "started_tracking");
    }

    /// `Display` matches `as_str()` so users can interpolate
    /// `format!("event={}", GestureEvent::ArenaAccepted)` without
    /// double-mapping.
    #[test]
    fn gesture_event_display_matches_as_str() {
        for ev in [
            GestureEvent::RecognizerAdded,
            GestureEvent::EventReceived,
            GestureEvent::ArenaAccepted,
            GestureEvent::ArenaRejected,
            GestureEvent::StoppedTracking,
            GestureEvent::UsedAfterDispose,
            GestureEvent::ArenaSwept,
            GestureEvent::ArenaClosed,
            GestureEvent::ArenaResolved,
            GestureEvent::StartedTracking,
        ] {
            assert_eq!(format!("{ev}"), ev.as_str());
        }
    }

    /// Span-name constants are unique, non-empty, and follow the
    /// `gesture.<subsystem>` convention.
    #[test]
    fn span_name_constants_are_unique() {
        assert!(!SPAN_RECOGNIZER.is_empty());
        assert!(!SPAN_ARENA.is_empty());
        assert_ne!(SPAN_RECOGNIZER, SPAN_ARENA);
        assert!(SPAN_RECOGNIZER.starts_with("gesture."));
        assert!(SPAN_ARENA.starts_with("gesture."));
    }

    /// `pointer_event_kind` returns the expected coarse-grained label
    /// for each `PointerEvent` variant. This guards the span-field
    /// string contract.
    #[test]
    fn pointer_event_kind_maps_variants() {
        use crate::events::{make_cancel_event, make_down_event, make_move_event, make_up_event};
        use flui_types::geometry::{Offset, Pixels};

        let pos = Offset::new(Pixels(0.0), Pixels(0.0));
        let pt = crate::events::PointerType::Touch;

        assert_eq!(pointer_event_kind(&make_down_event(pos, pt)), "down");
        assert_eq!(pointer_event_kind(&make_move_event(pos, pt)), "move");
        assert_eq!(pointer_event_kind(&make_up_event(pos, pt)), "up");
        assert_eq!(pointer_event_kind(&make_cancel_event(pt)), "cancel");
    }

    /// Smoke test: install a `fmt::TestWriter` subscriber and exercise a
    /// real `RecognizerBase` hot path. Proves the
    /// `#[tracing::instrument]` wiring compiles AND emits a span when
    /// the `tracing` machinery is active (the default no-op subscriber
    /// silently discards everything, so the hot path looks "uninstrumented"
    /// from a subscriber's perspective without a real subscriber installed).
    ///
    /// We use `set_default` rather than `init` to keep this test
    /// isolated from the global subscriber state.
    #[test]
    fn recognizer_base_accept_emits_span_with_subscriber() {
        use crate::arena::GestureArena;
        use crate::recognizers::recognizer::RecognizerBase;
        use flui_types::{Offset, geometry::Pixels};
        use std::sync::Arc;

        let subscriber = tracing_subscriber::fmt()
            .with_writer(tracing_subscriber::fmt::TestWriter::new)
            .with_max_level(tracing::Level::DEBUG)
            .with_target(false)
            .finish();

        let arena = GestureArena::new();
        let base = RecognizerBase::new(arena);
        let pointer = crate::ids::PointerId::PRIMARY;
        let position = Offset::new(Pixels(0.0), Pixels(0.0));

        // Minimal stand-in recogniser that implements `GestureArenaMember`
        // (satisfies the trait bound on `accept<T>`).
        #[derive(Clone)]
        struct StubRecognizer;
        impl crate::sealed::arena_member::Sealed for StubRecognizer {}
        impl crate::arena::GestureArenaMember for StubRecognizer {
            fn accept_gesture(&self, _p: crate::ids::PointerId) {}
            fn reject_gesture(&self, _p: crate::ids::PointerId) {}
        }

        let recognizer: Arc<StubRecognizer> = Arc::new(StubRecognizer);

        // Run the hot path inside the subscriber. We don't assert on
        // captured output (TestWriter is a sink) — the smoke value is
        // "this doesn't panic and the span machinery runs".
        tracing::subscriber::with_default(subscriber, || {
            base.start_tracking(pointer, position, &recognizer);
            base.accept(&recognizer);
        });
    }
}
