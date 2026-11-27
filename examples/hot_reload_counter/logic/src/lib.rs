//! Reloadable UI logic for the hot-reload counter example.
//!
//! Edit `INCREMENT_LABEL` below and save while `flui run` runs the host: it
//! rebuilds this crate, stages the dylib with its version in the
//! `.flui_worker_plugin` manifest, and the host reassembles while preserving
//! the counter value in the host element tree. A host run by hand loads the
//! worker it finds at start, and without the manifest's version it reloads a
//! rebuilt worker but reassembles nothing (ADR-0393 §3).
//!
//! ```bash
//! flui run
//! ```

use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

use flui_hot_reload::{WorkerBuildEnv, hot_reload_worker, request_rebuild};
use flui_painting::styling::Color;
use flui_view::prelude::*;
use flui_widgets::{ColoredBox, Column, GestureDetector, Padding, Text};
use hot_reload_counter_types::{CounterApp, CounterAppState, TYPE_FINGERPRINT};

/// **Edit this string and rebuild to test hot reload.**
const INCREMENT_LABEL: &str = "Increment (+1)";

fn build_counter_ui(
    _env: WorkerBuildEnv<'_>,
    state: &CounterAppState,
    _view: &CounterApp,
) -> flui_view::BoxedView {
    let count = state.count().load(Ordering::Relaxed);
    let count_for_tap: Arc<AtomicI32> = Arc::clone(state.count());

    Column::new(vec![
        Text::new(format!("Count: {count}")).boxed(),
        GestureDetector::new()
            .on_tap(move |_ctx| {
                count_for_tap.fetch_add(1, Ordering::Relaxed);
                request_rebuild();
            })
            .child(
                Padding::all(12.0).child(
                    ColoredBox::new(Color::rgb(40, 100, 200)).child(Text::new(INCREMENT_LABEL)),
                ),
            )
            .boxed(),
    ])
    .boxed()
}

fn init_counter_worker(register: flui_hot_reload::RegisterWorkerBuildFn) {
    register(TYPE_FINGERPRINT, build_counter_ui as *const ());
}

hot_reload_worker!(init_counter_worker, fingerprint: TYPE_FINGERPRINT);
