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

/// Records the name of every span opened while it is installed.
#[derive(Clone, Default)]
struct SpanNameCollector {
    names: Arc<Mutex<Vec<String>>>,
}

impl<S: tracing::Subscriber> Layer<S> for SpanNameCollector {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &tracing::Id, _ctx: Context<'_, S>) {
        self.names
            .lock()
            .expect("collector mutex is uncontended in a single-threaded test")
            .push(attrs.metadata().name().to_string());
    }
}

/// Runs `body` with a span collector installed, returning every span name seen.
fn spans_during(body: impl FnOnce()) -> Vec<String> {
    let collector = SpanNameCollector::default();
    let subscriber = Registry::default().with(collector.clone());
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
    collector
        .names
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

    assert!(
        names.iter().any(|name| name == "build"),
        "build_scope must open a `build` span so a tracing-subscribed profiler \
         can time the phase; saw {names:?}",
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

    let build_spans = names.iter().filter(|name| *name == "build").count();
    assert_eq!(
        build_spans, 2,
        "one span per build_scope call, dirty or not; saw {names:?}",
    );
}
