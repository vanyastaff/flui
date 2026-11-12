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

    fn submit(&mut self, _scene: flui_layer::ScenePublication) -> SubmitVerdict {
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

/// A factory installed on two runtimes builds one handler for each, and each handler receives
/// only its own runtime's reports: state a handler captures is per runtime, and it may be
/// owner-local (`Rc`).
#[test]
fn a_handler_factory_gives_each_runtime_its_own_handler() {
    let clock = ManualClock::new();
    let built = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reports: Arc<Mutex<Vec<(usize, u32)>>> = Arc::default();
    let factory = flui_runtime::frame_failure::FrameFailureHandlerFactory::new({
        let built = Arc::clone(&built);
        let reports = Arc::clone(&reports);
        move || {
            let handler = built.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let seen = std::rc::Rc::new(std::cell::Cell::new(0_u32));
            let reports = Arc::clone(&reports);
            FrameFailureHandler::new(move |report| {
                if matches!(report.kind, FrameFailureKind::RecoveredPanic { .. }) {
                    seen.set(seen.get() + 1);
                    reports
                        .lock()
                        .expect("handler reports")
                        .push((handler, seen.get()));
                }
            })
        }
    });
    let runtimes: Vec<UiRuntime> = (0..2)
        .map(|_| {
            let runtime = UiRuntime::try_new(
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
            factory.install_on(&runtime);
            runtime
        })
        .collect();
    assert_eq!(
        built.load(std::sync::atomic::Ordering::Relaxed),
        2,
        "the factory builds one handler per runtime it is installed on"
    );

    clock.advance(Duration::from_millis(16));
    for mut runtime in runtimes {
        runtime
            .attach_root_widget_with_size(&PanicsInBuild::new(), 800.0, 600.0)
            .expect("root");
        let _ = runtime.pump(
            &mut flui_runtime::pump::SampledClock(clock.now()),
            &mut DiscardSink,
        );
    }

    let mut reports = reports.lock().expect("handler reports").clone();
    reports.sort_unstable();
    assert_eq!(
        reports,
        vec![(0, 1), (1, 1)],
        "each runtime's report reaches its own handler, whose count starts from zero"
    );
}
