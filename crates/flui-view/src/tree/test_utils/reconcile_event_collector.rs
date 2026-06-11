//! `tracing_subscriber::Layer` implementation that captures
//! [`ReconcileEvent`](super::super::reconcile_event::ReconcileEvent)
//! emissions into an `Arc<Mutex<Vec<CollectedEvent>>>` so tests can
//! assert on the reconciler's trace stream.
//!
//! Plan §U14 / FR-035.
//!
//! # Why a parallel `CollectedEvent` type?
//!
//! The emitted [`tracing::Event`] carries the `view_type_id` as a
//! `Debug`-formatted string — `TypeId`'s `Debug` representation is
//! the only stable identifier available (no public `to_u128()`
//! method, no `Display` impl). Reconstructing the original `TypeId`
//! from the Debug string is not generally possible, so the collector
//! exposes a `CollectedEvent` shape with `view_type_id: String`
//! instead of `TypeId`. Tests compare on the Debug-string
//! representation, which is what the field actually carries on the
//! wire.
//!
//! Every other field IS typed (the `u64` / `bool` primitives from
//! FEAS-008) so the collector reads them via
//! [`tracing::field::Visit::record_u64`] /
//! [`tracing::field::Visit::record_bool`] without any
//! Debug-string parsing in the hot path.
//!
//! # Installation pattern (per-thread)
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use flui_view::tree::test_utils::ReconcileEventCollector;
//! use tracing_subscriber::{Registry, layer::SubscriberExt};
//!
//! let collector = ReconcileEventCollector::new();
//! let subscriber = Registry::default().with(collector.layer());
//! tracing::dispatcher::with_default(&tracing::Dispatch::new(subscriber), || {
//!     // ... code that calls `reconcile_children` ...
//! });
//! let events = collector.events();
//! assert!(!events.is_empty(), "vacuous-pass guard — must observe events");
//! ```
//!
//! The reconciler MUST NOT spawn worker threads emitting
//! `flui::reconcile` events while a per-thread collector is
//! installed; if a future optimisation requires that, switch to
//! `tracing::dispatcher::set_default()` (global) and gate the
//! affected tests behind `#[serial_test::serial]`. Phase 1 ships the
//! per-thread discipline (KTD-5).
//!
//! Even with per-thread dispatchers, tests installing a collector
//! must be `#[serial_test::serial]`-gated: tracing-core's callsite
//! interest cache is process-global, and concurrent dispatcher
//! installs/drops race its rebuild — a freshly installed collector
//! can then miss events, tripping the vacuous-pass guard.

use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Metadata, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use super::super::reconcile_event::{RECONCILE_TARGET, ReconcileEventKind};

/// Reconstructed reconciliation event suitable for test assertions.
///
/// Mirrors [`super::super::reconcile_event::ReconcileEvent`] but
/// stores `view_type_id` as the Debug-formatted string the trace
/// carries — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectedEvent {
    /// Parsed disposition.
    pub kind: ReconcileEventKind,
    /// Owning parent's packed `u64` (`ElementId::as_u64()`).
    pub parent: u64,
    /// `Some(hash)` when the child carries a key.
    pub child_key: Option<u64>,
    /// New slot index.
    pub slot: u64,
    /// `format!("{:?}", view.view_type_id())` — opaque string,
    /// useful only for grouping events by widget type in assertions.
    pub view_type_id: String,
    /// `Some(parent_id)` on a `Reparent` event; `None` otherwise.
    pub from_parent: Option<u64>,
}

/// Thread-safe sink for emitted reconciliation events.
///
/// Installed via `tracing::dispatcher::with_default`. The handle is
/// `Clone` so the same collector can be installed on multiple
/// threads (each thread's emissions land in the same backing
/// `Vec`).
#[derive(Debug, Clone, Default)]
pub struct ReconcileEventCollector {
    events: Arc<Mutex<Vec<CollectedEvent>>>,
}

impl ReconcileEventCollector {
    /// Construct an empty collector.
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Build a `tracing_subscriber::Layer` that records into this
    /// collector. The layer keeps an `Arc` to the events vec, so
    /// the collector survives as long as either side does.
    pub fn layer(&self) -> CollectorLayer {
        CollectorLayer {
            events: Arc::clone(&self.events),
        }
    }

    /// Snapshot the captured events.
    ///
    /// Returns a fresh `Vec` clone; the underlying buffer keeps
    /// growing as more events arrive. Tests typically call
    /// `events()` after the reconciler returns and assert on the
    /// snapshot.
    pub fn events(&self) -> Vec<CollectedEvent> {
        // Poisoned lock means a panic in another thread mid-push.
        // Unwrap the poison and read the data anyway — losing the
        // mid-push event is acceptable for a TEST helper; better to
        // surface the assertion than to swallow the poison silently.
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Clear the captured events. Useful between phases of a
    /// multi-frame test.
    pub fn clear(&self) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

/// The `Layer` impl returned by [`ReconcileEventCollector::layer`].
///
/// Lives as a distinct type so the collector handle stays cloneable
/// without dragging trait-object impl complexity onto its public
/// API.
#[derive(Debug, Clone)]
pub struct CollectorLayer {
    events: Arc<Mutex<Vec<CollectedEvent>>>,
}

impl<S> Layer<S> for CollectorLayer
where
    S: Subscriber,
{
    fn enabled(&self, metadata: &Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        // Short-circuit at the metadata stage so we never pay the
        // event-construction cost for unrelated targets. The
        // `flui::reconcile` filter is the single hot-path discriminator.
        metadata.target() == RECONCILE_TARGET
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Defensive: `enabled` already filters, but the layer can be
        // composed with subscribers that override filtering, so
        // re-check.
        if event.metadata().target() != RECONCILE_TARGET {
            return;
        }
        let mut visitor = EventFieldVisitor::default();
        event.record(&mut visitor);
        if let Some(collected) = visitor.build() {
            // Same poison-tolerant lock as `events()`.
            self.events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(collected);
        }
    }

    // `Layer` requires no-op default impls for span lifecycle
    // methods; keep them explicit so a future bump to a stricter
    // Layer surface surfaces here as a missing-method error.
    fn on_new_span(&self, _attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {}
    fn on_record(&self, _id: &Id, _values: &Record<'_>, _ctx: Context<'_, S>) {}
    fn on_enter(&self, _id: &Id, _ctx: Context<'_, S>) {}
    fn on_exit(&self, _id: &Id, _ctx: Context<'_, S>) {}
    fn on_close(&self, _id: Id, _ctx: Context<'_, S>) {}
}

/// Accumulates typed field values during `Event::record`.
///
/// Each field is recorded by name; `build()` validates the required
/// set and produces a `CollectedEvent`. A malformed event (missing
/// required field, unknown `kind` discriminant) returns `None` so the
/// collector silently drops it — tests assert on what they expect
/// to see, not on the absence of malformed events.
#[derive(Default)]
struct EventFieldVisitor {
    kind: Option<u8>,
    parent: Option<u64>,
    child_key: Option<u64>,
    child_key_present: Option<bool>,
    slot: Option<u64>,
    view_type_id: Option<String>,
    from_parent: Option<u64>,
    from_parent_present: Option<bool>,
}

impl Visit for EventFieldVisitor {
    fn record_u64(&mut self, field: &Field, value: u64) {
        match field.name() {
            "kind" => self.kind = u8::try_from(value).ok(),
            "parent" => self.parent = Some(value),
            "child_key" => self.child_key = Some(value),
            "slot" => self.slot = Some(value),
            "from_parent" => self.from_parent = Some(value),
            _ => {}
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        match field.name() {
            "child_key_present" => self.child_key_present = Some(value),
            "from_parent_present" => self.from_parent_present = Some(value),
            _ => {}
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "view_type_id" {
            self.view_type_id = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // Fallback for fields that fall through to Debug formatting —
        // notably some tracing emit-paths route `%expr` through
        // `record_str` and `?expr` through `record_debug`. The
        // `view_type_id` is emitted with `%` so it lands on
        // `record_str`; this branch covers an alternate code path
        // (some `tracing` versions route both display-and-debug-formatted
        // fields through this method when no faster Visit method
        // matches).
        if field.name() == "view_type_id" && self.view_type_id.is_none() {
            self.view_type_id = Some(format!("{value:?}"));
        }
    }
}

impl EventFieldVisitor {
    fn build(self) -> Option<CollectedEvent> {
        let kind = ReconcileEventKind::from_u8(self.kind?)?;
        let parent = self.parent?;
        let slot = self.slot?;
        let view_type_id = self.view_type_id?;
        let child_key = if self.child_key_present? {
            Some(self.child_key?)
        } else {
            None
        };
        let from_parent = if self.from_parent_present? {
            Some(self.from_parent?)
        } else {
            None
        };
        Some(CollectedEvent {
            kind,
            parent,
            child_key,
            slot,
            view_type_id,
            from_parent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::reconcile_event::{ReconcileEvent, emit as emit_event};
    use super::*;

    use flui_foundation::ElementId;
    use std::any::TypeId;
    use tracing::dispatcher::Dispatch;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    /// Run `body` with the collector's layer installed on the current
    /// thread, then return the captured events.
    fn with_collector<F: FnOnce()>(body: F) -> Vec<CollectedEvent> {
        let collector = ReconcileEventCollector::new();
        let subscriber = Registry::default().with(collector.layer());
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
        collector.events()
    }

    /// Vacuous-pass guard discipline: positive count BEFORE absence
    /// assertions. A test that never observed ANY event is not proof
    /// of "the right events fired" — it could just mean the
    /// dispatcher was never installed. This helper makes the check
    /// explicit so a future regression where the layer-install path
    /// is broken surfaces as an assertion failure here, not as a
    /// silent green test.
    fn assert_positive_count(events: &[CollectedEvent], min: usize) {
        assert!(
            events.len() >= min,
            "vacuous-pass guard: expected at least {} events, observed {}",
            min,
            events.len(),
        );
    }

    // All five tests below install a per-thread dispatcher via
    // `tracing::dispatcher::with_default`. Installing/dropping a
    // dispatcher triggers tracing-core's global callsite
    // interest-cache rebuild; under parallel test execution that
    // rebuild races between threads and a freshly installed
    // per-thread collector can miss events (observed as a
    // vacuous-pass-guard failure in full-workspace runs). The module
    // docs prescribe `#[serial_test::serial]` gating for exactly this
    // hazard.
    #[test]
    #[serial_test::serial]
    fn collector_captures_mount_event() {
        let events = with_collector(|| {
            emit_event(&ReconcileEvent::mount(
                ElementId::new(7),
                3,
                TypeId::of::<u32>(),
                Some(0xDEAD),
            ));
        });
        assert_positive_count(&events, 1);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.kind, ReconcileEventKind::Mount);
        assert_eq!(e.parent, ElementId::new(7).as_u64());
        assert_eq!(e.slot, 3);
        assert_eq!(e.child_key, Some(0xDEAD));
        assert_eq!(e.from_parent, None);
        assert!(!e.view_type_id.is_empty(), "TypeId Debug must be present");
    }

    #[test]
    #[serial_test::serial]
    fn collector_captures_all_five_kinds() {
        let events = with_collector(|| {
            let parent = ElementId::new(1);
            let donor = ElementId::new(2);
            let tid = TypeId::of::<String>();
            emit_event(&ReconcileEvent::mount(parent, 0, tid, None));
            emit_event(&ReconcileEvent::unmount(parent, 1, tid, Some(5)));
            emit_event(&ReconcileEvent::reuse(parent, 2, tid, None));
            emit_event(&ReconcileEvent::reorder(parent, 3, tid, Some(7)));
            emit_event(&ReconcileEvent::reparent(donor, parent, 4, tid, 0xBEEF));
        });
        assert_positive_count(&events, 5);
        assert_eq!(events.len(), 5);
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                ReconcileEventKind::Mount,
                ReconcileEventKind::Unmount,
                ReconcileEventKind::Reuse,
                ReconcileEventKind::Reorder,
                ReconcileEventKind::Reparent,
            ],
            "all five variants must round-trip through the wire",
        );
        // Reparent is the only variant carrying from_parent.
        // donor = ElementId::new(2); as_u64() encodes the packed generational id.
        assert_eq!(events[4].from_parent, Some(ElementId::new(2).as_u64()));
        for non_reparent in &events[..4] {
            assert!(non_reparent.from_parent.is_none());
        }
    }

    #[test]
    #[serial_test::serial]
    fn collector_ignores_other_targets() {
        let events = with_collector(|| {
            // Emission on a different target must NOT land in the
            // collector — proves the `enabled()` short-circuit and
            // the defensive `on_event` re-check both honour the
            // `RECONCILE_TARGET` filter.
            tracing::event!(
                target: "flui::other",
                tracing::Level::TRACE,
                kind = 0_u64,
                parent = 1_u64,
                slot = 0_u64,
                view_type_id = %"ignored",
                child_key = 0_u64,
                child_key_present = false,
                from_parent = 0_u64,
                from_parent_present = false,
            );
        });
        assert_eq!(
            events.len(),
            0,
            "events on unrelated targets must NOT be captured",
        );
    }

    #[test]
    #[serial_test::serial]
    fn collector_clear_resets_buffer() {
        let collector = ReconcileEventCollector::new();
        let subscriber = Registry::default().with(collector.layer());
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            emit_event(&ReconcileEvent::mount(
                ElementId::new(1),
                0,
                TypeId::of::<()>(),
                None,
            ));
            emit_event(&ReconcileEvent::mount(
                ElementId::new(1),
                1,
                TypeId::of::<()>(),
                None,
            ));
        });
        assert_eq!(collector.events().len(), 2);
        collector.clear();
        assert_eq!(collector.events().len(), 0);
    }

    #[test]
    #[serial_test::serial]
    fn malformed_event_dropped_silently() {
        // Emit a partial event (missing required fields) on the
        // reconcile target. The collector's `build()` returns None,
        // dropping the event without panicking — production
        // observability code MUST NOT crash on a future field-set
        // mismatch.
        let events = with_collector(|| {
            tracing::event!(
                target: RECONCILE_TARGET,
                tracing::Level::TRACE,
                kind = 0_u64,
                // `parent` deliberately omitted — required field.
            );
        });
        assert_eq!(events.len(), 0, "malformed event must be silently dropped");
    }
}
