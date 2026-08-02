//! Shared helpers for the process-global integration tests.
//!
//! Each test binary in this directory owns exactly one scenario, because the
//! global subscriber slot can only be written once per process. `cargo nextest`
//! gives every test its own process, so the scenarios never see each other's
//! state and never depend on execution order.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::sync::{Arc, Mutex};

use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// One captured event: its level, its target, and its `name=value` fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedEvent {
    pub level: String,
    pub target: String,
    pub fields: Vec<(String, String)>,
}

impl CapturedEvent {
    /// The value recorded for `name`, if the event carried it.
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }
}

/// A layer that keeps every event it sees.
///
/// Stacked *above* FLUI's default subscriber in the tests that need to observe
/// what actually reached the backend.
#[derive(Debug, Clone, Default)]
pub struct CaptureLayer {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl CaptureLayer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything captured so far.
    #[must_use]
    pub fn events(&self) -> Vec<CapturedEvent> {
        self.events
            .lock()
            .expect("BUG: the capture mutex is only locked by the test's own threads")
            .clone()
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut fields = Vec::new();
        event.record(&mut FieldCollector(&mut fields));

        let metadata = event.metadata();
        self.events
            .lock()
            .expect("BUG: the capture mutex is only locked by the test's own threads")
            .push(CapturedEvent {
                level: metadata.level().to_string(),
                target: metadata.target().to_owned(),
                fields,
            });
    }
}

struct FieldCollector<'a>(&'a mut Vec<(String, String)>);

impl tracing::field::Visit for FieldCollector<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

/// A minimal subscriber standing in for "the host already owns logging".
///
/// Deliberately not built from `flui-log`: the point of the tests that install
/// it is that FLUI must preserve a subscriber it knows nothing about.
#[derive(Debug, Clone, Default)]
pub struct HostSubscriber {
    events: Arc<Mutex<Vec<String>>>,
}

impl HostSubscriber {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Targets of the events this subscriber saw.
    #[must_use]
    pub fn targets(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("BUG: the host mutex is only locked by the test's own threads")
            .clone()
    }
}

impl Subscriber for HostSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &Event<'_>) {
        self.events
            .lock()
            .expect("BUG: the host mutex is only locked by the test's own threads")
            .push(event.metadata().target().to_owned());
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}
