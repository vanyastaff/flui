//! Observability for the gesture subsystem.
//!
//! This module provides the **shape** of observability events emitted by
//! `flui-interaction` — typed [`GestureEvent`] names + [`SPAN_RECOGNIZER`] /
//! [`SPAN_ARENA`] span-name constants — and a small test-only subscriber
//! helper. The crate emits events via [`tracing`]; **consumers wire their
//! own subscriber** at the application boundary — `flui-app` and `flui-cli`
//! do that through `flui-log`, and an embedded host does it however it
//! already does. `flui-interaction` depends on neither. Per
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

/// Span name for the `RecognizerBase` lifecycle methods
/// ([`crate::recognizers::RecognizerBase::start_tracking`], [`accept_tracked`](crate::recognizers::RecognizerBase::accept_tracked),
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
    /// `RecognizerBase::accept_tracked` won the arena.
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
mod tests {}
