//! Observability for the gesture subsystem.
//!
//! This module provides the **shape** of observability events emitted by
//! `flui-interaction` — typed [`GestureEvent`] names + [`SPAN_RECOGNIZER`] /
//! [`SPAN_ARENA`] component-name constants. The crate emits events via
//! [`tracing`]; **consumers wire their
//! own subscriber** at the application boundary. `flui-app` configures
//! `flui-log`; an embedded host uses its own subscriber. `flui-interaction`
//! depends on neither.
//!
//! Recognizer admission and dispatch spans carry typed `event` fields, while
//! [`crate::recognizers::PrimaryContact`] reports tracking transitions and
//! [`crate::arena::GestureArena`] reports competition lifecycle events.
//! Subscriber code can reenter or panic: diagnostics run without recognizer
//! state borrows. Event-kind strings describe the observation; they do not
//! prescribe a span hierarchy or a devtools integration.
//!
//! # How to consume
//!
//! A managed application gets a subscriber from `run_app`; anything else calls
//! `flui_log::setup` (or installs its own). `flui-interaction` emits the
//! events; the subscriber (`tracing-subscriber` fmt, tracy, opentelemetry,
//! devtools, …) decides what to do with them. To filter on a specific event
//! kind:
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

/// Stable component name for application-defined recognizer spans.
///
/// Use as the `name` of a manually-entered `tracing::info_span!` or as a
/// tag in an application subscriber. Individual recognizer operations use
/// their own span names.
pub const SPAN_RECOGNIZER: &str = "gesture.recognizer";

/// Stable component name for application-defined arena spans.
/// [`crate::arena::GestureArena`] operations use their own span names.
pub const SPAN_ARENA: &str = "gesture.arena";

/// Typed names for gesture-lifecycle events emitted on the `tracing`
/// span hierarchy.
///
/// Used as the `event` field value for filter routing. The string
/// form is stable: tests and downstream `RUST_LOG` filters depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GestureEvent {
    /// Recognizer admission was invoked.
    RecognizerAdded,
    /// A recognizer received a pointer dispatch.
    EventReceived,
    /// Stable vocabulary for an accepted recognizer.
    ArenaAccepted,
    /// Contact membership was withdrawn from its arena.
    ArenaRejected,
    /// A primary contact finished pointer-up tracking.
    StoppedTracking,
    /// Legacy disposal diagnostic spelling, retained for consumers of old traces.
    /// Owner-local recognizers have no disposal operation.
    UsedAfterDispose,
    /// `GestureArena::sweep` removed the entry.
    ArenaSwept,
    /// `GestureArena::close` closed the entry.
    ArenaClosed,
    /// `GestureArena::resolve` resolved with a winner.
    ArenaResolved,
    /// A primary contact was admitted and began tracking.
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
