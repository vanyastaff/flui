use std::sync::atomic::{AtomicBool as StdAtomicBool, AtomicUsize};

use super::*;

/// Minimal leaf view/element so a headless `attach_root_widget` has
/// something to mount without pulling in a widget crate.
#[derive(Clone)]
struct LeafView;

impl flui_view::RenderView for LeafView {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        flui_objects::RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_size(Some(0.0), Some(0.0))
    }
}

impl flui_view::View for LeafView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

fn test_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_foundation::geometry::Size::new(800.0, 600.0))
}

/// E2/E3 regression: `UiRuntime` hands its shared `PipelineOwner` to the
/// `WidgetsBinding` it owns, so `attach_root_widget` actually
/// bootstraps the root render tree.
pub(crate) fn attach_root_widget_bootstraps_shared_render_tree() {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&LeafView))
        .expect("attach succeeds");
    assert!(
        ui_runtime
            .pipeline_for_test()
            .with(|owner| owner.root_id().is_some()),
        "UiRuntime must pass its PipelineOwner to the widgets binding so the \
         root render tree bootstraps; without it the window renders nothing",
    );
    assert!(
        ui_runtime
            .renderer()
            .root_pipeline_owner()
            .with(|owner| owner.root_id().is_some()),
        "the ui_runtime's renderer exposes the same bootstrapped pipeline",
    );
}

/// The production frame path polls the async driver **exactly once**,
/// on the UI runtime's own scheduler, in the mid-frame slot — and the
/// pipeline runs afterwards, in the persistent slot.
pub(crate) fn the_production_frame_polls_the_ui_runtimes_async_driver_once_before_the_pipeline() {
    let ui_runtime = UiRuntime::for_test();
    let scheduler = ui_runtime.scheduler();

    let polls = Arc::new(AtomicUsize::new(0));
    let polls_for_task = Arc::clone(&polls);
    let _token = ui_runtime
        .owner_frame()
        .async_driver()
        .spawn_local(Box::pin(async move {
            polls_for_task.fetch_add(1, Ordering::Release);
        }));
    assert_eq!(
        polls.load(Ordering::Acquire),
        0,
        "spawn must not poll inline"
    );

    let polled_before_pipeline = Arc::new(StdAtomicBool::new(false));
    let flag = Arc::clone(&polled_before_pipeline);
    let polls_probe = Arc::clone(&polls);

    scheduler.drive_frame(
        ui_runtime.owner_frame(),
        flui_scheduler::Instant::now(),
        flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
        || {
            flag.store(polls_probe.load(Ordering::Acquire) == 1, Ordering::Release);
            let _ = ui_runtime.draw_frame(test_constraints());
        },
    );

    assert!(
        polled_before_pipeline.load(Ordering::Acquire),
        "the async driver must be polled before the pipeline runs"
    );
    assert_eq!(
        polls.load(Ordering::Acquire),
        1,
        "exactly one driver poll per frame"
    );
}

// ---- Input lifecycle gate --------------------------------------------

// ---- Gesture-arena / pointer dispatch --------------------------------

// ---- Vsync wiring (production frame continuation) -------------------

/// A raw frame time that is not a duration (NaN, ±∞, negative) or that runs
/// backwards holds the presentation's animation time: the running
/// controller keeps its value and keeps running, and the next valid frame
/// continues from where the timeline stood.
pub(crate) fn an_invalid_or_backwards_frame_time_holds_the_animation() {
    use flui_animation::{Animation as _, AnimationController};
    use std::time::Duration;

    let ui_runtime = mount_root();
    let owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&ui_runtime.vsync()));
    let controller = owner.controller();
    controller.forward().expect("fresh controller forwards");
    let frame_at = |secs: f64| {
        ui_runtime.set_now_secs_for_test(secs);
        let _ = ui_runtime.enter(|ui_runtime| ui_runtime.draw_frame_entered(test_constraints()));
    };

    // The first tick anchors the run; 250 ms later it is a quarter through.
    frame_at(0.5);
    frame_at(0.75);
    let held = controller.value();
    assert!((held - 0.25).abs() < 1e-9, "a quarter through, got {held}");

    for (case, secs) in [
        ("nan", f64::NAN),
        ("positive_infinity", f64::INFINITY),
        ("negative_infinity", f64::NEG_INFINITY),
        ("negative", -1.0),
        ("backwards", 0.6),
    ] {
        frame_at(secs);
        assert_eq!(
            controller.value().to_bits(),
            held.to_bits(),
            "{case}: the value holds"
        );
        assert!(controller.is_animating(), "{case}: the run keeps running");
        assert!(
            ui_runtime.vsync().has_running(),
            "{case}: the run still demands frames"
        );
    }

    frame_at(1.0);
    let resumed = controller.value();
    assert!(
        (resumed - 0.5).abs() < 1e-9,
        "continues from the timeline, got {resumed}"
    );
}

// ---- render_frame retry / first-frame-deferral semantics ----

fn mount_root() -> UiRuntime {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&LeafView))
        .expect("attach succeeds");
    ui_runtime
}

pub(crate) fn surface_lost_keeps_needs_redraw_armed_for_a_retry() {
    let ui_runtime = mount_root();
    let mut backend = ScriptedSink::single_shot(SubmitVerdict::SurfaceStale);

    ui_runtime.mark_rendered();
    let presented = ui_runtime.render_frame(&mut backend);

    assert!(!presented, "a SurfaceLost frame never reaches present()");
    assert_eq!(
        backend.submit_calls, 1,
        "precondition: the mounted scene actually reached render_scene"
    );
    assert!(
        ui_runtime.needs_redraw(),
        "a dropped SurfaceLost frame must re-arm needs_redraw so the next wake \
         actually retries"
    );
}
