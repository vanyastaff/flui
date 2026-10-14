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

fn a_competing_text_loan_waits_for_an_explicit_input_change() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&wakes);
    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let failures = Arc::new(AtomicUsize::new(0));
    let reports = Arc::clone(&failures);
    runtime.set_frame_failure_handler(Some(flui_runtime::frame_failure::FrameFailureHandler::new(
        move |report| {
            assert!(matches!(
                &report.kind,
                flui_runtime::frame_failure::FrameFailureKind::Pipeline {
                    error: flui_rendering::RenderError::TextContextBusy,
                }
            ));
            reports.fetch_add(1, Ordering::SeqCst);
        },
    )));
    let font_size = Arc::new(AtomicU64::new(16.0_f64.to_bits()));
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
        .expect("mount real text before its competing measurement");
    let text = runtime.text_context_for_test().clone();
    let mut sink = Sink::default();
    let mut now = web_time::Instant::now();
    let mut pump = |runtime: &mut flui_runtime::ui_runtime::UiRuntime, sink: &mut Sink| {
        now += std::time::Duration::from_millis(16);
        runtime.pump(&mut flui_runtime::pump::SampledClock(now), sink)
    };
    text.with(|_| {
        assert!(!pump(&mut runtime, &mut sink).presented());
        assert_eq!(failures.load(Ordering::SeqCst), 1);
        assert_eq!(sink.0, 0, "busy measurement cannot submit text geometry");
        wakes.store(0, Ordering::SeqCst);
        for _ in 0..4 {
            assert!(!pump(&mut runtime, &mut sink).presented());
        }
        assert_eq!(
            failures.load(Ordering::SeqCst),
            1,
            "unchanged text must not hot-retry the competing loan"
        );
        assert_eq!(
            wakes.load(Ordering::SeqCst),
            0,
            "the refused measurement must not keep waking the host"
        );
    });
    assert!(
        !pump(&mut runtime, &mut sink).presented(),
        "releasing an unrelated loan does not authorize an automatic retry"
    );
    assert_eq!(failures.load(Ordering::SeqCst), 1);
    assert_eq!(sink.0, 0);

    font_size.store(18.0_f64.to_bits(), Ordering::SeqCst);
    let handle = rebuild
        .lock()
        .as_ref()
        .expect("mounted lifecycle handle")
        .clone();
    handle.schedule(flui_foundation::RebuildReason::StateChange);
    assert!(
        pump(&mut runtime, &mut sink).presented(),
        "an explicit changed-input rebuild resumes retained text layout"
    );
    assert_eq!(sink.0, 1);
    assert_eq!(failures.load(Ordering::SeqCst), 1);
}

#[derive(Clone, StatefulView)]
struct TextFrameCompletion {
    sizing: std::rc::Rc<std::cell::RefCell<flui_painting::TextSizing>>,
    completed: Arc<AtomicUsize>,
    rebuild: Arc<parking_lot::Mutex<Option<flui_view::RebuildHandle>>>,
}

struct TextFrameCompletionState {
    completed: Arc<AtomicUsize>,
    rebuild: Arc<parking_lot::Mutex<Option<flui_view::RebuildHandle>>>,
}

impl StatefulView for TextFrameCompletion {
    type State = TextFrameCompletionState;

    fn create_state(&self) -> Self::State {
        TextFrameCompletionState {
            completed: Arc::clone(&self.completed),
            rebuild: Arc::clone(&self.rebuild),
        }
    }
}

impl ViewState<TextFrameCompletion> for TextFrameCompletionState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.rebuild.lock() = Some(ctx.rebuild_handle());
        let completed = Arc::clone(&self.completed);
        ctx.post_frame_handle()
            .expect("runtime installs the lifecycle post-frame capability")
            .schedule(move |_| {
                completed.fetch_add(1, Ordering::SeqCst);
            })
            .expect("mounted presentation accepts its completion callback");
    }

    fn build(&self, view: &TextFrameCompletion, _: &dyn BuildContext) -> impl IntoView {
        flui_widgets::MediaQuery::new(
            flui_widgets::MediaQueryData {
                text_sizing: view.sizing.borrow().clone(),
                ..Default::default()
            },
            flui_widgets::Text::new("awaiting captured text sizes").style(
                flui_painting::typography::TextStyle {
                    font_size: Some(16.0),
                    ..Default::default()
                },
            ),
        )
    }
}

fn text_frame_completion_waits_for_measurement(pending: bool) {
    let completed = Arc::new(AtomicUsize::new(0));
    let preparation_reports = Arc::new(AtomicUsize::new(0));
    let mut runtime = crate::owner_publication::runtime();
    let reports = Arc::clone(&preparation_reports);
    runtime.set_frame_failure_handler(Some(flui_runtime::frame_failure::FrameFailureHandler::new(
        move |report| {
            assert!(matches!(
                &report.kind,
                flui_runtime::frame_failure::FrameFailureKind::Pipeline {
                    error: flui_rendering::RenderError::TextPreparationPending(requests),
                } if !requests.requests.is_empty()
            ));
            reports.fetch_add(1, Ordering::SeqCst);
        },
    )));
    runtime
        .attach_root_widget_with_size(
            &TextFrameCompletion {
                sizing: std::rc::Rc::new(std::cell::RefCell::new(if pending {
                    flui_painting::TextSizing::exact([]).expect("sealed empty capture")
                } else {
                    flui_painting::TextSizing::fixed()
                })),
                completed: Arc::clone(&completed),
                rebuild: Arc::new(parking_lot::Mutex::new(None)),
            },
            400.0,
            400.0,
        )
        .expect("mount text with lifecycle completion callback");
    let mut sink = Sink::default();
    let outcome = runtime.pump(
        &mut flui_runtime::pump::SampledClock(
            web_time::Instant::now() + std::time::Duration::from_millis(16),
        ),
        &mut sink,
    );
    assert_eq!(outcome.presented(), !pending);
    assert_eq!(sink.0, usize::from(!pending));
    assert_eq!(
        preparation_reports.load(Ordering::SeqCst),
        usize::from(pending)
    );
    assert_eq!(
        completed.load(Ordering::SeqCst),
        usize::from(!pending),
        "a presentation callback cannot claim completion while native text preparation is pending"
    );
}

fn measured_text_delivers_its_lifecycle_completion_callback() {
    text_frame_completion_waits_for_measurement(false);
}

fn missing_captured_sizes_withhold_the_lifecycle_completion_callback() {
    text_frame_completion_waits_for_measurement(true);
}

fn an_unregistered_capture_parks_until_its_provider_changes() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&wakes);
    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let completed = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(AtomicUsize::new(0));
    let reports = Arc::clone(&failures);
    runtime.set_frame_failure_handler(Some(flui_runtime::frame_failure::FrameFailureHandler::new(
        move |report| {
            assert!(matches!(
                &report.kind,
                flui_runtime::frame_failure::FrameFailureKind::Pipeline {
                    error: flui_rendering::RenderError::TextPreparationPending(requests),
                } if !requests.requests.is_empty()
            ));
            reports.fetch_add(1, Ordering::SeqCst);
        },
    )));
    // No App resolver owns this separately sealed provider capture.
    let sizing = std::rc::Rc::new(std::cell::RefCell::new(
        flui_painting::TextSizing::exact([]).expect("sealed unregistered capture"),
    ));
    let rebuild = Arc::new(parking_lot::Mutex::new(None));
    runtime
        .attach_root_widget_with_size(
            &TextFrameCompletion {
                sizing: std::rc::Rc::clone(&sizing),
                completed: Arc::clone(&completed),
                rebuild: Arc::clone(&rebuild),
            },
            400.0,
            400.0,
        )
        .expect("mount explicit provider and lifecycle completion callback");
    let mut sink = Sink::default();
    let mut now = web_time::Instant::now();
    let mut pump = |runtime: &mut flui_runtime::ui_runtime::UiRuntime, sink: &mut Sink| {
        now += std::time::Duration::from_millis(16);
        runtime.pump(&mut flui_runtime::pump::SampledClock(now), sink)
    };
    assert!(!pump(&mut runtime, &mut sink).presented());
    assert_eq!(failures.load(Ordering::SeqCst), 1);
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    wakes.store(0, Ordering::SeqCst);
    for _ in 0..4 {
        assert!(!pump(&mut runtime, &mut sink).presented());
    }
    assert_eq!(
        failures.load(Ordering::SeqCst),
        1,
        "an unavailable capture cannot progress by repeating unchanged preparation"
    );
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
    assert_eq!(sink.0, 0);
    assert_eq!(completed.load(Ordering::SeqCst), 0);

    *sizing.borrow_mut() = flui_painting::TextSizing::fixed();
    let handle = rebuild
        .lock()
        .as_ref()
        .expect("mounted lifecycle rebuild handle")
        .clone();
    handle.schedule(flui_foundation::RebuildReason::StateChange);
    assert!(pump(&mut runtime, &mut sink).presented());
    assert_eq!(sink.0, 1);
    assert_eq!(failures.load(Ordering::SeqCst), 1);
    assert_eq!(
        completed.load(Ordering::SeqCst),
        1,
        "the original completion debt runs once after repaired geometry"
    );
    let _ = pump(&mut runtime, &mut sink);
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    assert_eq!(failures.load(Ordering::SeqCst), 1);
}

#[derive(Clone, StatefulView)]
struct CompletionScopeProbe {
    handle: std::rc::Rc<std::cell::RefCell<Option<flui_scheduler::PostFrameHandle>>>,
    builds: Arc<AtomicUsize>,
}

struct CompletionScopeProbeState(
    std::rc::Rc<std::cell::RefCell<Option<flui_scheduler::PostFrameHandle>>>,
);

impl StatefulView for CompletionScopeProbe {
    type State = CompletionScopeProbeState;

    fn create_state(&self) -> Self::State {
        CompletionScopeProbeState(std::rc::Rc::clone(&self.handle))
    }
}

impl ViewState<CompletionScopeProbe> for CompletionScopeProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.0.borrow_mut() = ctx.post_frame_handle();
    }

    fn build(&self, view: &CompletionScopeProbe, _: &dyn BuildContext) -> impl IntoView {
        view.builds.fetch_add(1, Ordering::SeqCst);
        flui_widgets::Text::new("complete geometry stays clean")
    }
}

fn a_clean_presentation_delivers_successor_callbacks_without_rebuilding() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let wakes = Arc::new(AtomicUsize::new(0));
    let wake = Arc::clone(&wakes);
    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
        wake.fetch_add(1, Ordering::SeqCst);
    }));
    let handle = Rc::new(RefCell::new(None));
    let builds = Arc::new(AtomicUsize::new(0));
    runtime
        .attach_root_widget_with_size(
            &CompletionScopeProbe {
                handle: Rc::clone(&handle),
                builds: Arc::clone(&builds),
            },
            400.0,
            400.0,
        )
        .expect("mount real text and retain its lifecycle capability");
    let mut sink = Sink::default();
    let mut now = web_time::Instant::now();
    let mut pump = |runtime: &mut flui_runtime::ui_runtime::UiRuntime, sink: &mut Sink| {
        now += std::time::Duration::from_millis(16);
        runtime.pump(&mut flui_runtime::pump::SampledClock(now), sink)
    };
    assert!(pump(&mut runtime, &mut sink).presented());
    let builds_before = builds.load(Ordering::SeqCst);
    let submits_before = sink.0;
    wakes.store(0, Ordering::SeqCst);
    let handle = handle
        .borrow()
        .as_ref()
        .expect("mounted callback handle")
        .clone();
    let completed = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&completed);
    handle
        .schedule(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
        })
        .expect("accept clean completion demand");
    assert!(
        wakes.load(Ordering::SeqCst) > 0,
        "an idle host must be woken"
    );
    assert!(
        runtime.has_pending_work(),
        "the installed driver's work gate must admit completion"
    );
    assert!(!pump(&mut runtime, &mut sink).presented());
    assert_eq!(completed.load(Ordering::SeqCst), 1);
    assert_eq!(builds.load(Ordering::SeqCst), builds_before);
    assert_eq!(sink.0, submits_before);
    assert!(!runtime.has_pending_work());
}

#[test]
fn clean_presentation_completion_keeps_the_tree_clean() {
    crate::table_test::run_table(
        "clean_presentation_completion_keeps_the_tree_clean",
        &[(
            "a_clean_presentation_delivers_successor_callbacks_without_rebuilding",
            a_clean_presentation_delivers_successor_callbacks_without_rebuilding as fn(),
        )],
    );
}

#[derive(Debug)]
struct ImeGeometryRead {
    caret: Result<
        flui_platform_api::text_store::RangeRect,
        flui_platform_api::text_store::TextStoreError,
    >,
    bounds: Result<
        flui_foundation::geometry::Bounds<f64>,
        flui_platform_api::text_store::TextStoreError,
    >,
    index: Result<
        flui_platform_api::text_store::Utf16Offset,
        flui_platform_api::text_store::TextStoreError,
    >,
    document_len: usize,
    edited: bool,
}

#[derive(Clone, StatefulView)]
struct PartialTextFrame {
    pending: std::rc::Rc<std::cell::Cell<bool>>,
    hit_test: std::rc::Rc<std::cell::RefCell<Option<flui_interaction::HitTestHandle>>>,
    pending_sizing: flui_painting::TextSizing,
    controller: flui_widgets::TextEditingController,
    focus: std::rc::Rc<flui_interaction::focus::FocusNode>,
    rebuild: std::rc::Rc<std::cell::RefCell<Option<flui_view::RebuildHandle>>>,
    store: std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn flui_platform_api::TextStore>>>>,
    grant: std::rc::Rc<std::cell::RefCell<Option<ImeGeometryRead>>>,
    lock_outcome: std::rc::Rc<
        std::cell::RefCell<
            Option<
                Result<
                    flui_platform_api::text_store::LockOutcome,
                    flui_platform_api::text_store::TextStoreError,
                >,
            >,
        >,
    >,
}

struct PartialTextFrameState(
    std::rc::Rc<std::cell::RefCell<Option<flui_view::RebuildHandle>>>,
    std::rc::Rc<std::cell::RefCell<Option<flui_interaction::HitTestHandle>>>,
);

impl StatefulView for PartialTextFrame {
    type State = PartialTextFrameState;

    fn create_state(&self) -> Self::State {
        PartialTextFrameState(
            std::rc::Rc::clone(&self.rebuild),
            std::rc::Rc::clone(&self.hit_test),
        )
    }
}

impl ViewState<PartialTextFrame> for PartialTextFrameState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.0.borrow_mut() = Some(ctx.rebuild_handle());
        *self.1.borrow_mut() = ctx.hit_test_handle();
    }

    fn build(&self, view: &PartialTextFrame, _: &dyn BuildContext) -> impl IntoView {
        use flui_platform_api::text_store::{
            LockGrant, LockTiming, PointMode, Utf16Offset, Utf16Range,
        };

        if view.pending.get() {
            let store = view
                .store
                .borrow()
                .clone()
                .expect("previously focused editable store");
            let grant = std::rc::Rc::clone(&view.grant);
            let outcome = store.request_lock(
                LockGrant::read_write(move |session| {
                    let caret = session.rect_for_range(Utf16Range::collapsed(Utf16Offset::new(4)));
                    let bounds = session.document_bounds();
                    let index = session.index_at_point(
                        flui_foundation::geometry::Point::new(0.0, 0.0),
                        PointMode::Nearest,
                    );
                    let document_len = session.document_len().get();
                    let edited = session.insert_at_selection("x").is_ok();
                    *grant.borrow_mut() = Some(ImeGeometryRead {
                        caret,
                        bounds,
                        index,
                        document_len,
                        edited,
                    });
                }),
                LockTiming::Async,
            );
            *view.lock_outcome.borrow_mut() = Some(outcome);
        }
        flui_widgets::MediaQuery::new(
            flui_widgets::MediaQueryData {
                text_sizing: if view.pending.get() {
                    view.pending_sizing.clone()
                } else {
                    flui_painting::TextSizing::fixed()
                },
                ..Default::default()
            },
            flui_widgets::Column::new(vec![
                flui_widgets::EditableText::new(
                    view.controller.clone(),
                    std::rc::Rc::clone(&view.focus),
                )
                .text_style(flui_painting::typography::TextStyle::default().with_font_size(16.0))
                .boxed(),
                flui_widgets::Text::new("later sibling has no captured answer")
                    .style(flui_painting::typography::TextStyle::default().with_font_size(20.0))
                    .boxed(),
            ]),
        )
    }
}

fn parked_partial_text_geometry_is_unavailable_to_the_real_ime_store() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, TextStoreError, Utf16Offset, Utf16Range,
    };

    let native_store_host = flui_testing::RecordingTextStoreHost::new();
    let mut runtime = flui_runtime::ui_runtime::UiRuntime::new(
        crate::owner_publication::window().with_text_store_host(Some(native_store_host.clone())),
        1.0,
        flui_runtime::ui_runtime::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("runtime with a pull-model text store host");
    let pending = Rc::new(Cell::new(false));
    let hit_test = Rc::new(RefCell::new(None));
    let rebuild = Rc::new(RefCell::new(None));
    let store = Rc::new(RefCell::new(None));
    let grant = Rc::new(RefCell::new(None));
    let lock_outcome = Rc::new(RefCell::new(None));
    let focus = flui_interaction::focus::FocusNode::new();
    runtime
        .attach_root_widget_with_size(
            &PartialTextFrame {
                pending: Rc::clone(&pending),
                hit_test: Rc::clone(&hit_test),
                pending_sizing: flui_painting::TextSizing::exact([(
                    TextSizeRequest {
                        size: TextSize::new(16.0).expect("authored editable size"),
                        profile: TextScaleProfile::Body,
                    },
                    TextSize::new(32.0).expect("resolved editable size"),
                )])
                .expect("sealed capture intentionally omits the later sibling"),
                controller: flui_widgets::TextEditingController::with_text("abcd"),
                focus: Rc::clone(&focus),
                rebuild: Rc::clone(&rebuild),
                store: Rc::clone(&store),
                grant: Rc::clone(&grant),
                lock_outcome: Rc::clone(&lock_outcome),
            },
            400.0,
            400.0,
        )
        .expect("mount ordered editable and sibling text");
    let mut sink = Sink::default();
    let mut now = web_time::Instant::now();
    let mut pump = |runtime: &mut flui_runtime::ui_runtime::UiRuntime, sink: &mut Sink| {
        now += std::time::Duration::from_millis(16);
        runtime.pump(&mut flui_runtime::pump::SampledClock(now), sink)
    };
    assert!(pump(&mut runtime, &mut sink).presented());
    let _ = focus.request_focus();
    assert!(pump(&mut runtime, &mut sink).presented());
    let focused = native_store_host
        .focused_store()
        .expect("actual mounted editable store");
    let hit_test = hit_test
        .borrow()
        .as_ref()
        .expect("actual lifecycle hit-test handle")
        .clone();
    let initial_hit =
        runtime.enter(|_| hit_test.hit_test_at(
            flui_foundation::geometry::Offset::new(1.0, 1.0),
            flui_platform_api::pointer::PointerKind::Touch,
        ));
    assert!(
        !initial_hit
            .expect("complete presentation exposes its hit-test geometry")
            .is_empty(),
        "the mounted editable contributes real hit-test targets"
    );
    let initial_caret = Rc::new(RefCell::new(None));
    let initial = Rc::clone(&initial_caret);
    assert_eq!(
        focused.request_lock(
            LockGrant::read(move |session| {
                *initial.borrow_mut() = Some(
                    session
                        .rect_for_range(Utf16Range::collapsed(Utf16Offset::new(4)))
                        .expect("committed initial caret geometry"),
                );
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );
    assert!(initial_caret.borrow().is_some());
    let fallback_rect = flui_foundation::geometry::Rect::from_ltwh(-1000.0, -1000.0, 7.0, 11.0);
    focus.set_rect(fallback_rect);
    let initial_focus_rect = runtime.enter(|_| focus.rect());
    assert_ne!(
        initial_focus_rect, fallback_rect,
        "a mounted focus provider exposes actual committed geometry"
    );
    assert!(initial_focus_rect.width() > 0.0 && initial_focus_rect.height() > 0.0);
    *store.borrow_mut() = Some(focused);
    let pending_reports = Arc::new(AtomicUsize::new(0));
    let reports = Arc::clone(&pending_reports);
    runtime.set_frame_failure_handler(Some(flui_runtime::frame_failure::FrameFailureHandler::new(
        move |report| {
            let flui_runtime::frame_failure::FrameFailureKind::Pipeline {
                error: flui_rendering::RenderError::TextPreparationPending(preparation),
            } = &report.kind
            else {
                panic!("expected the later sibling's missing captured size");
            };
            assert_eq!(
                preparation.requests,
                [TextSizeRequest {
                    size: TextSize::new(20.0).expect("authored sibling size"),
                    profile: TextScaleProfile::Body,
                }]
            );
            reports.fetch_add(1, Ordering::SeqCst);
        },
    )));
    pending.set(true);
    let handle = rebuild
        .borrow()
        .as_ref()
        .expect("lifecycle rebuild handle")
        .clone();
    handle.schedule(flui_foundation::RebuildReason::StateChange);
    let submitted = sink.0;
    assert!(!pump(&mut runtime, &mut sink).presented());
    assert_eq!(sink.0, submitted, "the partial frame is not submitted");
    assert_eq!(pending_reports.load(Ordering::SeqCst), 1);
    assert_eq!(*lock_outcome.borrow(), Some(Ok(LockOutcome::Deferred)));
    let observed = grant
        .borrow_mut()
        .take()
        .expect("parked document edits remain deliverable");
    assert_eq!(observed.document_len, 4);
    assert!(
        observed.edited,
        "IME can edit the document to repair parked content"
    );
    assert_eq!(
        observed.caret,
        Err(TextStoreError::NoLayout),
        "partial caret geometry must be unavailable: {observed:?}"
    );
    assert_eq!(observed.bounds, Err(TextStoreError::NoLayout));
    assert_eq!(observed.index, Err(TextStoreError::NoLayout));
    assert_eq!(
        runtime.enter(|_| focus.rect()),
        fallback_rect,
        "parked traversal must preserve the caller's fallback instead of reading partial layout"
    );
    let unavailable_hit =
        runtime.enter(|_| hit_test.hit_test_at(
            flui_foundation::geometry::Offset::new(1.0, 1.0),
            flui_platform_api::pointer::PointerKind::Touch,
        ));
    assert!(
        matches!(
            unavailable_hit,
            Err(flui_interaction::InteractionDispatchError::TreeBusy)
        ),
        "a lifecycle hit-test handle must refuse partially updated geometry while text is parked: {unavailable_hit:?}"
    );

    pending.set(false);
    handle.schedule(flui_foundation::RebuildReason::StateChange);
    assert!(pump(&mut runtime, &mut sink).presented());
    let recovered = Rc::new(RefCell::new(None));
    let restored_focus_rect = runtime.enter(|_| focus.rect());
    assert_ne!(
        restored_focus_rect, fallback_rect,
        "replacement completion restores actual focus geometry"
    );
    assert!(restored_focus_rect.width() > 0.0 && restored_focus_rect.height() > 0.0);
    let recovered_hit =
        runtime.enter(|_| hit_test.hit_test_at(
            flui_foundation::geometry::Offset::new(1.0, 1.0),
            flui_platform_api::pointer::PointerKind::Touch,
        ));
    assert!(
        !recovered_hit
            .expect("replacement layout restores hit-test geometry")
            .is_empty(),
        "repair restores the actual mounted target"
    );
    let result = Rc::clone(&recovered);
    let focused = store.borrow().as_ref().expect("live focused store").clone();
    assert_eq!(
        focused.request_lock(
            LockGrant::read(move |session| {
                assert_eq!(session.document_len().get(), 5);
                *result.borrow_mut() = Some(
                    session
                        .rect_for_range(Utf16Range::collapsed(Utf16Offset::new(5)))
                        .expect("the completed replacement publishes geometry"),
                );
                assert!(session.document_bounds().is_ok());
                assert!(
                    session
                        .index_at_point(
                            flui_foundation::geometry::Point::new(0.0, 0.0),
                            flui_platform_api::text_store::PointMode::Nearest,
                        )
                        .is_ok()
                );
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );
    let initial_height = initial_caret
        .borrow()
        .as_ref()
        .expect("initial caret")
        .bounds
        .size
        .height;
    let recovered_height = recovered
        .borrow()
        .as_ref()
        .expect("recovered caret")
        .bounds
        .size
        .height;
    assert!((initial_height - recovered_height).abs() < 0.001);
}

#[derive(Default)]
struct PreparedTextSink {
    submitted: usize,
    glyph_sizes: Vec<(String, Vec<f64>)>,
}

impl flui_runtime::sink::FrameSink for PreparedTextSink {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(&mut self, scene: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
        let mut registry = flui_painting::glyphs::FontRegistry::new();
        self.glyph_sizes.clear();
        for (_, node) in scene.tree().iter() {
            let flui_layer::Layer::Picture(picture) = node.layer() else {
                continue;
            };
            for command in picture.picture() {
                let flui_painting::DrawOp::Paragraph { paragraph, .. } = &command.op else {
                    continue;
                };
                let mut sizes = Vec::new();
                for run in paragraph.runs() {
                    let key = registry.prepare_run(&run).expect("submitted portable font");
                    for glyph in run.placed_glyphs(key, (0.0, 0.0), 1.0) {
                        assert_ne!(glyph.key.glyph_id(), 0, "the text must paint actual glyphs");
                        sizes.push(f64::from(glyph.key.size()));
                    }
                }
                self.glyph_sizes.push((paragraph.text().to_owned(), sizes));
            }
        }
        self.submitted += 1;
        flui_runtime::sink::SubmitVerdict::Presented
    }
}

impl PreparedTextSink {
    fn assert_text_size(&self, text: &str, expected: f64) {
        let sizes = &self
            .glyph_sizes
            .iter()
            .find(|(painted, _)| painted == text)
            .expect("the intended text reaches the actual submitted scene")
            .1;
        assert_ne!(sizes.as_slice(), [] as [f64; 0]);
        assert!(
            sizes.iter().all(|size| (*size - expected).abs() < 0.001),
            "submitted {text:?}: wanted {expected}, got {sizes:?}"
        );
    }
}

#[derive(Clone, StatefulView)]
struct PreparedTextProbe {
    child: BoxedView,
    rebuild: std::rc::Rc<std::cell::RefCell<Option<flui_view::RebuildHandle>>>,
    post_frame: std::rc::Rc<std::cell::RefCell<Option<flui_scheduler::PostFrameHandle>>>,
}

struct PreparedTextProbeState {
    rebuild: std::rc::Rc<std::cell::RefCell<Option<flui_view::RebuildHandle>>>,
    post_frame: std::rc::Rc<std::cell::RefCell<Option<flui_scheduler::PostFrameHandle>>>,
}

impl StatefulView for PreparedTextProbe {
    type State = PreparedTextProbeState;

    fn create_state(&self) -> Self::State {
        PreparedTextProbeState {
            rebuild: std::rc::Rc::clone(&self.rebuild),
            post_frame: std::rc::Rc::clone(&self.post_frame),
        }
    }
}

impl ViewState<PreparedTextProbe> for PreparedTextProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
        *self.post_frame.borrow_mut() = ctx.post_frame_handle();
    }

    fn build(&self, view: &PreparedTextProbe, _: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

fn prepared_runtime(
    clock: &flui_foundation::ManualClock,
) -> (
    flui_runtime::ui_runtime::UiRuntime,
    std::rc::Rc<flui_testing::RecordingTextStoreHost>,
) {
    let host = flui_testing::RecordingTextStoreHost::new();
    let runtime = flui_runtime::ui_runtime::UiRuntime::new(
        crate::owner_publication::window().with_text_store_host(Some(host.clone())),
        1.0,
        flui_runtime::ui_runtime::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime with actual mounted text-store ingress");
    (runtime, host)
}

fn answer_prepared_frontier(
    work: &flui_runtime::ui_runtime::TextSizingWork,
    writer: &mut flui_painting::TextSizingAdmission,
) {
    assert_eq!(work.source(), &writer.source());
    assert_ne!(work.requests(), []);
    writer
        .admit(work.requests().iter().map(|request| {
            let factor = match request.profile {
                flui_foundation::TextScaleProfile::Body => 1.5,
                flui_foundation::TextScaleProfile::Subheadline => 2.0,
                other => panic!("unexpected real producer profile: {other:?}"),
            };
            (
                *request,
                flui_foundation::TextSize::new(request.size.value() * factor)
                    .expect("finite fixture answer"),
            )
        }))
        .expect("host admits answers only after taking actual runtime work");
}

fn finish_prepared_text(
    stage: &str,
    runtime: &mut flui_runtime::ui_runtime::UiRuntime,
    clock: &mut flui_foundation::ManualClock,
    sink: &mut PreparedTextSink,
    writer: &mut flui_painting::TextSizingAdmission,
) {
    for attempt in 0..8 {
        let before = sink.submitted;
        let outcome = runtime.pump(clock, sink);
        if outcome.presented() {
            return;
        }
        let work = runtime.take_text_sizing_frontiers();
        assert!(
            !work.is_empty(),
            "{stage}, attempt {attempt}: no presented scene or registered frontier; outcome={outcome:?}, submitted={before}->{}, runtime_pending={}",
            sink.submitted,
            runtime.has_pending_work()
        );
        for frontier in work {
            assert_eq!(frontier.presentation(), runtime.presentation_id());
            answer_prepared_frontier(&frontier, writer);
            assert!(runtime.settle_text_sizing(
                frontier,
                flui_runtime::ui_runtime::TextSizingSettlement::Ready
            ));
        }
    }
    panic!("{stage}: finite fixture text did not converge");
}

fn an_animated_size_resumes_its_original_tick_after_host_service_yields() {
    animated_preparation_retains_its_tick(false);
}

fn deadline_delivery_does_not_discard_the_retained_animation_tick() {
    animated_preparation_retains_its_tick(true);
}

fn animated_preparation_retains_its_tick(failing_deadline: bool) {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use flui_animation::Animation as _;
    use flui_foundation::{TextScaleProfile, TextSizingIntent};
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, Utf16Offset, Utf16Range,
    };
    use flui_runtime::frame_failure::{FrameFailureKind, SamplingFailureOrigin, SamplingStage};
    use flui_runtime::ui_runtime::TextSizingSettlement;

    let mut clock = flui_foundation::ManualClock::new();
    let (mut runtime, host) = prepared_runtime(&clock);
    let (_policy, mut writer) = flui_painting::TextSizing::captured();
    writer.set_warm_capacity(1);
    let source = writer.source();
    assert!(runtime.install_captured_text_sizing_for(runtime.presentation_id(), source.clone()));
    let owner = flui_animation::AnimationController::builder(Duration::from_millis(100))
        .build_on(Some(&runtime.vsync()));
    let animation = owner.controller().clone();
    let samples = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&samples);
    let sampled = animation.clone();
    let focus = flui_interaction::focus::FocusNode::new();
    let field_focus = Rc::clone(&focus);
    let controller = flui_widgets::TextEditingController::with_text("abcd");
    let post_frame = Rc::new(RefCell::new(None));
    let source_policy = source.policy();
    let child = flui_widgets::AnimatedBuilder::new(Rc::new(animation.clone()), move || {
        let value = sampled.value();
        recorded.borrow_mut().push(value);
        flui_widgets::MediaQuery::new(
            flui_widgets::MediaQueryData {
                text_sizing: source_policy.clone(),
                ..Default::default()
            },
            flui_widgets::Column::new(vec![
                flui_widgets::EditableText::new(controller.clone(), Rc::clone(&field_focus))
                    .text_style(
                        flui_painting::typography::TextStyle::default()
                            .with_font_size(16.0 + 8.0 * value),
                    )
                    .boxed(),
                flui_widgets::Text::new("role")
                    .style(
                        flui_painting::typography::TextStyle::default()
                            .with_font_size(14.0 + 4.0 * value)
                            .with_sizing(TextSizingIntent::Profile(TextScaleProfile::Subheadline)),
                    )
                    .boxed(),
            ]),
        )
    });
    runtime
        .attach_root_widget_with_size(
            &PreparedTextProbe {
                child: child.boxed(),
                rebuild: Rc::new(RefCell::new(None)),
                post_frame: Rc::clone(&post_frame),
            },
            400.0,
            400.0,
        )
        .expect("mount actual animated editable and mixed role text");
    let mut sink = PreparedTextSink::default();
    finish_prepared_text(
        "initial mount",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    let _ = focus.request_focus();
    finish_prepared_text(
        "focus acquisition",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    let store = host.focused_store().expect("actual focused editable store");
    let baseline = Rc::new(RefCell::new(None));
    let baseline_read = Rc::clone(&baseline);
    assert_eq!(
        store.request_lock(
            LockGrant::read(move |session| {
                *baseline_read.borrow_mut() = Some(
                    session
                        .rect_for_range(Utf16Range::collapsed(Utf16Offset::new(4)))
                        .expect("committed baseline caret"),
                );
            }),
            LockTiming::Sync
        ),
        Ok(LockOutcome::Granted)
    );
    animation
        .forward()
        .expect("start the real presentation-bound animation");
    let _ = runtime.pump(&mut clock, &mut sink);
    assert_eq!(
        animation.value(),
        0.0,
        "forward anchors at the current time"
    );
    let entries_before = samples.borrow().len();
    let scenes_before = sink.submitted;
    let completed = Rc::new(Cell::new(0));
    let completed_read = Rc::clone(&completed);
    let caret = Rc::new(RefCell::new(None));
    let caret_read = Rc::clone(&caret);
    let lock_outcome = Rc::new(RefCell::new(None));
    let lock_read = Rc::clone(&lock_outcome);
    let completion = post_frame
        .borrow()
        .as_ref()
        .expect("lifecycle post-frame handle")
        .clone();
    completion
        .schedule(move |_| {
            completed_read.set(completed_read.get() + 1);
            *lock_read.borrow_mut() = Some(store.request_lock(
                LockGrant::read(move |session| {
                    *caret_read.borrow_mut() = Some(
                        session
                            .rect_for_range(Utf16Range::collapsed(Utf16Offset::new(4)))
                            .expect("completed IME caret"),
                    );
                }),
                LockTiming::Async,
            ));
        })
        .expect("completion belongs to the pending presentation epoch");
    clock.advance(Duration::from_micros(31_250));
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    assert!((animation.value() - 0.3125).abs() < 0.000_001);
    assert_eq!(samples.borrow().len(), entries_before + 1);
    let mut pending = runtime.take_text_sizing_frontiers();
    assert_eq!(pending.len(), 1);
    let mut work = pending.pop().expect("actual fractional frontier");
    assert!(
        work.requests().iter().any(
            |request| request.size.value() == 18.5 && request.profile == TextScaleProfile::Body
        )
    );
    let deadline_calls = Rc::new(Cell::new(0));
    let reports = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(flui_runtime::frame_failure::FrameFailureHandler::new(
        move |report| {
            let sampling = match &report.kind {
                FrameFailureKind::SamplingPanic {
                    first,
                    blocking,
                    admitted_attempt,
                    ..
                } => Some((
                    (first.stage, first.origin),
                    blocking.map(|blocker| (blocker.stage, blocker.origin)),
                    *admitted_attempt,
                )),
                _ => None,
            };
            reported.lock().push((
                report.address,
                report.disposition,
                report.consecutive_failures,
                sampling,
            ));
        },
    )));
    let _deadline = failing_deadline.then(|| {
        use flui_interaction::GestureRecognizer as _;
        let calls = Rc::clone(&deadline_calls);
        let recognizer = flui_interaction::LongPressGestureRecognizer::builder(
            runtime.gestures().arena().clone(),
        )
        .settings(
            flui_interaction::gesture::GestureSettings::default()
                .with_long_press_timeout(Duration::from_millis(10)),
        )
        .on_long_press_start(move |_| {
            calls.set(calls.get() + 1);
            std::panic::panic_any("retained deadline delivery");
        })
        .build();
        let down = flui_interaction::events::make_down_event_for_id(
            flui_platform_api::PointerId::new(std::num::NonZeroU64::MIN),
            flui_foundation::geometry::Offset::new(10.0, 10.0),
            flui_platform_api::PointerKind::Touch,
        )
        .expect("finite retained contact");
        recognizer.add_pointer(flui_interaction::PointerDispatch::at_root(&down));
        recognizer
    });
    for _ in 0..2 {
        assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Waiting));
        clock.advance(Duration::from_millis(20));
        assert!(!runtime.pump(&mut clock, &mut sink).presented());
        assert_eq!(samples.borrow().len(), entries_before + 1);
        assert!((animation.value() - 0.3125).abs() < 0.000_001);
        assert_eq!(completed.get(), 0);
        assert!(caret.borrow().is_none());
        assert_eq!(sink.submitted, scenes_before);
        assert!(
            runtime.take_text_sizing_frontiers().is_empty(),
            "Waiting requires an explicit host readiness cue"
        );
        assert!(runtime.service_text_sizing_source_for(runtime.presentation_id(), &source));
        let mut frontier = runtime.take_text_sizing_frontiers();
        assert_eq!(frontier.len(), 1);
        work = frontier
            .pop()
            .expect("explicitly serviced fractional frontier");
    }
    if failing_deadline {
        assert_eq!(
            deadline_calls.get(),
            1,
            "committed activation is not replayed"
        );
        assert_eq!(
            reports.lock().as_slice(),
            &[(
                flui_foundation::PresentationAddress {
                    ui_runtime_id: runtime.id(),
                    presentation_id: runtime.presentation_id(),
                },
                flui_runtime::frame_failure::FailureDisposition::Contained,
                0,
                Some((
                    (
                        SamplingStage::GestureDeadline,
                        SamplingFailureOrigin::Delivery
                    ),
                    None,
                    true,
                )),
            )],
            "delivery cannot reject the compatible frozen text attempt"
        );
    } else {
        assert!(reports.lock().is_empty());
    }
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    finish_prepared_text(
        "fractional continuation completion",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    assert_eq!(
        samples.borrow().len(),
        entries_before + 1,
        "continuation does not enter widgets again"
    );
    assert!((animation.value() - 0.3125).abs() < 0.000_001);
    assert_eq!(sink.submitted, scenes_before + 1);
    sink.assert_text_size("abcd", 27.75);
    sink.assert_text_size("role", 30.5);
    assert_eq!(completed.get(), 1);
    assert_eq!(*lock_outcome.borrow(), Some(Ok(LockOutcome::Deferred)));
    let baseline_height = baseline
        .borrow()
        .as_ref()
        .expect("baseline")
        .bounds
        .size
        .height;
    let final_height = caret
        .borrow()
        .as_ref()
        .expect("actual final caret grant")
        .bounds
        .size
        .height;
    assert!(
        (final_height - baseline_height * 18.5 / 16.0).abs() < 0.002,
        "IME caret must follow the painted fractional size"
    );
}

#[derive(Clone, StatefulView)]
struct ReentrantPreparedText {
    size: std::rc::Rc<std::cell::Cell<f64>>,
    reschedule: std::rc::Rc<std::cell::Cell<bool>>,
    rebuild: std::rc::Rc<std::cell::RefCell<Option<flui_view::RebuildHandle>>>,
}

struct ReentrantPreparedTextState(
    std::rc::Rc<std::cell::RefCell<Option<flui_view::RebuildHandle>>>,
);

impl StatefulView for ReentrantPreparedText {
    type State = ReentrantPreparedTextState;

    fn create_state(&self) -> Self::State {
        ReentrantPreparedTextState(std::rc::Rc::clone(&self.rebuild))
    }
}

impl ViewState<ReentrantPreparedText> for ReentrantPreparedTextState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.0.borrow_mut() = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &ReentrantPreparedText, _: &dyn BuildContext) -> impl IntoView {
        if view.reschedule.get() {
            let handle = self
                .0
                .borrow()
                .as_ref()
                .expect("mounted lifecycle handle")
                .clone();
            handle.schedule(flui_foundation::RebuildReason::StateChange);
        }
        flui_widgets::Text::new("replacement")
            .style(flui_painting::typography::TextStyle::default().with_font_size(view.size.get()))
    }
}

fn a_same_queued_rebuild_admission_cancels_the_old_preparation_premise() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use flui_runtime::ui_runtime::TextSizingSettlement;

    let mut clock = flui_foundation::ManualClock::new();
    let (mut runtime, _) = prepared_runtime(&clock);
    let size = Rc::new(Cell::new(20.0));
    let reschedule = Rc::new(Cell::new(false));
    let rebuild = Rc::new(RefCell::new(None));
    runtime
        .attach_root_widget_with_size(
            &ReentrantPreparedText {
                size: Rc::clone(&size),
                reschedule: Rc::clone(&reschedule),
                rebuild: Rc::clone(&rebuild),
            },
            400.0,
            400.0,
        )
        .expect("mount real reentrant text producer");
    let mut sink = PreparedTextSink::default();
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    let scenes_before = sink.submitted;
    let (_policy, mut writer) = flui_painting::TextSizing::captured();
    assert!(runtime.install_captured_text_sizing_for(runtime.presentation_id(), writer.source()));
    reschedule.set(true);
    let handle = rebuild
        .borrow()
        .as_ref()
        .expect("lifecycle rebuild handle")
        .clone();
    handle.schedule(flui_foundation::RebuildReason::StateChange);
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    let mut frontiers = runtime.take_text_sizing_frontiers();
    assert_eq!(frontiers.len(), 1);
    let old = frontiers.pop().expect("original actual text frontier");
    assert!(
        old.requests()
            .iter()
            .all(|request| request.size.value() == 20.0)
    );
    answer_prepared_frontier(&old, &mut writer);

    // The real build's bounded reentry leaves this SAME handle queued. A new
    // accepted change must replace the saved premise even when the ID coalesces.
    reschedule.set(false);
    size.set(22.75);
    handle.schedule(flui_foundation::RebuildReason::StateChange);
    let _ = runtime.settle_text_sizing(old, TextSizingSettlement::Ready);
    assert!(
        !runtime.pump(&mut clock, &mut sink).presented(),
        "the old 20-pixel text cannot commit after a new admission"
    );
    assert_eq!(sink.submitted, scenes_before);
    let mut replacement = runtime.take_text_sizing_frontiers();
    assert_eq!(replacement.len(), 1);
    let work = replacement
        .pop()
        .expect("replacement authored size needs its own answer");
    assert!(
        work.requests()
            .iter()
            .all(|request| request.size.value() == 22.75)
    );
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    finish_prepared_text(
        "replacement continuation completion",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    assert_eq!(sink.submitted, scenes_before + 1);
    sink.assert_text_size("replacement", 34.125);
}

struct PendingEditable {
    clock: flui_foundation::ManualClock,
    runtime: flui_runtime::ui_runtime::UiRuntime,
    sink: PreparedTextSink,
    writer: flui_painting::TextSizingAdmission,
    store: std::rc::Rc<dyn flui_platform_api::TextStore>,
    completed: std::rc::Rc<std::cell::Cell<usize>>,
    work: flui_runtime::ui_runtime::TextSizingWork,
}

fn pending_editable() -> PendingEditable {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    let mut clock = flui_foundation::ManualClock::new();
    let (mut runtime, host) = prepared_runtime(&clock);
    let focus = flui_interaction::focus::FocusNode::new();
    let post_frame = Rc::new(RefCell::new(None));
    runtime
        .attach_root_widget_with_size(
            &PreparedTextProbe {
                child: flui_widgets::EditableText::new(
                    flui_widgets::TextEditingController::with_text("receipt"),
                    Rc::clone(&focus),
                )
                .text_style(flui_painting::typography::TextStyle::default().with_font_size(16.0))
                .boxed(),
                rebuild: Rc::new(RefCell::new(None)),
                post_frame: Rc::clone(&post_frame),
            },
            400.0,
            400.0,
        )
        .expect("mount real receipt field");
    let mut sink = PreparedTextSink::default();
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    let _ = focus.request_focus();
    let _ = runtime.pump(&mut clock, &mut sink);
    let store = host.focused_store().expect("actual receipt text store");
    let (_policy, writer) = flui_painting::TextSizing::captured();
    assert!(runtime.install_captured_text_sizing_for(runtime.presentation_id(), writer.source()));
    let completed = Rc::new(Cell::new(0));
    let observed = Rc::clone(&completed);
    let callback = post_frame
        .borrow()
        .as_ref()
        .expect("mounted completion handle")
        .clone();
    callback
        .schedule(move |_| observed.set(observed.get() + 1))
        .expect("pending completion");
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    let mut frontiers = runtime.take_text_sizing_frontiers();
    assert_eq!(frontiers.len(), 1);
    PendingEditable {
        clock,
        runtime,
        sink,
        writer,
        store,
        completed,
        work: frontiers.pop().expect("actual receipt frontier"),
    }
}

struct UnknownRetainedDeadline {
    due: flui_platform_api::EventTime,
    effects: std::rc::Rc<std::cell::Cell<usize>>,
    fail_query: bool,
}

impl flui_interaction::gesture::GestureArenaMember for UnknownRetainedDeadline {
    fn accept_gesture(&self, _: flui_platform_api::PointerId) {}

    fn reject_gesture(&self, _: flui_platform_api::PointerId) {}

    fn deadline(&self) -> Option<flui_platform_api::EventTime> {
        if self.fail_query {
            self.effects.set(self.effects.get() + 1);
            std::panic::panic_any("unknown retained deadline discovery");
        }
        Some(self.due)
    }

    fn poll_deadline(&self, _: flui_platform_api::EventTime) {
        self.effects.set(self.effects.get() + 1);
        std::panic::panic_any("unknown retained deadline progress");
    }
}

fn unknown_deadline_withdraws_the_old_receipt_and_recovers_fresh_geometry() {
    retained_deadline_failure_case(false, false);
}

fn a_background_deadline_withdraws_the_old_receipt_without_opening_the_barrier() {
    retained_deadline_failure_case(true, false);
}

fn a_background_query_failure_withdraws_the_old_receipt_without_opening_the_barrier() {
    retained_deadline_failure_case(true, true);
}

fn retained_deadline_failure_case(background: bool, fail_query: bool) {
    use flui_foundation::MonotonicClock as _;
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, TextStoreError, Utf16Offset, Utf16Range,
    };
    use flui_runtime::frame_failure::{
        FailureDisposition, FrameFailureHandler, FrameFailureKind, SamplingFailureOrigin,
        SamplingStage,
    };
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    let PendingEditable {
        mut clock,
        mut runtime,
        mut sink,
        mut writer,
        store,
        completed,
        work,
    } = pending_editable();
    let before = sink.submitted;
    if background {
        runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Paused);
        assert!(!runtime.scheduler().frames_enabled());
    }
    let reports = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        let sampling = match &report.kind {
            FrameFailureKind::SamplingPanic {
                first,
                blocking,
                admitted_attempt,
                ..
            } => Some((
                (first.stage, first.origin),
                blocking.map(|blocker| (blocker.stage, blocker.origin)),
                *admitted_attempt,
            )),
            _ => None,
        };
        reported.lock().push((
            report.address,
            report.disposition,
            report.consecutive_failures,
            sampling,
        ));
    })));
    let effects = Rc::new(Cell::new(0));
    let member = Rc::new(UnknownRetainedDeadline {
        due: crate::event_at(&clock, clock.now()),
        effects: Rc::clone(&effects),
        fail_query,
    });
    let _entry = runtime.gestures().arena().add(
        flui_platform_api::PointerId::new(std::num::NonZeroU64::new(99).expect("contact")),
        &member,
    );
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if background {
            runtime.pump_background();
        } else {
            let _ = runtime.pump(&mut clock, &mut sink);
        }
    }));
    assert_eq!(
        effects.get(),
        1,
        "the open extension was evaluated through the runtime owner turn"
    );
    attempt.expect("unknown deadline failure is presentation-local");
    assert_eq!(
        sink.submitted, before,
        "withdrawal cannot publish old geometry"
    );
    assert_eq!(completed.get(), 0);
    assert!(
        !runtime.settle_text_sizing(work, TextSizingSettlement::Ready),
        "late receipt cannot authorize the withdrawn attempt"
    );
    let premature = Rc::new(Cell::new(false));
    let prematurely_granted = Rc::clone(&premature);
    assert_eq!(
        store.request_lock(
            LockGrant::read(move |_| prematurely_granted.set(true)),
            LockTiming::Sync,
        ),
        Err(TextStoreError::SyncLockUnavailable),
        "withdrawal cannot open the document barrier before replacement"
    );
    assert!(!premature.get());
    let geometry = Rc::new(RefCell::new(None));
    let observed = Rc::clone(&geometry);
    assert_eq!(
        store.request_lock(
            LockGrant::read(move |session| {
                *observed.borrow_mut() =
                    Some(session.rect_for_range(Utf16Range::collapsed(Utf16Offset::new(7))));
            }),
            LockTiming::Async,
        ),
        Ok(LockOutcome::Deferred)
    );
    if background {
        runtime.pump_background();
        assert_eq!(effects.get(), 1, "the same failing revision stays parked");
        assert_eq!(sink.submitted, before);
        assert_eq!(completed.get(), 0);
        assert!(geometry.borrow().is_none());
        runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
    }
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    let stage = if fail_query {
        SamplingStage::DeadlineDiscovery
    } else {
        SamplingStage::GestureDeadline
    };
    assert_eq!(
        effects.get(),
        1,
        "unknown side effects cannot automatically replay"
    );
    assert!(
        geometry.borrow().is_none(),
        "replacement is still uncommitted"
    );
    let mut replacement = runtime.take_text_sizing_frontiers();
    assert_eq!(
        replacement.len(),
        1,
        "fresh layout is runnable without another input or synthetic root redraw"
    );
    let work = replacement.pop().expect("actual replacement frontier");
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    finish_prepared_text(
        "unknown deadline replacement",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    assert_eq!(effects.get(), 1, "same deadline revision stays quarantined");
    assert_eq!(sink.submitted, before + 1);
    sink.assert_text_size("receipt", 24.0);
    assert_eq!(
        completed.get(),
        1,
        "accepted completion survives withdrawal"
    );
    assert!(
        geometry
            .borrow()
            .as_ref()
            .expect("replacement grant")
            .is_ok()
    );
    assert_eq!(
        reports.lock().as_slice(),
        &[(
            flui_foundation::PresentationAddress {
                ui_runtime_id: runtime.id(),
                presentation_id: runtime.presentation_id(),
            },
            FailureDisposition::FrameDropped,
            1,
            Some((
                (stage, SamplingFailureOrigin::Unknown),
                Some((stage, SamplingFailureOrigin::Unknown)),
                true,
            )),
        )],
        "one unresolved operation rejects the retained attempt only once"
    );
}

fn a_dropped_receipt_retries_only_after_its_deadline_and_completes() {
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::time::Duration;

    let PendingEditable {
        mut clock,
        mut runtime,
        mut sink,
        mut writer,
        completed,
        work,
        ..
    } = pending_editable();
    let before = sink.submitted;
    drop(work);
    clock.advance(Duration::from_millis(99));
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    assert!(runtime.take_text_sizing_frontiers().is_empty());
    assert_eq!(completed.get(), 0);
    clock.advance(Duration::from_millis(1));
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    let mut retried = runtime.take_text_sizing_frontiers();
    assert_eq!(retried.len(), 1, "lost host ownership remains deliverable");
    let work = retried.pop().expect("deadline retry");
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    finish_prepared_text(
        "lost receipt recovery",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    assert_eq!(sink.submitted, before + 1);
    sink.assert_text_size("receipt", 24.0);
    assert_eq!(completed.get(), 1);
}

fn a_held_receipt_parks_geometry_but_a_late_answer_heals_it() {
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, TextStoreError, Utf16Offset, Utf16Range,
    };
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    let PendingEditable {
        mut clock,
        mut runtime,
        mut sink,
        mut writer,
        store,
        completed,
        work,
    } = pending_editable();
    let before = sink.submitted;
    let geometry = Rc::new(RefCell::new(None));
    let observed = Rc::clone(&geometry);
    assert_eq!(
        store.request_lock(
            LockGrant::read_write(move |session| {
                assert_eq!(
                    session.document_len().get(),
                    7,
                    "document remains usable when geometry parks"
                );
                *observed.borrow_mut() = Some((
                    session.rect_for_range(Utf16Range::collapsed(Utf16Offset::new(7))),
                    session.document_bounds(),
                ));
            }),
            LockTiming::Async
        ),
        Ok(LockOutcome::Deferred)
    );
    clock.advance(Duration::from_millis(100));
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    assert!(
        runtime.take_text_sizing_frontiers().is_empty(),
        "held receipt is not duplicated"
    );
    let parked = geometry.borrow();
    let (caret, bounds) = parked.as_ref().expect("parked document lock is granted");
    assert!(matches!(caret, Err(TextStoreError::NoLayout)));
    assert!(matches!(bounds, Err(TextStoreError::NoLayout)));
    drop(parked);
    assert_eq!(completed.get(), 0);
    assert_eq!(sink.submitted, before);
    answer_prepared_frontier(&work, &mut writer);
    assert!(
        runtime.settle_text_sizing(work, TextSizingSettlement::Ready),
        "late current receipt still heals"
    );
    finish_prepared_text(
        "late held receipt",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    assert_eq!(completed.get(), 1);
    sink.assert_text_size("receipt", 24.0);
    assert_eq!(
        store.request_lock(
            LockGrant::read(|session| {
                assert!(
                    session
                        .rect_for_range(Utf16Range::collapsed(Utf16Offset::new(7)))
                        .is_ok()
                );
                assert!(session.document_bounds().is_ok());
            }),
            LockTiming::Sync
        ),
        Ok(LockOutcome::Granted)
    );
}

fn repeated_lost_receipts_stop_retrying_until_explicit_service() {
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::time::Duration;

    let PendingEditable {
        mut clock,
        mut runtime,
        mut sink,
        mut writer,
        completed,
        work,
        ..
    } = pending_editable();
    let source = writer.source();
    let before = sink.submitted;
    drop(work);
    for _ in 1..8 {
        clock.advance(Duration::from_millis(100));
        assert!(!runtime.pump(&mut clock, &mut sink).presented());
        let retry = runtime.take_text_sizing_frontiers();
        assert_eq!(retry.len(), 1);
        drop(retry);
    }
    for _ in 0..4 {
        clock.advance(Duration::from_millis(100));
        assert!(!runtime.pump(&mut clock, &mut sink).presented());
        assert!(
            runtime.take_text_sizing_frontiers().is_empty(),
            "bounded lost ownership must not spin"
        );
    }
    assert_eq!(sink.submitted, before);
    assert_eq!(completed.get(), 0);
    assert!(runtime.service_text_sizing_source_for(runtime.presentation_id(), &source));
    let mut repaired = runtime.take_text_sizing_frontiers();
    assert_eq!(repaired.len(), 1);
    let work = repaired.pop().expect("explicit recovery of stalled source");
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    finish_prepared_text(
        "bounded lost receipts",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    sink.assert_text_size("receipt", 24.0);
    assert_eq!(completed.get(), 1);
}

fn a_background_answer_waits_for_resume_before_paint_and_completion() {
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use flui_scheduler::AppLifecycleState;

    let PendingEditable {
        mut clock,
        mut runtime,
        mut sink,
        mut writer,
        completed,
        work,
        ..
    } = pending_editable();
    let before = sink.submitted;
    runtime.update_host_lifecycle(AppLifecycleState::Paused);
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    for _ in 0..3 {
        runtime.pump_background();
    }
    assert_eq!(sink.submitted, before);
    assert_eq!(
        completed.get(),
        0,
        "background service does not complete presentation geometry"
    );
    runtime.update_host_lifecycle(AppLifecycleState::Resumed);
    finish_prepared_text(
        "resume prepared field",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    sink.assert_text_size("receipt", 24.0);
    assert_eq!(completed.get(), 1);
}

#[derive(Clone, StatefulView)]
struct LazyPreparedText {
    entries: std::rc::Rc<std::cell::Cell<usize>>,
    lazy_entries: Arc<AtomicUsize>,
}

struct LazyPreparedTextState;

impl StatefulView for LazyPreparedText {
    type State = LazyPreparedTextState;

    fn create_state(&self) -> Self::State {
        LazyPreparedTextState
    }
}

impl ViewState<LazyPreparedText> for LazyPreparedTextState {
    fn build(&self, view: &LazyPreparedText, _: &dyn BuildContext) -> impl IntoView {
        view.entries.set(view.entries.get() + 1);
        let lazy_entries = Arc::clone(&view.lazy_entries);
        flui_widgets::Column::new(vec![
            flui_widgets::Text::new("before lazy")
                .style(flui_painting::typography::TextStyle::default().with_font_size(18.5))
                .boxed(),
            flui_widgets::LayoutBuilder::new(move |_, _| {
                lazy_entries.fetch_add(1, Ordering::SeqCst);
                flui_widgets::Text::new("lazy child")
                    .style(flui_painting::typography::TextStyle::default().with_font_size(27.5))
            })
            .boxed(),
        ])
    }
}

fn a_lazy_child_adds_a_frontier_without_reentering_the_root_or_partial_paint() {
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::cell::Cell;
    use std::rc::Rc;

    let mut clock = flui_foundation::ManualClock::new();
    let (mut runtime, _) = prepared_runtime(&clock);
    let (_policy, mut writer) = flui_painting::TextSizing::captured();
    writer.set_warm_capacity(0);
    assert!(runtime.install_captured_text_sizing_for(runtime.presentation_id(), writer.source()));
    let entries = Rc::new(Cell::new(0));
    let lazy_entries = Arc::new(AtomicUsize::new(0));
    runtime
        .attach_root_widget_with_size(
            &LazyPreparedText {
                entries: Rc::clone(&entries),
                lazy_entries: Arc::clone(&lazy_entries),
            },
            400.0,
            400.0,
        )
        .expect("mount actual layout-built text");
    let mut sink = PreparedTextSink::default();
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    assert_eq!(entries.get(), 1);
    assert_eq!(sink.submitted, 0);
    let mut frontiers = runtime.take_text_sizing_frontiers();
    assert_eq!(frontiers.len(), 1);
    let first = frontiers.pop().expect("first real measurement frontier");
    assert!(
        first
            .requests()
            .iter()
            .any(|request| request.size.value() == 18.5)
    );
    assert!(
        !first
            .requests()
            .iter()
            .any(|request| request.size.value() == 27.5),
        "lazy text is not fabricated before its builder runs"
    );
    answer_prepared_frontier(&first, &mut writer);
    assert!(runtime.settle_text_sizing(first, TextSizingSettlement::Ready));
    assert!(!runtime.pump(&mut clock, &mut sink).presented());
    assert_eq!(
        sink.submitted, 0,
        "known first text does not publish a partial scene"
    );
    assert_eq!(entries.get(), 1);
    assert_eq!(lazy_entries.load(Ordering::SeqCst), 1);
    let mut frontiers = runtime.take_text_sizing_frontiers();
    assert_eq!(frontiers.len(), 1);
    let second = frontiers
        .pop()
        .expect("actual newly mounted lazy text frontier");
    assert!(
        second
            .requests()
            .iter()
            .any(|request| request.size.value() == 27.5)
    );
    answer_prepared_frontier(&second, &mut writer);
    assert!(runtime.settle_text_sizing(second, TextSizingSettlement::Ready));
    finish_prepared_text(
        "lazy frontier completion",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    assert_eq!(entries.get(), 1);
    assert_eq!(lazy_entries.load(Ordering::SeqCst), 1);
    assert_eq!(sink.submitted, 1);
    sink.assert_text_size("before lazy", 27.75);
    sink.assert_text_size("lazy child", 41.25);
}

#[derive(Clone, StatefulView)]
struct PreparedSibling {
    animation: std::rc::Rc<std::cell::RefCell<Option<flui_animation::AnimationController>>>,
    post_frame: std::rc::Rc<std::cell::RefCell<Option<flui_scheduler::PostFrameHandle>>>,
    focus: std::rc::Rc<flui_interaction::focus::FocusNode>,
}

struct PreparedSiblingState {
    animation: std::rc::Rc<std::cell::RefCell<Option<flui_animation::AnimationController>>>,
    post_frame: std::rc::Rc<std::cell::RefCell<Option<flui_scheduler::PostFrameHandle>>>,
    focus: std::rc::Rc<flui_interaction::focus::FocusNode>,
    controller: flui_widgets::TextEditingController,
    driven: Option<flui_animation::DrivenController>,
}

impl StatefulView for PreparedSibling {
    type State = PreparedSiblingState;

    fn create_state(&self) -> Self::State {
        PreparedSiblingState {
            animation: std::rc::Rc::clone(&self.animation),
            post_frame: std::rc::Rc::clone(&self.post_frame),
            focus: std::rc::Rc::clone(&self.focus),
            controller: flui_widgets::TextEditingController::with_text("sibling"),
            driven: None,
        }
    }
}

impl ViewState<PreparedSibling> for PreparedSiblingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let vsync =
            flui_widgets::VsyncScope::maybe_of(ctx).expect("sibling's actual inherited registry");
        let driven =
            flui_animation::AnimationController::builder(std::time::Duration::from_millis(100))
                .build_on(Some(&vsync));
        *self.animation.borrow_mut() = Some(driven.controller().clone());
        self.driven = Some(driven);
        *self.post_frame.borrow_mut() = ctx.post_frame_handle();
    }

    fn build(&self, _: &PreparedSibling, _: &dyn BuildContext) -> impl IntoView {
        use flui_animation::Animation as _;
        let animation = self
            .driven
            .as_ref()
            .expect("lifecycle owns sibling animation")
            .controller()
            .clone();
        let sampled = animation.clone();
        let focus = std::rc::Rc::clone(&self.focus);
        let controller = self.controller.clone();
        flui_widgets::AnimatedBuilder::new(std::rc::Rc::new(animation), move || {
            flui_widgets::EditableText::new(controller.clone(), std::rc::Rc::clone(&focus))
                .text_style(
                    flui_painting::typography::TextStyle::default()
                        .with_font_size(20.0 + 10.0 * sampled.value()),
                )
        })
    }
}

fn a_sibling_animation_admission_replaces_ready_text_before_geometry_publication() {
    use flui_foundation::Listenable as _;
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    let mut clock = flui_foundation::ManualClock::new();
    let (mut runtime, _) = prepared_runtime(&clock);
    let size = Rc::new(Cell::new(20.0));
    let rebuild = Rc::new(RefCell::new(None));
    let post_frame = Rc::new(RefCell::new(None));
    runtime
        .attach_root_widget_with_size(
            &PreparedTextProbe {
                child: ReentrantPreparedText {
                    size: Rc::clone(&size),
                    reschedule: Rc::new(Cell::new(false)),
                    rebuild: Rc::clone(&rebuild),
                }
                .boxed(),
                rebuild: Rc::new(RefCell::new(None)),
                post_frame: Rc::clone(&post_frame),
            },
            400.0,
            400.0,
        )
        .expect("mount target text with its lifecycle rebuild handle");
    let mut sink = PreparedTextSink::default();
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    let sibling = runtime.assemble_presentation(crate::owner_publication::window());
    let id = runtime.install_presentation(sibling);
    let animation_slot = Rc::new(RefCell::new(None));
    runtime
        .attach_root_widget_with_size_to(
            id,
            &PreparedSibling {
                animation: Rc::clone(&animation_slot),
                post_frame: Rc::new(RefCell::new(None)),
                focus: flui_interaction::focus::FocusNode::new(),
            },
            400.0,
            400.0,
        )
        .expect("mount sibling with its actual presentation animation registry");
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    let animation = animation_slot
        .borrow()
        .as_ref()
        .expect("sibling lifecycle owns its animation")
        .clone();
    animation.forward().expect("start sibling animation");
    let _ = runtime.pump(&mut clock, &mut sink);

    let (_, mut writer) = flui_painting::TextSizing::captured();
    let completed = Rc::new(Cell::new(0));
    let completion = Rc::clone(&completed);
    post_frame
        .borrow()
        .as_ref()
        .expect("target completion handle")
        .schedule(move |_| {
            completion.set(completion.get() + 1);
        })
        .expect("completion admitted before original pending geometry");
    assert!(runtime.install_captured_text_sizing_for(runtime.presentation_id(), writer.source()));
    let _ = runtime.pump(&mut clock, &mut sink);
    let mut pending = runtime.take_text_sizing_frontiers();
    assert_eq!(pending.len(), 1);
    let ready = pending.pop().expect("target's original numeric frontier");
    assert!(
        ready
            .requests()
            .iter()
            .all(|request| request.size.value() == 20.0)
    );
    answer_prepared_frontier(&ready, &mut writer);
    assert!(runtime.settle_text_sizing(ready, TextSizingSettlement::Ready));

    let handle = rebuild
        .borrow()
        .as_ref()
        .expect("target lifecycle rebuild handle")
        .clone();
    let changed = Rc::new(Cell::new(false));
    let accepted = Rc::clone(&changed);
    let listener = animation.add_listener(Rc::new(move || {
        if !accepted.replace(true) {
            size.set(22.75);
            handle.schedule(flui_foundation::RebuildReason::StateChange);
        }
    }));
    clock.advance(Duration::from_millis(20));
    let _ = runtime.pump(&mut clock, &mut sink);
    assert!(
        changed.get(),
        "sibling tick accepted the new target input in this pump"
    );
    assert_eq!(
        completed.get(),
        0,
        "the obsolete ready geometry cannot complete after sibling input admission"
    );
    let mut replacement = runtime.take_text_sizing_frontiers();
    // A fresh next-pump sample is allowed; completing the obsolete geometry is not.
    if replacement.is_empty() {
        let _ = runtime.pump(&mut clock, &mut sink);
        replacement = runtime.take_text_sizing_frontiers();
    }
    assert_eq!(
        replacement.len(),
        1,
        "a sibling's pre-phase admission must replace ready obsolete geometry before publication"
    );
    let work = replacement
        .pop()
        .expect("changed authored size requires its own exact answer");
    assert_eq!(work.presentation(), runtime.presentation_id());
    assert!(
        work.requests()
            .iter()
            .all(|request| request.size.value() == 22.75)
    );
    animation.remove_listener(listener);
    animation
        .stop()
        .expect("stop sibling after the observed admission");
    assert!(
        runtime.close_presentation_entered(id),
        "close sibling before observing the target's one-sink output"
    );
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    let _ = runtime.pump(&mut clock, &mut sink);
    sink.assert_text_size("replacement", 34.125);
    assert_eq!(
        completed.get(),
        1,
        "only repaired geometry discharges the retained completion"
    );
}

fn a_sampling_report_replaces_ready_text_before_geometry_publication() {
    sampling_report_replacement_case(false);
}

#[derive(Clone, StatefulView)]
struct AuthorityAnimatedText {
    mounted: std::rc::Rc<std::cell::RefCell<Option<AuthorityAnimation>>>,
    samples: std::rc::Rc<std::cell::RefCell<Vec<f64>>>,
}

struct AuthorityAnimation {
    controller: flui_animation::AnimationController,
    completion: flui_scheduler::PostFrameHandle,
    pipeline: flui_rendering::pipeline::PipelineCell,
}

struct AuthorityAnimatedTextState {
    owner: Option<flui_animation::DrivenController>,
    mounted: std::rc::Rc<std::cell::RefCell<Option<AuthorityAnimation>>>,
}

impl StatefulView for AuthorityAnimatedText {
    type State = AuthorityAnimatedTextState;

    fn create_state(&self) -> Self::State {
        AuthorityAnimatedTextState {
            owner: None,
            mounted: std::rc::Rc::clone(&self.mounted),
        }
    }
}

impl ViewState<AuthorityAnimatedText> for AuthorityAnimatedTextState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let registry = flui_widgets::VsyncScope::maybe_of(ctx)
            .expect("actual authority presentation animation registry");
        let owner =
            flui_animation::AnimationController::builder(std::time::Duration::from_millis(100))
                .build_on(Some(&registry));
        *self.mounted.borrow_mut() = Some(AuthorityAnimation {
            controller: owner.controller().clone(),
            completion: ctx.post_frame_handle().expect("authority completion lane"),
            pipeline: ctx.pipeline_owner().expect("authority pipeline capability"),
        });
        self.owner = Some(owner);
    }

    fn build(&self, view: &AuthorityAnimatedText, _: &dyn BuildContext) -> impl IntoView {
        use flui_animation::Animation as _;
        let controller = self
            .owner
            .as_ref()
            .expect("mounted authority animation")
            .controller();
        let sampled = controller.clone();
        let samples = std::rc::Rc::clone(&view.samples);
        flui_widgets::AnimatedBuilder::new(std::rc::Rc::new(controller.clone()), move || {
            let value = sampled.value();
            samples.borrow_mut().push(value);
            flui_widgets::Text::new("authority").style(
                flui_painting::typography::TextStyle::default().with_font_size(20.0 + 10.0 * value),
            )
        })
    }
}

thread_local! {
    // The report is Send + Sync; its UI runtime remains owner-thread local.
    // Release this borrow before invoking the real authority operation.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static AUTHORITY_REPORT_CONTEXT: std::cell::RefCell<Option<std::rc::Rc<AuthorityReportContext>>> = const { std::cell::RefCell::new(None) };
}

struct AuthorityReportContext {
    runtime: std::rc::Rc<flui_runtime::ui_runtime::UiRuntime>,
    source: flui_painting::TextSizingSource,
    replacement: flui_painting::TextSizingSource,
}

struct AuthorityReportScope;

impl AuthorityReportScope {
    fn install(
        runtime: &std::rc::Rc<flui_runtime::ui_runtime::UiRuntime>,
        source: &flui_painting::TextSizingSource,
        replacement: &flui_painting::TextSizingSource,
    ) -> Self {
        let context = std::rc::Rc::new(AuthorityReportContext {
            runtime: std::rc::Rc::clone(runtime),
            source: source.clone(),
            replacement: replacement.clone(),
        });
        let previous = AUTHORITY_REPORT_CONTEXT.with(|slot| slot.replace(Some(context)));
        assert!(previous.is_none(), "authority report scope is not nested");
        Self
    }
}

impl Drop for AuthorityReportScope {
    fn drop(&mut self) {
        let retired = AUTHORITY_REPORT_CONTEXT.with(std::cell::RefCell::take);
        drop(retired);
    }
}

/// The established test-support full-frame drive admits safe shared-owner
/// callback reentry; ordinary mutable host pumps cannot share an Rc runtime.
fn drive_shared_prepared_frame(
    runtime: &flui_runtime::ui_runtime::UiRuntime,
    clock: &flui_foundation::ManualClock,
    sink: &mut PreparedTextSink,
) -> bool {
    use flui_foundation::MonotonicClock as _;
    let now = clock.now();
    runtime.enter(|runtime| {
        runtime
            .owner_frame()
            .drive_frame(
                now,
                flui_scheduler::IdleDeadline::far_future(now),
                || {
                    runtime.drain_owner_inbox();
                },
                || runtime.render_frame_for_test(sink),
            )
            .expect("public shared owner admits the complete frame")
    })
}

fn an_initial_report_withdraws_retained_authority_before_publication() {
    authority_report_case(false, false, true);
}

fn an_initial_report_replaces_retained_authority_before_publication() {
    authority_report_case(false, true, true);
}

fn a_late_report_withdraws_its_own_retained_authority_before_publication() {
    authority_report_case(true, false, true);
}

fn a_late_report_replaces_its_own_retained_authority_before_publication() {
    authority_report_case(true, true, true);
}

fn fresh_authority_withdrawal_keeps_the_chosen_frame() {
    authority_report_case(false, false, false);
}

fn authority_report_case(late: bool, replace: bool, retained: bool) {
    use flui_animation::Animation as _;
    use flui_foundation::Listenable as _;
    use flui_runtime::frame_failure::{
        FailureDisposition, FrameFailureHandler, FrameFailureKind, SamplingFailureOrigin,
        SamplingStage,
    };
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    let clock = flui_foundation::ManualClock::new();
    let (mut runtime, _) = prepared_runtime(&clock);
    let primary_animation = Rc::new(RefCell::new(None));
    runtime
        .attach_root_widget_with_size(
            &PreparedSibling {
                animation: Rc::clone(&primary_animation),
                post_frame: Rc::new(RefCell::new(None)),
                focus: flui_interaction::focus::FocusNode::new(),
            },
            400.0,
            400.0,
        )
        .expect("mount actual reporting presentation");
    let target = runtime.assemble_presentation(crate::owner_publication::window());
    let target_id = runtime.install_presentation(target);
    let mounted = Rc::new(RefCell::new(None));
    let samples = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size_to(
            target_id,
            &AuthorityAnimatedText {
                mounted: Rc::clone(&mounted),
                samples: Rc::clone(&samples),
            },
            400.0,
            400.0,
        )
        .expect("mount actual authority target");
    let runtime = Rc::new(runtime);
    let mut sink = PreparedTextSink::default();
    assert!(drive_shared_prepared_frame(&runtime, &clock, &mut sink));
    let primary = primary_animation
        .borrow()
        .as_ref()
        .expect("primary controller")
        .clone();
    let target = mounted.take().expect("target lifecycle capabilities");
    let (_, mut writer) = flui_painting::TextSizing::captured();
    let source = writer.source();
    assert!(runtime.install_captured_text_sizing_for(target_id, source.clone()));
    let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
    let mut work = runtime.take_text_sizing_frontiers();
    assert_eq!(work.len(), 1);
    let original = work.pop().expect("initial authority frontier");
    answer_prepared_frontier(&original, &mut writer);
    assert!(runtime.settle_text_sizing(original, TextSizingSettlement::Ready));
    let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
    let _primary_run = primary.forward().expect("primary run");
    let _target_run = target.controller.forward().expect("target run");
    let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
    let completed = Rc::new(Cell::new(0));
    if retained {
        let completion = Rc::clone(&completed);
        target
            .completion
            .schedule(move |_| completion.set(completion.get() + 1))
            .expect("retained completion");
    }
    clock.advance(Duration::from_millis(25));
    let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
    assert_eq!(target.controller.value(), 0.25);
    let mut work = runtime.take_text_sizing_frontiers();
    assert_eq!(work.len(), 1);
    let ready = work.pop().expect("fractional retained authority frontier");
    assert!(
        ready
            .requests()
            .iter()
            .all(|request| request.size.value() == 22.5)
    );
    answer_prepared_frontier(&ready, &mut writer);
    assert!(runtime.settle_text_sizing(ready, TextSizingSettlement::Ready));
    if !retained {
        let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
        let completion = Rc::clone(&completed);
        target
            .completion
            .schedule(move |_| completion.set(completion.get() + 1))
            .expect("fresh completion");
    }
    let (_, mut replacement_writer) = flui_painting::TextSizing::captured();
    let replacement = replacement_writer.source();
    let wake_armed = Arc::new(AtomicBool::new(false));
    let wake_failures = Arc::new(AtomicUsize::new(0));
    let paint = late.then(|| {
        let pipeline = &target.pipeline;
        let paint = pipeline.with(|owner| {
            owner
                .render_invalidation_handle(owner.root_id().expect("target render root"))
                .expect("target paint capability")
        });
        let armed = Arc::clone(&wake_armed);
        let failures = Arc::clone(&wake_failures);
        pipeline.with_mut(|owner| {
            owner.set_on_need_visual_update(move || {
                if armed.swap(false, Ordering::SeqCst) {
                    failures.fetch_add(1, Ordering::SeqCst);
                    std::panic::panic_any("authority late paint wake");
                }
            });
        });
        paint
    });
    let reports = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let observed = Arc::clone(&reports);
    let changes = Arc::new(AtomicUsize::new(0));
    let accepted_changes = Arc::clone(&changes);
    let primary_id = runtime.presentation_id();
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        let sampling = match &report.kind {
            FrameFailureKind::SamplingPanic {
                first,
                blocking,
                admitted_attempt,
                ..
            } => Some((
                (first.stage, first.origin),
                blocking.map(|blocker| (blocker.stage, blocker.origin)),
                *admitted_attempt,
            )),
            _ => None,
        };
        observed
            .lock()
            .push((report.address, report.disposition, sampling));
        if late && report.address.presentation_id == primary_id {
            paint
                .as_ref()
                .expect("late paint capability")
                .mark_needs_paint()
                .expect("enqueue before arming wake failure");
            wake_armed.store(true, Ordering::SeqCst);
            return;
        }
        if report.address.presentation_id == if late { target_id } else { primary_id } {
            let context = AUTHORITY_REPORT_CONTEXT
                .with(|slot| slot.borrow().clone())
                .expect("scoped shared runtime");
            let changed = if replace {
                context
                    .runtime
                    .install_captured_text_sizing_for(target_id, context.replacement.clone())
            } else {
                context
                    .runtime
                    .withdraw_captured_text_sizing_for(target_id, &context.source)
            };
            if changed {
                accepted_changes.fetch_add(1, Ordering::SeqCst);
            }
        }
    })));
    let scope = AuthorityReportScope::install(&runtime, &source, &replacement);
    let armed = Rc::new(Cell::new(true));
    let failure = Rc::clone(&armed);
    let listener = primary.add_listener(Rc::new(move || {
        if failure.replace(false) {
            std::panic::panic_any("authority sampling delivery");
        }
    }));
    let samples_before = samples.borrow().len();
    clock.advance(Duration::from_millis(25));
    let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
    assert!(
        !armed.get(),
        "actual primary animation delivered the failing callback"
    );
    assert_eq!(
        changes.load(Ordering::SeqCst),
        1,
        "real authority operation committed once"
    );
    assert_eq!(wake_failures.load(Ordering::SeqCst), usize::from(late));
    assert_eq!(reports.lock().len(), 1 + usize::from(late));
    if retained {
        assert_eq!(
            completed.get(),
            0,
            "authority replacement must not complete the withdrawn frozen attempt"
        );
        assert_eq!(
            samples.borrow().len(),
            samples_before,
            "the report frame cannot rebuild using its obsolete retained tick"
        );
        assert!(
            !sink.glyph_sizes.iter().any(|(text, _)| text == "authority"),
            "the report frame cannot publish withdrawn target geometry"
        );
        assert!(
            runtime.take_text_sizing_frontiers().is_empty(),
            "a replacement authority cannot receive a frontier from the obsolete tick"
        );
        assert_eq!(target.controller.value(), 0.25);
        let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
        assert_eq!(
            target.controller.value(),
            0.5,
            "the next owner frame selects fresh motion time"
        );
        assert_eq!(samples.borrow().last().copied(), Some(0.5));
        if replace {
            let mut replacement_work = runtime.take_text_sizing_frontiers();
            assert_eq!(replacement_work.len(), 1);
            let work = replacement_work.pop().expect("fresh replacement frontier");
            assert_eq!(work.source(), &replacement_writer.source());
            assert!(
                work.requests()
                    .iter()
                    .all(|request| request.size.value() == 25.0)
            );
            answer_prepared_frontier(&work, &mut replacement_writer);
            assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
            let _ = drive_shared_prepared_frame(&runtime, &clock, &mut sink);
            sink.assert_text_size("authority", 37.5);
        } else {
            sink.assert_text_size("authority", 25.0);
        }
    } else {
        assert_eq!(target.controller.value(), 0.5);
        assert_eq!(samples.borrow().last().copied(), Some(0.5));
        sink.assert_text_size("authority", 25.0);
    }
    assert_eq!(
        completed.get(),
        1,
        "one coherent replacement discharges completion once"
    );
    assert_eq!(reports.lock().len(), 1 + usize::from(late));
    let reports = reports.lock();
    assert_eq!(reports[0].1, FailureDisposition::Contained);
    assert_eq!(
        reports[0].2,
        Some((
            (SamplingStage::Animation, SamplingFailureOrigin::Delivery),
            None,
            true
        ))
    );
    if late {
        assert_eq!(reports[1].1, FailureDisposition::Contained);
        assert_eq!(
            reports[1].2,
            Some((
                (SamplingStage::Preparation, SamplingFailureOrigin::Delivery),
                None,
                true
            ))
        );
    }
    drop(reports);
    primary.remove_listener(listener);
    drop(scope);
}

fn a_late_preparation_report_replaces_its_own_ready_text_before_geometry_publication() {
    sampling_report_replacement_case(true);
}

fn sampling_report_replacement_case(late_preparation: bool) {
    use flui_foundation::Listenable as _;
    use flui_runtime::frame_failure::{
        FailureDisposition, FrameFailureHandler, FrameFailureKind, SamplingFailureOrigin,
        SamplingStage,
    };
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    let mut clock = flui_foundation::ManualClock::new();
    let (mut runtime, _) = prepared_runtime(&clock);
    let size = Arc::new(AtomicU64::new(20.0_f64.to_bits()));
    let rebuild = Arc::new(parking_lot::Mutex::new(None));
    let post_frame = Rc::new(RefCell::new(None));
    runtime
        .attach_root_widget_with_size(
            &PreparedTextProbe {
                child: AuthoredText {
                    font_size: Arc::clone(&size),
                    rebuild: Arc::clone(&rebuild),
                }
                .boxed(),
                rebuild: Rc::new(RefCell::new(None)),
                post_frame: Rc::clone(&post_frame),
            },
            400.0,
            400.0,
        )
        .expect("mount target text with its ordinary cross-thread rebuild handle");
    let mut sink = PreparedTextSink::default();
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    let sibling = runtime.assemble_presentation(crate::owner_publication::window());
    let sibling_id = runtime.install_presentation(sibling);
    let animation_slot = Rc::new(RefCell::new(None));
    runtime
        .attach_root_widget_with_size_to(
            sibling_id,
            &PreparedSibling {
                animation: Rc::clone(&animation_slot),
                post_frame: Rc::new(RefCell::new(None)),
                focus: flui_interaction::focus::FocusNode::new(),
            },
            400.0,
            400.0,
        )
        .expect("mount actual sibling animation");
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    let animation = animation_slot
        .borrow()
        .as_ref()
        .expect("sibling lifecycle owns animation")
        .clone();
    let _run = animation.forward().expect("start sibling animation");
    let _ = runtime.pump(&mut clock, &mut sink);
    let (_, mut writer) = flui_painting::TextSizing::captured();
    let completed = Rc::new(Cell::new(0));
    let completion = Rc::clone(&completed);
    post_frame
        .borrow()
        .as_ref()
        .expect("target completion handle")
        .schedule(move |_| completion.set(completion.get() + 1))
        .expect("completion admitted before retained preparation");
    assert!(runtime.install_captured_text_sizing_for(runtime.presentation_id(), writer.source()));
    let _ = runtime.pump(&mut clock, &mut sink);
    let mut frontiers = runtime.take_text_sizing_frontiers();
    assert_eq!(frontiers.len(), 1);
    let ready = frontiers.pop().expect("original target frontier");
    assert!(
        ready
            .requests()
            .iter()
            .all(|request| request.size.value() == 20.0)
    );
    answer_prepared_frontier(&ready, &mut writer);
    assert!(runtime.settle_text_sizing(ready, TextSizingSettlement::Ready));
    let handle = rebuild
        .lock()
        .as_ref()
        .expect("target lifecycle rebuild handle")
        .clone();
    let wake_failures = Arc::new(AtomicUsize::new(0));
    let wake_armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let paint = late_preparation.then(|| {
        let pipeline = runtime
            .widgets()
            .pipeline_owner()
            .expect("mounted target owns its real pipeline");
        let paint = pipeline.with(|owner| {
            owner
                .render_invalidation_handle(owner.root_id().expect("mounted root render object"))
                .expect("actual root admits external paint invalidation")
        });
        let armed = Arc::clone(&wake_armed);
        let failures = Arc::clone(&wake_failures);
        pipeline.with_mut(|owner| {
            owner.set_on_need_visual_update(move || {
                if armed.swap(false, Ordering::SeqCst) {
                    failures.fetch_add(1, Ordering::SeqCst);
                    std::panic::panic_any("late retained paint wake");
                }
            });
        });
        paint
    });
    let target_id = runtime.presentation_id();
    let reports = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        let sampling = match &report.kind {
            FrameFailureKind::SamplingPanic {
                first, blocking, ..
            } => Some((
                (first.stage, first.origin),
                blocking.map(|blocker| (blocker.stage, blocker.origin)),
            )),
            _ => None,
        };
        reported
            .lock()
            .push((report.address, report.disposition, sampling));
        if let Some(paint) = &paint {
            if report.address.presentation_id == sibling_id {
                paint
                    .mark_needs_paint()
                    .expect("accepted paint-only input preserves the saved layout premise");
                // The enqueue's wake succeeded. Fail only the canonical
                // mark's later delivery during this target's reconciliation.
                wake_armed.store(true, Ordering::SeqCst);
            } else if report.address.presentation_id == target_id {
                size.store(22.75_f64.to_bits(), Ordering::SeqCst);
                handle.schedule(flui_foundation::RebuildReason::StateChange);
            }
        } else {
            size.store(22.75_f64.to_bits(), Ordering::SeqCst);
            handle.schedule(flui_foundation::RebuildReason::StateChange);
        }
    })));
    let armed = Rc::new(Cell::new(true));
    let failing = Rc::clone(&armed);
    let listener = animation.add_listener(Rc::new(move || {
        if failing.replace(false) {
            std::panic::panic_any("sibling delivery before ready text");
        }
    }));
    clock.advance(Duration::from_millis(20));
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.pump(&mut clock, &mut sink)
    }));
    let _ = attempt.expect("sampling report delivery cannot escape the pump");
    assert!(!armed.get(), "the actual animation listener failed");
    assert_eq!(
        wake_failures.load(Ordering::SeqCst),
        usize::from(late_preparation),
        "the late case failed only after the actual paint mark committed"
    );
    let mut expected = vec![(
        flui_foundation::PresentationAddress {
            ui_runtime_id: runtime.id(),
            presentation_id: sibling_id,
        },
        FailureDisposition::Contained,
        Some((
            (SamplingStage::Animation, SamplingFailureOrigin::Delivery),
            None,
        )),
    )];
    if late_preparation {
        expected.push((
            flui_foundation::PresentationAddress {
                ui_runtime_id: runtime.id(),
                presentation_id: target_id,
            },
            FailureDisposition::Contained,
            Some((
                (SamplingStage::Preparation, SamplingFailureOrigin::Delivery),
                None,
            )),
        ));
    }
    assert_eq!(reports.lock().as_slice(), expected);
    assert_eq!(
        completed.get(),
        0,
        "diagnostic-side admission cannot complete obsolete ready geometry"
    );
    let mut replacement = runtime.take_text_sizing_frontiers();
    if replacement.is_empty() {
        let _ = runtime.pump(&mut clock, &mut sink);
        replacement = runtime.take_text_sizing_frontiers();
    }
    assert_eq!(
        replacement.len(),
        1,
        "target requires an exact fresh frontier"
    );
    let work = replacement.pop().expect("replacement target frontier");
    assert_eq!(work.presentation(), runtime.presentation_id());
    assert!(
        work.requests()
            .iter()
            .all(|request| request.size.value() == 22.75)
    );
    animation.remove_listener(listener);
    animation.stop().expect("stop observed sibling");
    assert!(
        runtime.close_presentation_entered(sibling_id),
        "one sink can now observe the target's actual scene"
    );
    answer_prepared_frontier(&work, &mut writer);
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Ready));
    finish_prepared_text(
        "diagnostic-side replacement",
        &mut runtime,
        &mut clock,
        &mut sink,
        &mut writer,
    );
    sink.assert_text_size("measured", 34.125);
    assert_eq!(completed.get(), 1, "replacement completes once");
    assert_eq!(reports.lock().len(), expected.len());
}

fn a_waiting_presentation_does_not_suspend_sibling_animation_completion_or_ime() {
    use flui_animation::Animation as _;
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, Utf16Offset, Utf16Range,
    };
    use flui_runtime::ui_runtime::TextSizingSettlement;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    let PendingEditable {
        mut clock,
        mut runtime,
        mut sink,
        completed,
        work,
        ..
    } = pending_editable();
    assert!(runtime.settle_text_sizing(work, TextSizingSettlement::Waiting));
    let sibling_host = flui_testing::RecordingTextStoreHost::new();
    let sibling = runtime.assemble_presentation(
        crate::owner_publication::window().with_text_store_host(Some(sibling_host.clone())),
    );
    let id = runtime.install_presentation(sibling);
    let animation_slot = Rc::new(RefCell::new(None));
    let post_frame = Rc::new(RefCell::new(None));
    let focus = flui_interaction::focus::FocusNode::new();
    runtime
        .attach_root_widget_with_size_to(
            id,
            &PreparedSibling {
                animation: Rc::clone(&animation_slot),
                post_frame: Rc::clone(&post_frame),
                focus: Rc::clone(&focus),
            },
            400.0,
            400.0,
        )
        .expect("mount actual sibling animated field once");
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    let animation = animation_slot
        .borrow()
        .as_ref()
        .expect("lifecycle created the retained sibling animation")
        .clone();
    let _ = focus.request_focus();
    let _ = runtime.pump(&mut clock, &mut sink);
    let store = sibling_host
        .focused_store()
        .expect("sibling owns its own focused text store");
    animation.forward().expect("start sibling animation");
    let _ = runtime.pump(&mut clock, &mut sink);
    let sibling_completed = Rc::new(Cell::new(0));
    let observed = Rc::clone(&sibling_completed);
    let geometry = Rc::new(RefCell::new(None));
    let granted = Rc::clone(&geometry);
    let callback = post_frame
        .borrow()
        .as_ref()
        .expect("sibling scoped completion")
        .clone();
    callback
        .schedule(move |_| {
            observed.set(observed.get() + 1);
            assert_eq!(
                store.request_lock(
                    LockGrant::read(move |session| {
                        *granted.borrow_mut() = Some(
                            session
                                .rect_for_range(Utf16Range::collapsed(Utf16Offset::new(7)))
                                .expect("healthy sibling caret"),
                        );
                    }),
                    LockTiming::Async
                ),
                Ok(LockOutcome::Deferred)
            );
        })
        .expect("sibling callback admission");
    clock.advance(Duration::from_millis(20));
    assert!(
        runtime.pump(&mut clock, &mut sink).presented(),
        "healthy sibling still presents through aggregate sink"
    );
    assert!((animation.value() - 0.2).abs() < 0.000_001);
    sink.assert_text_size("sibling", 22.0);
    assert_eq!(sibling_completed.get(), 1);
    assert!(
        geometry
            .borrow()
            .as_ref()
            .expect("sibling deferred grant reached healthy geometry")
            .bounds
            .size
            .height
            > 22.0
    );
    assert_eq!(
        completed.get(),
        0,
        "target completion stays blocked independently"
    );
    assert!(
        runtime.take_text_sizing_frontiers().is_empty(),
        "target still awaits an explicit service cue"
    );
}

#[test]
fn registered_text_preparation_preserves_the_actual_presentation_attempt() {
    crate::table_test::run_table(
        "registered_text_preparation_preserves_the_actual_presentation_attempt",
        &[
            (
                "an_animated_size_resumes_its_original_tick_after_host_service_yields",
                an_animated_size_resumes_its_original_tick_after_host_service_yields as fn(),
            ),
            (
                "deadline_delivery_does_not_discard_the_retained_animation_tick",
                deadline_delivery_does_not_discard_the_retained_animation_tick as fn(),
            ),
            (
                "unknown_deadline_withdraws_the_old_receipt_and_recovers_fresh_geometry",
                unknown_deadline_withdraws_the_old_receipt_and_recovers_fresh_geometry as fn(),
            ),
            (
                "a_background_deadline_withdraws_the_old_receipt_without_opening_the_barrier",
                a_background_deadline_withdraws_the_old_receipt_without_opening_the_barrier
                    as fn(),
            ),
            (
                "a_background_query_failure_withdraws_the_old_receipt_without_opening_the_barrier",
                a_background_query_failure_withdraws_the_old_receipt_without_opening_the_barrier
                    as fn(),
            ),
            (
                "a_same_queued_rebuild_admission_cancels_the_old_preparation_premise",
                a_same_queued_rebuild_admission_cancels_the_old_preparation_premise as fn(),
            ),
            (
                "a_sibling_animation_admission_replaces_ready_text_before_geometry_publication",
                a_sibling_animation_admission_replaces_ready_text_before_geometry_publication
                    as fn(),
            ),
            (
                "a_sampling_report_replaces_ready_text_before_geometry_publication",
                a_sampling_report_replaces_ready_text_before_geometry_publication as fn(),
            ),
            (
                "a_late_preparation_report_replaces_its_own_ready_text_before_geometry_publication",
                a_late_preparation_report_replaces_its_own_ready_text_before_geometry_publication
                    as fn(),
            ),
            (
                "an_initial_report_withdraws_retained_authority_before_publication",
                an_initial_report_withdraws_retained_authority_before_publication as fn(),
            ),
            (
                "an_initial_report_replaces_retained_authority_before_publication",
                an_initial_report_replaces_retained_authority_before_publication as fn(),
            ),
            (
                "a_late_report_withdraws_its_own_retained_authority_before_publication",
                a_late_report_withdraws_its_own_retained_authority_before_publication as fn(),
            ),
            (
                "a_late_report_replaces_its_own_retained_authority_before_publication",
                a_late_report_replaces_its_own_retained_authority_before_publication as fn(),
            ),
            (
                "fresh_authority_withdrawal_keeps_the_chosen_frame",
                fresh_authority_withdrawal_keeps_the_chosen_frame as fn(),
            ),
            (
                "a_dropped_receipt_retries_only_after_its_deadline_and_completes",
                a_dropped_receipt_retries_only_after_its_deadline_and_completes as fn(),
            ),
            (
                "a_held_receipt_parks_geometry_but_a_late_answer_heals_it",
                a_held_receipt_parks_geometry_but_a_late_answer_heals_it as fn(),
            ),
            (
                "repeated_lost_receipts_stop_retrying_until_explicit_service",
                repeated_lost_receipts_stop_retrying_until_explicit_service as fn(),
            ),
            (
                "a_background_answer_waits_for_resume_before_paint_and_completion",
                a_background_answer_waits_for_resume_before_paint_and_completion as fn(),
            ),
            (
                "a_lazy_child_adds_a_frontier_without_reentering_the_root_or_partial_paint",
                a_lazy_child_adds_a_frontier_without_reentering_the_root_or_partial_paint as fn(),
            ),
            (
                "a_waiting_presentation_does_not_suspend_sibling_animation_completion_or_ime",
                a_waiting_presentation_does_not_suspend_sibling_animation_completion_or_ime
                    as fn(),
            ),
        ],
    );
}

#[test]
fn native_text_preparation_does_not_complete_the_presentation() {
    crate::table_test::run_table(
        "native_text_preparation_does_not_complete_the_presentation",
        &[
            (
                "measured_text_delivers_its_lifecycle_completion_callback",
                measured_text_delivers_its_lifecycle_completion_callback as fn(),
            ),
            (
                "missing_captured_sizes_withhold_the_lifecycle_completion_callback",
                missing_captured_sizes_withhold_the_lifecycle_completion_callback as fn(),
            ),
            (
                "an_unregistered_capture_parks_until_its_provider_changes",
                an_unregistered_capture_parks_until_its_provider_changes as fn(),
            ),
        ],
    );
}

#[test]
fn parked_text_preparation_does_not_publish_partial_ime_geometry() {
    crate::table_test::run_table(
        "parked_text_preparation_does_not_publish_partial_ime_geometry",
        &[(
            "parked_partial_text_geometry_is_unavailable_to_the_real_ime_store",
            parked_partial_text_geometry_is_unavailable_to_the_real_ime_store as fn(),
        )],
    );
}

#[test]
fn competing_text_measurement_waits_for_changed_input() {
    crate::table_test::run_table(
        "competing_text_measurement_waits_for_changed_input",
        &[(
            "a_competing_text_loan_waits_for_an_explicit_input_change",
            a_competing_text_loan_waits_for_an_explicit_input_change as fn(),
        )],
    );
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
