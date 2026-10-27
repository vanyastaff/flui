use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

use flui_rendering::constraints::BoxConstraints;

use super::*;

fn counting_window(id: u64) -> (Rc<dyn PlatformWindow>, Arc<AtomicU32>) {
    let window = crate::testing::TestWindow::new().with_id(id);
    let redraw_calls = window.redraw_calls_handle();
    (Rc::new(window), redraw_calls)
}

/// The same edge, fired from a foreign thread — the shape an async
/// task completing on a background executor actually takes.
///
/// The wake handed to a UI runtime is `Send + Sync` precisely so this is
/// legal; this pins that the scheduler seam preserves it.
pub(crate) fn a_cross_thread_frame_request_reaches_the_ui_runtimes_platform_wake() {
    let wakes = Arc::new(AtomicU32::new(0));
    let wake_counter = Arc::clone(&wakes);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_counter.fetch_add(1, AtomicOrdering::Relaxed);
    });

    let (window, _calls) = counting_window(1);
    let mut ui_runtime = UiRuntime::new(
        window,
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            wake,
            Arc::new(AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("ui_runtime constructs");
    // Clear the `frame_scheduled` latch so the request below is a real
    // false->true transition — the only edge the hook fires on.
    ui_runtime.pump_background();
    let before = wakes.load(AtomicOrdering::Relaxed);

    let waker = ui_runtime.scheduler().frame_waker();
    std::thread::spawn(move || waker.request_frame())
        .join()
        .expect("waker thread completes");

    assert!(
        wakes.load(AtomicOrdering::Relaxed) > before,
        "a frame request raised off the owner thread must still reach the wake"
    );
}

fn counting_ui_runtime(id: u64) -> (UiRuntime, Arc<AtomicU32>) {
    let wakes = Arc::new(AtomicU32::new(0));
    let wake_counter = Arc::clone(&wakes);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_counter.fetch_add(1, AtomicOrdering::Relaxed);
    });
    let (window, _calls) = counting_window(id);
    let mut ui_runtime = UiRuntime::new(
        window,
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            wake,
            Arc::new(AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("ui_runtime constructs");
    ui_runtime.pump_background();
    (ui_runtime, wakes)
}

/// The cross-thread half of a UI runtime's scheduler: a `FrameWaker` sent to a
/// worker wakes its own UI runtime once per frame demand, never a sibling, and
/// does nothing once its UI runtime is gone.
pub(crate) fn frame_waker_wakes_the_ui_runtime_from_a_worker() {
    let (mut ui_runtime, wakes) = counting_ui_runtime(1);
    let (sibling, sibling_wakes) = counting_ui_runtime(2);
    let sibling_before = sibling_wakes.load(AtomicOrdering::Relaxed);

    let before = wakes.load(AtomicOrdering::Relaxed);

    let waker = ui_runtime.scheduler().frame_waker();
    let worker_waker = waker.clone();
    std::thread::spawn(move || {
        worker_waker.request_frame();
        worker_waker.request_frame();
    })
    .join()
    .expect("worker completes");
    assert_eq!(
        wakes.load(AtomicOrdering::Relaxed),
        before + 1,
        "two requests before a frame are one demand, one wake"
    );
    assert!(ui_runtime.scheduler().is_frame_scheduled());

    ui_runtime.pump_background();
    let worker_waker = waker.clone();
    std::thread::spawn(move || worker_waker.request_frame())
        .join()
        .expect("worker completes");
    assert_eq!(
        wakes.load(AtomicOrdering::Relaxed),
        before + 2,
        "a cleared latch takes the next demand"
    );

    // Cleared, so a waker that kept the scheduler alive would wake it again.
    ui_runtime.pump_background();
    drop(ui_runtime);
    std::thread::spawn(move || waker.request_frame())
        .join()
        .expect("a waker outliving its ui_runtime is inert, not a panic");
    assert_eq!(wakes.load(AtomicOrdering::Relaxed), before + 2);
    assert_eq!(
        sibling_wakes.load(AtomicOrdering::Relaxed),
        sibling_before,
        "another ui_runtime's waker never wakes this one"
    );
    drop(sibling);
}

/// A mounted UI runtime with its first frame already drawn.
fn settled_runtime() -> (UiRuntime, ScriptedSink) {
    let mut ui_runtime = UiRuntime::for_test();
    ui_runtime
        .attach_root_widget(&SizedBox::square(10.0))
        .expect("root attaches");
    let mut sink = ScriptedSink::always_presents();
    let _ = ui_runtime.pump(
        &mut crate::pump::SampledClock(web_time::Instant::now()),
        &mut sink,
    );
    (ui_runtime, sink)
}

/// A redraw requested after a presentation's segment took its redraw bit
/// (the orphaned-focus and semantics-subscriber repairs) is the next frame's
/// demand: the frame that raised it must not settle it as rendered.
pub(crate) fn a_redraw_requested_after_its_segment_survives_the_frame() {
    let (mut ui_runtime, mut sink) = settled_runtime();
    let armed = Rc::new(Cell::new(true));
    let probe = Rc::clone(&armed);
    ui_runtime.presentations.primary().set_segment_probe(
        SegmentPhase::Tail,
        Some(Box::new(move |presentation| {
            if probe.replace(false) {
                presentation.mark_redraw_pending();
            }
        })),
    );
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    });
    let _ = ui_runtime.pump(
        &mut crate::pump::SampledClock(web_time::Instant::now()),
        &mut sink,
    );
    assert!(!armed.get(), "the segment ran and raised the request");
    assert!(
        ui_runtime.needs_redraw(),
        "the request raised during the frame keeps the host gate open"
    );
}

/// A visual update declined because the frame was still in its pipeline slot
/// (a hover callback's state change, a paint-time mark) leaves the tree dirty;
/// the frame asks for the next one instead of waiting for unrelated input.
pub(crate) fn a_mark_after_the_pipeline_requests_the_next_frame() {
    let (mut ui_runtime, mut sink) = settled_runtime();
    let armed = Rc::new(Cell::new(true));
    let probe = Rc::clone(&armed);
    ui_runtime.presentations.primary().set_segment_probe(
        SegmentPhase::Tail,
        Some(Box::new(move |presentation| {
            if probe.replace(false) {
                presentation.pipeline().with_mut(|owner| {
                    let root = owner.root_id().expect("a mounted root");
                    owner.mark_needs_paint(root);
                });
            }
        })),
    );
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    });
    let _ = ui_runtime.pump(
        &mut crate::pump::SampledClock(web_time::Instant::now()),
        &mut sink,
    );
    assert!(!armed.get(), "the segment ran and marked its tree");
    assert!(
        ui_runtime.needs_redraw(),
        "the dirty tree left by the frame's tail keeps the host gate open"
    );
}

/// Demand from many state changes before a frame pokes the window once; the
/// pump that samples the presentation lets the next demand poke again.
pub(crate) fn many_frame_requests_before_a_frame_poke_the_window_once() {
    use flui_animation::AnimationController;
    use std::time::Duration;

    let (window, redraws) = counting_window(1);
    // A presentation holds its window weakly; the host keeps it alive.
    let ui_runtime = UiRuntime::for_test_with_window(Rc::clone(&window));
    let before = redraws.load(AtomicOrdering::Relaxed);
    let owners: Vec<_> = (0..100)
        .map(|_| {
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(&ui_runtime.vsync()))
        })
        .collect();
    for owner in &owners {
        owner.controller().forward().expect("run admission");
    }
    assert_eq!(
        redraws.load(AtomicOrdering::Relaxed) - before,
        1,
        "a hundred frame requests before a frame poke the window once"
    );

    let mut sink = ScriptedSink::always_presents();
    let _ = ui_runtime.render_frame(&mut sink);
    let late =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&ui_runtime.vsync()));
    late.controller().forward().expect("run admission");
    assert_eq!(
        redraws.load(AtomicOrdering::Relaxed) - before,
        2,
        "the frame that sampled the presentation reopens the poke"
    );
}

/// Each presentation is laid out at its own window's logical size, not the
/// primary surface's.
pub(crate) fn a_secondary_presentation_is_laid_out_at_its_own_size() {
    use flui_foundation::geometry::Size;

    let mut ui_runtime = UiRuntime::for_test();
    ui_runtime
        .attach_root_widget(&SizedBox::square(10.0))
        .expect("primary root");
    let window: Rc<dyn PlatformWindow> = Rc::new(
        crate::testing::TestWindow::new()
            .focused(false)
            .with_scale_factor(2.0)
            .with_sizes(Size::new(600, 400), Size::new(300.0, 200.0)),
    );
    // A presentation holds its window weakly; the host keeps it alive.
    let assembled = ui_runtime.assemble_presentation(Rc::clone(&window));
    let secondary = ui_runtime.install_presentation(assembled);
    ui_runtime
        .attach_root_widget_to_for_test(secondary, &SizedBox::expand())
        .expect("secondary root");

    let mut sink = ScriptedSink::always_presents().with_size(800, 600);
    let _ = ui_runtime.render_frame(&mut sink);

    let constraints_of = |presentation: &crate::presentation::PresentationState| {
        presentation
            .pipeline()
            .with(flui_rendering::pipeline::PipelineOwner::root_constraints)
    };
    assert_eq!(
        constraints_of(ui_runtime.presentations.primary()),
        Some(BoxConstraints::tight(Size::new(800.0, 600.0)))
    );
    assert_eq!(
        constraints_of(ui_runtime.presentations.get(secondary).expect("secondary")),
        Some(BoxConstraints::tight(Size::new(300.0, 200.0)))
    );
}
