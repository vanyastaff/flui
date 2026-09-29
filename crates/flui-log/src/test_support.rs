//! In-crate helpers for unit tests.
//!
//! Nothing here touches the process-global subscriber: every helper installs a
//! subscriber for the duration of one closure on the current thread only, so
//! unit tests stay order-independent and can run in the same process. Scenarios
//! that genuinely need the global slot live in `tests/`, one per binary.

use std::sync::{Arc, Mutex};

use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

use crate::backend::record::FieldRecorder;

/// Layer that renders each event with [`FieldRecorder`] and keeps the lines.
#[derive(Clone, Default)]
struct RenderCapture(Arc<Mutex<Vec<String>>>);

impl<S> Layer<S> for RenderCapture
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut recorder = FieldRecorder::new();
        event.record(&mut recorder);
        self.0
            .lock()
            .expect("BUG: capture mutex is only locked by this test's own thread")
            .push(recorder.into_string());
    }
}

/// Run `emit` against a thread-local subscriber and return the rendered lines.
pub(crate) fn capture_rendered_events(emit: impl FnOnce()) -> Vec<String> {
    let capture = RenderCapture::default();
    let subscriber = Registry::default().with(capture.clone());

    tracing::subscriber::with_default(subscriber, emit);

    capture
        .0
        .lock()
        .expect("BUG: capture mutex is only locked by this test's own thread")
        .clone()
}
