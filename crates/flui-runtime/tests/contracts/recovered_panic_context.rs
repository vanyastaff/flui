//! A recovered lifecycle panic reaches the frame-failure handler with where in the element tree it
//! happened.

use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Duration;

use flui_foundation::{CreationSite, ManualClock, MonotonicClock, TreeContext};
use flui_runtime::frame_failure::{FrameFailureHandler, FrameFailureKind};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_view::prelude::*;

/// A view whose `build` panics, made by a constructor that records its site.
#[derive(Clone, StatelessView)]
struct PanicsInBuild {
    #[view(site)]
    site: CreationSite,
}

impl PanicsInBuild {
    #[track_caller]
    fn new() -> Self {
        Self {
            site: CreationSite::here(),
        }
    }
}

impl StatelessView for PanicsInBuild {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        assert!(!std::hint::black_box(true), "induced build panic");
        flui_widgets::SizedBox::shrink()
    }
}

/// The parent of the panicking view.
#[derive(Clone, StatelessView)]
struct Middle {
    child: PanicsInBuild,
}

impl StatelessView for Middle {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}

/// The grandparent of the panicking view.
#[derive(Clone, StatelessView)]
struct Outer {
    child: Middle,
}

impl StatelessView for Outer {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
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

/// A `build` panic under `Outer > Middle` is reported as a recovered panic whose context runs
/// from the panicked view up through its parents and names where the view was created.
///
/// Fails without the binding's fill of the path at the drain: the report's path is then empty.
#[test]
fn a_recovered_panic_report_names_its_ancestors_and_creation_site() {
    let clock = ManualClock::new();
    let mut runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime");
    let contexts: Arc<Mutex<Vec<TreeContext>>> = Arc::default();
    let reported = Arc::clone(&contexts);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        if let FrameFailureKind::RecoveredPanic { context, .. } = &report.kind {
            reported
                .lock()
                .expect("recovered panic contexts")
                .push(context.clone());
        }
    })));

    let view = PanicsInBuild::new();
    let line = line!() - 1;
    runtime
        .attach_root_widget_with_size(
            &Outer {
                child: Middle { child: view },
            },
            800.0,
            600.0,
        )
        .expect("root");
    clock.advance(Duration::from_millis(16));
    let _ = runtime.pump(
        &mut flui_runtime::pump::SampledClock(clock.now()),
        &mut DiscardSink,
    );

    let contexts = contexts.lock().expect("recovered panic contexts");
    let [context] = contexts.as_slice() else {
        panic!("one recovered panic is reported: {contexts:?}");
    };
    let kinds: Vec<&str> = context
        .path()
        .iter()
        .map(flui_foundation::PathEntry::kind)
        .collect();
    assert_eq!(
        kinds.get(..3),
        Some(&["PanicsInBuild", "Middle", "Outer"][..]),
        "the path runs from the panicked view up through its parents: {context}"
    );
    if CreationSite::KEPT {
        let site = context.site();
        assert!(
            site.file()
                .is_some_and(|file| file.ends_with("recovered_panic_context.rs"))
                && site.line() == Some(line),
            "the site names the constructor call on line {line}: {site:?}"
        );
    }
}
