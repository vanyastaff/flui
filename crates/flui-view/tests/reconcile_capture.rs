//! Shared tracing-capture fixture for reconcile-event tests.
//!
//! Every test that observes the `flui::reconcile` trace stream must capture
//! through this module — not by building its own scoped dispatcher — because
//! of a `tracing` callsite-interest race under parallel test threads:
//!
//! `tracing` caches per-callsite interest globally. While no interested
//! subscriber exists, a callsite is cached as `never`, and every
//! registration/drop of a dispatcher (including the scoped `with_default`
//! ones this fixture creates) rebuilds that cache. An event emitted on one
//! thread in the window where another thread's rebuild has the callsite at
//! `never` is skipped before the emitting thread's own dispatcher is even
//! consulted — the capture comes back empty. Installing one process-global,
//! always-enabled, no-op subscriber keeps every callsite's computed interest
//! non-`never` for the life of the process, so scoped capture dispatchers
//! always see their thread's events.
#![cfg(feature = "test-utils")]

use std::sync::OnceLock;

use flui_view::tree::test_utils::{CollectedEvent, ReconcileEventCollector};
use tracing::dispatcher::Dispatch;
use tracing_subscriber::{Registry, layer::SubscriberExt};

static GLOBAL_SUBSCRIBER: OnceLock<()> = OnceLock::new();

fn ensure_global_subscriber() {
    GLOBAL_SUBSCRIBER.get_or_init(|| {
        let _ = tracing::subscriber::set_global_default(Registry::default());
    });
}

/// Runs `body` with a scoped reconcile-event collector installed as this
/// thread's dispatcher and returns the events captured during the call.
pub fn capture<F: FnOnce()>(body: F) -> Vec<CollectedEvent> {
    ensure_global_subscriber();
    let collector = ReconcileEventCollector::new();
    let subscriber = Registry::default().with(collector.layer());
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
    collector.events()
}
