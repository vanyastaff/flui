//! An accessibility announcement must never reach a log field verbatim.
//!
//! A live region is written to be read aloud to the user, so it routinely
//! carries their data — a sender's name, a balance, a validation echo. A
//! `tracing` field is world-readable in Apple's unified log and in Android's
//! logcat, which makes "log the announcement" the same thing as "publish it".
//!
//! Both fallbacks under test fire when the semantics binding is not
//! initialised, which is exactly the state a host embedding FLUI starts in.

use std::sync::{Arc, Mutex};

use flui_semantics::{SemanticsEvent, SemanticsService};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

/// A string that could not plausibly be anything but the payload.
const SECRET: &str = "Message from Alice: account balance is 12345";

#[derive(Clone, Default)]
struct FieldCapture(Arc<Mutex<Vec<(String, String)>>>);

struct Collector<'a>(&'a mut Vec<(String, String)>);

impl Visit for Collector<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.push((field.name().to_owned(), value.to_string()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.push((field.name().to_owned(), value.to_string()));
    }
}

impl<S> Layer<S> for FieldCapture
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut fields = Vec::new();
        event.record(&mut Collector(&mut fields));
        self.0
            .lock()
            .expect("BUG: the capture mutex is only locked by this test's own thread")
            .extend(fields);
    }
}

/// Every `name=value` pair emitted while `emit` ran.
///
/// Thread-local, so this touches no process-global subscriber and stays
/// order-independent alongside every other test in the crate.
fn captured_fields(emit: impl FnOnce()) -> Vec<(String, String)> {
    let capture = FieldCapture::default();
    tracing::subscriber::with_default(Registry::default().with(capture.clone()), emit);

    capture
        .0
        .lock()
        .expect("BUG: the capture mutex is only locked by this test's own thread")
        .clone()
}

fn assert_secret_absent(fields: &[(String, String)], what: &str) {
    for (name, value) in fields {
        assert!(
            !value.contains(SECRET),
            "{what} published the announcement verbatim in field `{name}` = {value:?}"
        );
    }
}

#[test]
fn announce_logs_the_length_and_never_the_message() {
    let fields = captured_fields(|| {
        SemanticsService::announce(SECRET);
    });

    assert!(
        !fields.is_empty(),
        "the uninitialised-binding fallback is expected to emit an event"
    );
    assert_secret_absent(&fields, "SemanticsService::announce");

    // The diagnostic still has to be worth emitting: the length is what tells a
    // developer the announcement happened and roughly how big it was.
    let length = fields
        .iter()
        .find(|(name, _)| name == "message_len")
        .map(|(_, value)| value.as_str());
    assert_eq!(
        length,
        Some(SECRET.len().to_string().as_str()),
        "expected a `message_len` field; captured {fields:?}"
    );
}

#[test]
fn send_event_logs_the_type_and_never_the_payload() {
    // `tooltip` builds a `SemanticsEvent` whose data is the string, so a
    // `Debug` of the whole event would print it.
    let fields = captured_fields(|| {
        SemanticsService::send_event(SemanticsEvent::tooltip(SECRET));
    });

    assert!(
        !fields.is_empty(),
        "the uninitialised-binding fallback is expected to emit an event"
    );
    assert_secret_absent(&fields, "SemanticsService::send_event");

    assert!(
        fields.iter().any(|(name, _)| name == "event_type"),
        "expected an `event_type` field; captured {fields:?}"
    );
}

#[test]
fn tooltip_routes_through_send_event_without_publishing_the_text() {
    // The convenience wrapper must not have its own logging path.
    let fields = captured_fields(|| {
        SemanticsService::tooltip(SECRET);
    });

    assert_secret_absent(&fields, "SemanticsService::tooltip");
}
