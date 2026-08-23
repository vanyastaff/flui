//! The build phase emits a `build` span.
//!
//! `flui-devtools` is layer 9 and its own manifest note says nothing in the
//! framework consumes it, so a frame profiler cannot be handed timings
//! directly — it has to subscribe. `layout`, `paint` and `compositing` already
//! emit spans from the pipeline; `build` was the missing fourth, and without it
//! any profiler would report a frame whose largest phase is simply absent.
//!
//! This asserts the span exists and carries its dirty-element count, because a
//! span that a refactor silently drops fails nothing otherwise — the profiler
//! would just start under-reporting.

use std::sync::{Arc, Mutex};

use flui_view::{BuildOwner, tree::ElementTree};
use tracing::{Dispatch, span::Attributes};
use tracing_subscriber::{Registry, layer::Context, layer::Layer, layer::SubscriberExt};

/// One opened span: its name and the fields it declares.
///
/// Fields are recorded because a profiler reads them. A collector that kept
/// only names would keep passing after `dirty_elements` was renamed away, and
/// the profiler would silently start reporting nothing for it.
#[derive(Clone, Debug, PartialEq)]
struct OpenedSpan {
    name: String,
    fields: Vec<String>,
}

#[derive(Clone, Default)]
struct SpanCollector {
    spans: Arc<Mutex<Vec<OpenedSpan>>>,
}

impl<S: tracing::Subscriber> Layer<S> for SpanCollector {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &tracing::Id, _ctx: Context<'_, S>) {
        let metadata = attrs.metadata();
        self.spans
            .lock()
            .expect("collector mutex is uncontended in a single-threaded test")
            .push(OpenedSpan {
                name: metadata.name().to_string(),
                fields: metadata
                    .fields()
                    .iter()
                    .map(|field| field.name().to_string())
                    .collect(),
            });
    }
}

/// Runs `body` with a span collector installed, returning every span opened.
fn spans_during(body: impl FnOnce()) -> Vec<OpenedSpan> {
    let collector = SpanCollector::default();
    let subscriber = Registry::default().with(collector.clone());
    // Disarm `tracing`'s process-global callsite-interest cache first: it is
    // computed on whichever thread reaches a callsite FIRST, so without this a
    // sibling test can have it cached as `never` and silently empty this capture.
    // See `flui_testing::log_capture`.
    flui_testing::log_capture::disarm_interest_cache();
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
    collector
        .spans
        .lock()
        .expect("collector mutex is uncontended")
        .clone()
}

#[test]
fn build_scope_emits_a_build_span() {
    let names = spans_during(|| {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        owner.build_scope(&mut tree);
    });

    let build = names
        .iter()
        .find(|span| span.name == "build")
        .unwrap_or_else(|| {
            panic!(
                "build_scope must open a `build` span so a tracing-subscribed \
                 profiler can time the phase; saw {names:?}"
            )
        });
    assert!(
        build.fields.iter().any(|field| field == "dirty_elements"),
        "the span must carry `dirty_elements` — a profiler reads it, so \
         renaming it away is a silent regression; saw {:?}",
        build.fields,
    );
}

/// An empty build still opens the span. A profiler that only saw the span on
/// dirty frames would silently attribute an idle frame's cost to whatever phase
/// ran next.
#[test]
fn the_span_opens_even_when_nothing_is_dirty() {
    let names = spans_during(|| {
        let mut owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        // Twice: the second pass has definitely nothing to do.
        owner.build_scope(&mut tree);
        owner.build_scope(&mut tree);
    });

    let build_spans = names.iter().filter(|span| span.name == "build").count();
    assert_eq!(
        build_spans, 2,
        "one span per build_scope call, dirty or not; saw {names:?}",
    );
}
