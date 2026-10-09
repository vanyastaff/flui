//! Authored text failures retain delivery without continuously requesting frames.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use flui_view::prelude::*;

#[derive(Clone, StatefulView)]
struct AuthoredText {
    font_size: Arc<AtomicU64>,
    rebuild: Arc<parking_lot::Mutex<Option<flui_view::RebuildHandle>>>,
}

struct AuthoredTextState(Arc<parking_lot::Mutex<Option<flui_view::RebuildHandle>>>);

impl StatefulView for AuthoredText {
    type State = AuthoredTextState;

    fn create_state(&self) -> Self::State {
        AuthoredTextState(Arc::clone(&self.rebuild))
    }
}

impl ViewState<AuthoredText> for AuthoredTextState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.0.lock() = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &AuthoredText, _: &dyn BuildContext) -> impl IntoView {
        flui_widgets::Text::new("measured").style(flui_painting::typography::TextStyle {
            font_size: Some(f64::from_bits(view.font_size.load(Ordering::SeqCst))),
            ..Default::default()
        })
    }
}

#[derive(Default)]
struct Sink(usize);

impl flui_runtime::sink::FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
        self.0 += 1;
        flui_runtime::sink::SubmitVerdict::Presented
    }
}

#[derive(Clone, Copy)]
enum Recovery {
    Rebuild,
    AnimationThenRebuild,
    FailureHandler,
}

fn rejected_text_recovers(recovery: Recovery) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&wakes);
    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let failures = Arc::new(AtomicUsize::new(0));
    let font_size = Arc::new(AtomicU64::new(f64::MAX.to_bits()));
    let rebuild = Arc::new(parking_lot::Mutex::new(None));
    runtime
        .attach_root_widget_with_size(
            &AuthoredText {
                font_size: Arc::clone(&font_size),
                rebuild: Arc::clone(&rebuild),
            },
            400.0,
            400.0,
        )
        .expect("mount authored text");
    let reports = Arc::clone(&failures);
    let corrected_size = Arc::clone(&font_size);
    let correction_rebuild = Arc::clone(&rebuild);
    runtime.set_frame_failure_handler(Some(flui_runtime::frame_failure::FrameFailureHandler::new(
        move |report| {
            assert!(matches!(
                &report.kind,
                flui_runtime::frame_failure::FrameFailureKind::Pipeline {
                    error: flui_rendering::RenderError::TextLayout(_),
                }
            ));
            reports.fetch_add(1, Ordering::SeqCst);
            if matches!(recovery, Recovery::FailureHandler) {
                corrected_size.store(16.0_f64.to_bits(), Ordering::SeqCst);
                let handle = correction_rebuild
                    .lock()
                    .as_ref()
                    .expect("mounted handle")
                    .clone();
                handle.schedule(flui_foundation::RebuildReason::StateChange);
            }
        },
    )));
    let mut sink = Sink::default();
    let mut now = web_time::Instant::now();
    let mut pump = |runtime: &mut flui_runtime::ui_runtime::UiRuntime, sink: &mut Sink| {
        now += std::time::Duration::from_millis(16);
        runtime.pump(&mut flui_runtime::pump::SampledClock(now), sink)
    };
    assert!(!pump(&mut runtime, &mut sink).presented());
    assert_eq!(failures.load(Ordering::SeqCst), 1);
    assert_eq!(sink.0, 0, "invalid text never reaches the sink");
    if matches!(recovery, Recovery::FailureHandler) {
        assert!(
            pump(&mut runtime, &mut sink).presented(),
            "diagnostic-time rebuild remains deliverable"
        );
        assert_eq!(sink.0, 1);
        assert_eq!(failures.load(Ordering::SeqCst), 1);
        return;
    }
    let controller = matches!(recovery, Recovery::AnimationThenRebuild).then(|| {
        let owner = flui_animation::AnimationController::builder(std::time::Duration::from_secs(1))
            .build_on(Some(&runtime.vsync()));
        owner.controller().forward().expect("fresh controller runs");
        owner
    });
    wakes.store(0, Ordering::SeqCst);
    for _ in 0..4 {
        assert!(!pump(&mut runtime, &mut sink).presented());
    }
    assert_eq!(
        failures.load(Ordering::SeqCst),
        1,
        "unchanged input is not retried"
    );
    if let Some(controller) = controller {
        use flui_animation::Animation as _;
        assert!(
            controller.controller().value() > 0.0,
            "independent animation still advances"
        );
    } else {
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            0,
            "rejected input does not keep waking the host"
        );
    }
    assert_eq!(sink.0, 0, "invalid text never reaches the sink");

    font_size.store(16.0_f64.to_bits(), Ordering::SeqCst);
    let handle = rebuild.lock().as_ref().expect("mounted handle").clone();
    handle.schedule(flui_foundation::RebuildReason::StateChange);
    assert!(
        pump(&mut runtime, &mut sink).presented(),
        "corrected input resumes retained layout"
    );
    assert_eq!(sink.0, 1);
    assert_eq!(failures.load(Ordering::SeqCst), 1);
}

fn rejected_text_parks_until_rebuild() {
    rejected_text_recovers(Recovery::Rebuild);
}

fn independent_animation_does_not_retry_unchanged_rejected_text() {
    rejected_text_recovers(Recovery::AnimationThenRebuild);
}

fn failure_handler_rebuild_recovers_the_retained_layout() {
    rejected_text_recovers(Recovery::FailureHandler);
}

#[test]
fn invalid_authored_text_waits_for_changed_input_and_then_presents() {
    crate::table_test::run_table(
        "invalid_authored_text_waits_for_changed_input_and_then_presents",
        &[
            (
                "rejected_text_parks_until_rebuild",
                rejected_text_parks_until_rebuild as fn(),
            ),
            (
                "independent_animation_does_not_retry_unchanged_rejected_text",
                independent_animation_does_not_retry_unchanged_rejected_text as fn(),
            ),
            (
                "failure_handler_rebuild_recovers_the_retained_layout",
                failure_handler_rebuild_recovers_the_retained_layout as fn(),
            ),
        ],
    );
}
