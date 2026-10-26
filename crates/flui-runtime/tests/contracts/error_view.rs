//! The error view factory belongs to the UI runtime that was configured with it.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_view::{ErrorView, FrameworkError, prelude::*};

#[derive(Clone, StatelessView)]
struct Panics(bool);

impl StatelessView for Panics {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        assert!(!self.0, "error view contract build failure");
        flui_widgets::SizedBox::shrink()
    }
}

struct DiscardSink;

impl FrameSink for DiscardSink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }

    fn submit(&mut self, _scene: flui_layer::Scene) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

fn runtime(clock: &ManualClock) -> UiRuntime {
    UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime")
}

fn frame(runtime: &mut UiRuntime, clock: &ManualClock) {
    clock.advance(Duration::from_millis(16));
    let _ = runtime.pump(
        &mut flui_runtime::pump::SampledClock(clock.now()),
        &mut DiscardSink,
    );
}

#[test]
fn the_error_view_factory_reaches_every_presentation_of_its_runtime_only() {
    static CONFIGURED: AtomicUsize = AtomicUsize::new(0);
    fn configured(error: &FrameworkError) -> Box<dyn View> {
        CONFIGURED.fetch_add(1, Ordering::SeqCst);
        Box::new(ErrorView::new(format!("configured: {}", error.message)))
    }

    let clock = ManualClock::new();
    let mut configured_runtime = runtime(&clock);
    let mut other_runtime = runtime(&clock);
    configured_runtime.set_error_view_builder(Some(configured));

    configured_runtime
        .attach_root_widget_with_size(&Panics(true), 800.0, 600.0)
        .expect("first root");
    frame(&mut configured_runtime, &clock);
    assert_eq!(
        CONFIGURED.load(Ordering::SeqCst),
        1,
        "the first presentation recovers through the runtime's factory"
    );

    let sibling = configured_runtime
        .presentation_factory()
        .assemble(crate::owner_publication::window())
        .expect("assemble sibling");
    let sibling = configured_runtime.install_presentation(sibling);
    configured_runtime
        .attach_root_widget_with_size_to(sibling, &Panics(true), 800.0, 600.0)
        .expect("sibling root");
    frame(&mut configured_runtime, &clock);
    assert_eq!(
        CONFIGURED.load(Ordering::SeqCst),
        2,
        "a presentation installed later inherits the factory"
    );

    other_runtime
        .attach_root_widget_with_size(&Panics(true), 800.0, 600.0)
        .expect("other root");
    frame(&mut other_runtime, &clock);
    assert_eq!(
        CONFIGURED.load(Ordering::SeqCst),
        2,
        "another runtime in the process keeps the default error view"
    );
}
