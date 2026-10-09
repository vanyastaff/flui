use super::*;

pub(crate) fn closed_presentation_animation_cannot_wake_a_surviving_window() {
    use crate::pump::SampledClock;
    use flui_animation::AnimationController;
    use std::time::Duration;

    let mut runtime = mount_root_here();
    let closed = runtime.install_second_presentation_for_test();
    let retired_clock = runtime
        .presentations
        .get(closed)
        .expect("second presentation")
        .vsync();
    assert!(runtime.close_presentation_entered(closed));
    let mut sink = ScriptedSink::always_presents();
    let origin = web_time::Instant::now();
    runtime.set_now_secs_for_test(0.0);
    let _ = runtime.pump(&mut SampledClock(origin), &mut sink);
    assert!(
        !runtime.scheduler().has_scheduled_frame(),
        "survivor quiesces"
    );

    let retired_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&retired_clock));
    retired_owner
        .controller()
        .forward()
        .expect("retained clock run");
    assert!(
        !runtime.scheduler().has_scheduled_frame(),
        "a closed presentation cannot request a sibling's frame"
    );
    let live_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&runtime.vsync()));
    live_owner.controller().forward().expect("survivor run");
    assert!(
        runtime.scheduler().has_scheduled_frame(),
        "live driver still wakes"
    );
}

pub(crate) fn starting_an_idle_presentation_animation_requests_its_first_frame() {
    use crate::pump::SampledClock;
    use flui_animation::{Animation as _, AnimationController};
    use std::time::Duration;

    let mut runtime = mount_root_here();
    let mut sink = ScriptedSink::always_presents();
    let origin = web_time::Instant::now();
    runtime.set_now_secs_for_test(0.0);
    assert!(
        runtime
            .pump(&mut SampledClock(origin), &mut sink)
            .presented(),
        "mount frame presents"
    );
    assert!(
        !runtime.scheduler().has_scheduled_frame(),
        "presentation quiesces"
    );

    let owner =
        AnimationController::builder(Duration::from_millis(100)).build_on(Some(&runtime.vsync()));
    assert!(
        !runtime.scheduler().has_scheduled_frame(),
        "an idle seat needs no frame"
    );
    owner.controller().forward().expect("fresh run");
    assert!(
        runtime.scheduler().has_scheduled_frame(),
        "accepted run wakes the driver before a pump"
    );

    runtime.set_now_secs_for_test(0.02);
    let _ = runtime.pump(
        &mut SampledClock(origin + Duration::from_millis(20)),
        &mut sink,
    );
    assert!(
        owner.controller().is_animating(),
        "first sample preserves the run"
    );
    assert!(
        runtime.needs_redraw(),
        "the running animation requests continuation"
    );
    runtime.set_now_secs_for_test(0.12);
    let _ = runtime.pump(
        &mut SampledClock(origin + Duration::from_millis(120)),
        &mut sink,
    );
    assert_eq!(owner.controller().value(), 1.0);
    assert!(
        !runtime.vsync().has_running(),
        "completed run releases frame demand"
    );
}

pub(crate) fn agent_playback_drives_independent_windows_and_one_paused_step_frame() {
    use flui_animation::{Animation as _, AnimationController};
    use flui_protocol::MotionRequest;
    use std::time::Duration;

    let mut runtime = mount_root_here();
    let a = runtime.presentation_id();
    let b = runtime.install_second_presentation_for_test();
    runtime
        .attach_root_widget_to_for_test(b, &flui_widgets::SizedBox::square(10.0))
        .expect("second root");
    let a_window = runtime.dev_agent_window(a).expect("A agent");
    let b_window = runtime.dev_agent_window(b).expect("B agent");
    let edit = |window: &flui_view::dev_agent::AgentWindow, request| {
        let mut answer = window.motion(request).expect("admitted");
        assert_eq!(runtime.drain_commands().invoked, 1);
        answer.try_take().expect("answered").expect("valid request")
    };
    let a_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&runtime.vsync()));
    let b_owner = AnimationController::builder(Duration::from_secs(1))
        .build_on(Some(&runtime.presentations.get(b).expect("B").vsync()));
    a_owner.controller().forward().expect("A run");
    b_owner.controller().forward().expect("B run");
    edit(&a_window, MotionRequest::new().with_rate(2.0));
    edit(&b_window, MotionRequest::new().with_rate(0.5));
    let mut sink = ScriptedSink::always_presents();
    runtime.set_now_secs_for_test(0.0);
    let _ = runtime.render_frame(&mut sink);
    runtime.set_now_secs_for_test(0.25);
    let _ = runtime.render_frame(&mut sink);
    assert!((a_owner.controller().value() - 0.5).abs() < 1e-9);
    assert!((b_owner.controller().value() - 0.125).abs() < 1e-9);

    edit(&a_window, MotionRequest::new().with_rate(0.0));
    runtime.set_now_secs_for_test(10.0);
    let _ = runtime.render_frame(&mut sink);
    assert_eq!(a_owner.controller().value(), 0.5);
    assert_eq!(b_owner.controller().value(), 1.0);
    edit(&b_window, MotionRequest::new().with_rate(0.0));
    runtime.set_now_secs_for_test(11.0);
    let _ = runtime.render_frame(&mut sink);
    let a_frames = runtime
        .presentations
        .get(a)
        .expect("A")
        .clock()
        .produced_count();
    let b_frames = runtime
        .presentations
        .get(b)
        .expect("B")
        .clock()
        .produced_count();
    edit(&a_window, MotionRequest::new().with_step_ms(100));
    let _ = runtime.render_frame(&mut sink);
    assert!((a_owner.controller().value() - 0.6).abs() < 1e-9);
    assert_eq!(
        runtime
            .presentations
            .get(a)
            .expect("A")
            .clock()
            .produced_count(),
        a_frames + 1
    );
    assert_eq!(
        runtime
            .presentations
            .get(b)
            .expect("B")
            .clock()
            .produced_count(),
        b_frames
    );
    runtime.set_now_secs_for_test(12.0);
    let _ = runtime.render_frame(&mut sink);
    assert_eq!(
        runtime
            .presentations
            .get(a)
            .expect("A")
            .clock()
            .produced_count(),
        a_frames + 1,
        "a paused run cannot keep producing after its explicit step"
    );
}

pub(crate) fn gated_presentations_hold_samples_then_catch_up_when_visible() {
    use flui_animation::{Animation as _, AnimationController};
    use std::time::Duration;

    for hidden in [true, false] {
        let runtime = mount_root_here();
        let owner =
            AnimationController::builder(Duration::from_secs(1)).build_on(Some(&runtime.vsync()));
        let controller = owner.controller();
        controller.forward().expect("fresh run");
        let mut sink = ScriptedSink::always_presents();
        runtime.set_now_secs_for_test(0.0);
        let _ = runtime.render_frame(&mut sink);
        runtime.set_now_secs_for_test(0.25);
        let _ = runtime.render_frame(&mut sink);
        assert!((controller.value() - 0.25).abs() < 1e-9);

        let mut scheduler = runtime.scheduler().clone();
        if hidden {
            runtime.set_presentation_hidden(runtime.presentation_id(), true);
        } else {
            scheduler.set_frames_enabled(false);
        }
        for time in [0.5, 1.0, 2.0] {
            runtime.set_now_secs_for_test(time);
            let _ = runtime.render_frame(&mut sink);
            assert!(
                (controller.value() - 0.25).abs() < 1e-9,
                "hidden={hidden}: gated frames cannot invoke the controller"
            );
            assert!(controller.is_animating());
        }

        if hidden {
            runtime.set_presentation_hidden(runtime.presentation_id(), false);
        } else {
            scheduler.set_frames_enabled(true);
        }
        runtime.set_now_secs_for_test(2.1);
        let _ = runtime.render_frame(&mut sink);
        assert_eq!(
            controller.value(),
            1.0,
            "visibility resumes at current timeline time"
        );
        assert!(!controller.is_animating());
    }
}

fn table_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_foundation::geometry::Size::new(800.0, 600.0))
}

/// A UI runtime with a minimal root widget already attached -- this
/// module's own copy: `frame_pipeline_and_vsync::mount_root` is
/// private to that sibling module.
fn mount_root_here() -> UiRuntime {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&flui_widgets::SizedBox::new(1.0, 1.0)))
        .expect("attach succeeds");
    ui_runtime
}

/// END-STATE INVARIANT: at N=1 -- a clock that is
/// never hidden and never backpressured -- the segment gate's
/// decision (`clock.poll().should_run_segment()`, fed `Dirty`
/// demand from `take_redraw_pending() || has_pending_work()`) is
/// equivalent to that same union read directly, the predicate it
/// replaces, over every reachable combination of the two inputs.
/// This table covers the NOT-deferred subspace; first-frame
/// deferral is a THIRD, orthogonal axis that does not perturb it —
/// `should_run_segment()` is `true` for both `Produce` and
/// `ProduceWithheld`, so a deferred presentation reaches the exact
/// same "did the segment run" answer as an undeferred one with the
/// same demand (see `segment_still_runs_by_the_same_predicate_while_
/// deferred` below for that companion proof; deferral only changes
/// whether `render_frame` may submit the result):
///
/// | woken | pending build | segment runs |
/// |-------|---------------|---------------|
/// | false | false         | false         |
/// | true  | false         | true          |
/// | false | true          | true          |
/// | true  | true          | true          |
pub(crate) fn segment_runs_iff_woken_or_has_pending_work_over_the_full_table() {
    for (case, woken, attach_root, expect_segment_runs) in [
        ("neither", false, false, false),
        ("woken_only", true, false, true),
        ("pending_work_only", false, true, true),
        ("both", true, true, true),
    ] {
        let ui_runtime = UiRuntime::for_test();
        if attach_root {
            ui_runtime
                .enter(|ui_runtime| {
                    ui_runtime.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
                })
                .expect("attach succeeds");
        }
        // `attach_root_widget` itself calls `request_redraw()`,
        // which marks the wake bit too -- normalize it to exactly
        // this case's own `woken` input so the two axes stay
        // independent, rather than whatever attaching happened to
        // leave behind.
        let _ = ui_runtime.presentations.primary().take_redraw_pending();
        if woken {
            ui_runtime.presentations.primary().mark_redraw_pending();
        }

        let _ = ui_runtime.enter(|ui_runtime| ui_runtime.draw_frame_entered(table_constraints()));

        assert_eq!(
            ui_runtime.presentations.primary().flush_count() == 1,
            expect_segment_runs,
            "case {case}: woken={woken} attach_root={attach_root} -- whether the \
             segment ran must equal woken || has_pending_work"
        );
    }
}

/// Edge-trigger discipline, end to end: N ticks
/// of a running controller that land while capacity never frees
/// must collapse into exactly one platform-facing wake, through
/// the REAL production call site — not just
/// `FrameClock::try_arm_redraw_request` in isolation (see
/// flui-scheduler's own unit test for that, at the bare-clock
/// level). A produce (capacity freed) must re-arm it.
///
/// Drives `render_frame`, not the bare `draw_frame_entered`
/// this test used before: the continuation-wake check now runs
/// AFTER `mark_rendered()`, inside `render_frame`, not
/// inside `draw_frame_entered`'s vsync loop — see that
/// method's own comment for why the move was necessary: raising
/// the wake before `mark_rendered()` let the SAME callback silently
/// clobber it). One consequence, pinned here rather than left
/// implicit: the callback that frees capacity and produces ALSO
/// re-arms and wakes again in that SAME callback now (the check
/// runs after `poll()` already reset the latch), not on a
/// SEPARATE, later callback the way it did when the check ran
/// before `poll()`.
pub(crate) fn n_ticks_under_backpressure_wake_the_platform_exactly_once_then_rearm() {
    use std::time::Duration;

    use flui_animation::AnimationController;

    let (wake, wake_count) = super::counting_wake();
    let ui_runtime = super::new_runtime(wake).expect("runtime");
    let mut owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&ui_runtime.vsync()));
    let controller = owner.controller();
    controller.forward().expect("fresh controller forwards");

    let clock = ui_runtime.presentations.primary().clock();
    clock.set_max_in_flight(1);
    clock.record_submit(); // at capacity for the whole loop below
    wake_count.store(0, Ordering::Relaxed);

    let mut backend = ScriptedSink::always_presents();

    for tick in 1..=5 {
        ui_runtime.set_now_secs_for_test(0.01 * f64::from(tick));
        let _ = ui_runtime.render_frame(&mut backend);
    }
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        1,
        "five ticks landing while capacity never frees must collapse into exactly \
         one platform-facing wake"
    );

    // Free capacity: THIS SAME callback's `render_frame`
    // now both produces (`poll()` grants a produce, clearing the
    // mask and the armed latch) AND re-arms a fresh wake for the
    // controller's continuation in its own post-render step --
    // the two are no longer split across two separate callbacks.
    clock.record_retire();
    ui_runtime.set_now_secs_for_test(0.10);
    let _ = ui_runtime.render_frame(&mut backend);
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        2,
        "the produce-and-continue callback re-arms in the SAME callback now that the \
         continuation check runs after mark_rendered(), which itself runs after \
         poll() already reset the latch"
    );

    owner.dispose();
}

// ================================================================
// Hidden-surface gating: `FrameClock::set_hidden` wired to
// `UiRuntime::set_presentation_hidden`, and the ungate->wake edge
// that keeps a demand mark from stranding the presentation.
// ================================================================

/// The liveness-critical ungate->wake edge: a demand mark that
/// arrives while hidden is retained by the mask but produces
/// nothing; unhiding must both (a) let the very next pump produce
/// exactly once, and (b) actually WAKE the platform loop itself --
/// not merely be observable if some unrelated caller happens to
/// pump again. Without the unconditional wake in
/// `set_presentation_hidden`, this demand would strand: nothing
/// self-wakes an idle `ControlFlow::Wait` loop.
pub(crate) fn occlude_then_dirty_then_unocclude_wakes_exactly_once_and_produces_exactly_once() {
    let (wake, wake_count) = super::counting_wake();
    let ui_runtime = super::new_runtime(wake).expect("runtime");
    ui_runtime
        .enter(|ui_runtime| ui_runtime.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("attach succeeds");
    let presentation_id = ui_runtime.presentations.primary().id();
    let mut backend = ScriptedSink::always_presents();

    // Settle the attach's own first paint.
    ui_runtime.set_now_secs_for_test(0.0);
    let _ = ui_runtime.render_frame(&mut backend);
    let produced_before = ui_runtime.presentations.primary().clock().produced_count();
    let submits_before = backend.submit_calls;

    ui_runtime.set_presentation_hidden(presentation_id, true);
    wake_count.store(0, Ordering::Relaxed);

    // Dirty the real render tree once while hidden -- a genuine
    // repaint demand, not just the wake-only redraw bit, so the
    // eventual unhide pump below actually has something to paint.
    // This ALSO fires the render pipeline's own pre-existing wake
    // wire (`PipelineOwner::set_on_need_visual_update` -> the
    // scheduler's `ensure_visual_update` -> the `on_frame_scheduled`
    // hook, the ui_runtime's `wake` -- unrelated to this slice's FrameClock
    // gate) exactly once for this clean->dirty transition -- expected,
    // not the property under test. What IS under test is the DELTA at
    // unhide below: one MORE wake beyond whatever this mark already
    // caused.
    ui_runtime
        .presentations
        .primary()
        .pipeline()
        .with_mut(|owner| {
            if let Some(root_id) = owner.root_id() {
                owner.mark_needs_paint(root_id);
            }
        });

    for pump in 1..=3 {
        ui_runtime.set_now_secs_for_test(0.016 * f64::from(pump));
        let presented = ui_runtime.render_frame(&mut backend);
        assert!(!presented, "pump {pump}: must not present while hidden");
    }
    assert_eq!(
        ui_runtime.presentations.primary().clock().produced_count(),
        produced_before,
        "zero produces while hidden, however many times pumped"
    );
    assert_eq!(
        backend.submit_calls, submits_before,
        "zero submits while hidden"
    );
    let wake_count_before_unhide = wake_count.load(Ordering::Relaxed);

    // Unocclude: the retained demand must wake the platform loop
    // exactly one MORE time (the ungate edge) beyond whatever the
    // dirty mark's own independent wake already contributed...
    ui_runtime.set_presentation_hidden(presentation_id, false);
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wake_count_before_unhide + 1,
        "unhiding with retained demand must wake the platform loop exactly once more"
    );

    // ...and produce exactly once on the very next pump, with no
    // further external input.
    ui_runtime.set_now_secs_for_test(0.1);
    let presented = ui_runtime.render_frame(&mut backend);
    assert!(presented, "the unhide pump must present");
    assert_eq!(
        ui_runtime.presentations.primary().clock().produced_count(),
        produced_before + 1,
        "exactly one produce after unhiding -- no lost frame, no extra one"
    );
    assert_eq!(
        backend.submit_calls,
        submits_before + 1,
        "exactly one submit after unhiding"
    );

    // Settling: a further pump with nothing newly dirtied produces
    // nothing more.
    ui_runtime.set_now_secs_for_test(0.2);
    let _ = ui_runtime.render_frame(&mut backend);
    assert_eq!(
        ui_runtime.presentations.primary().clock().produced_count(),
        produced_before + 1,
        "no further produce once the retained demand is consumed"
    );
}

// ------------------------------------------------------------------
// Frame telemetry: input->present attribution, driven through the
// REAL production sequence (`handle_input_addressed` ->
// `render_frame`), never `FrameClock`'s own methods called
// directly — the lesson this codebase has already paid for once in
// this same area (see `render_frame`'s own doc on why a
// production-sequence probe caught what calling `draw_frame_entered`
// directly could not).
// ------------------------------------------------------------------

/// Finding: on `SurfaceLost`, `record_submit_telemetry` used to
/// unconditionally DRAIN this presentation's pending input epochs
/// into the failed attempt's own `Errored` snapshot — but
/// `render_frame` arms a retry for exactly this outcome
/// (`retry_needed = true` -> `wake_frame()`), so the frame that
/// eventually reaches the screen would find the epoch buffer
/// already empty and carry no attribution at all. Drives the exact
/// sequence the finding names: input -> `SurfaceLost` -> retry ->
/// the presented frame still carries the original epoch.
pub(crate) fn surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerKind, make_down_event};

    for verdict in [SubmitVerdict::SurfaceStale, SubmitVerdict::Retry] {
        let ui_runtime = mount_root_here();
        let primary_id = ui_runtime.presentations.primary().id();

        let down = make_down_event(Offset::new(0.0, 0.0), PointerKind::Mouse)
            .expect("finite test position");
        ui_runtime.enter(|ui_runtime| {
            ui_runtime.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
        });

        // First attempt fails temporarily. A retry is armed, and the
        // failed attempt's own snapshot must still see the epoch that
        // was pending (diagnostic value), but must NOT consume it.
        let mut failing_backend = ScriptedSink::new(move |_, _| verdict);
        let presented = ui_runtime.render_frame(&mut failing_backend);
        assert!(!presented, "a failed attempt never reaches present()");
        let after_failure = ui_runtime
            .presentations
            .primary()
            .clock()
            .frames_since(None);
        assert_eq!(
            after_failure.len(),
            1,
            "the failed attempt is still recorded, for diagnostics"
        );
        assert_eq!(
            after_failure[0].latencies().count(),
            1,
            "the failed attempt's own snapshot still reports the pending epoch"
        );

        assert!(
            ui_runtime.needs_redraw(),
            "retry must wake without new input"
        );
        let mut succeeding_backend = ScriptedSink::always_presents();
        let presented = ui_runtime.render_frame(&mut succeeding_backend);
        assert!(presented, "the retry must actually reach present()");

        let after_retry = ui_runtime
            .presentations
            .primary()
            .clock()
            .frames_since(None);
        assert_eq!(
            after_retry.len(),
            2,
            "the retry adds a second snapshot alongside the failed attempt's"
        );
        let retry_snapshot = &after_retry[1];
        assert_eq!(
            retry_snapshot.present_outcome,
            flui_scheduler::PresentOutcome::Presented
        );
        assert_eq!(
            retry_snapshot.latencies().count(),
            1,
            "the presented retry frame must carry the ORIGINAL input epoch -- retaining \
             it across the failed attempt must not have lost it"
        );
    }
}
