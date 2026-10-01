use super::*;

fn table_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_foundation::geometry::Size::new(800.0, 600.0))
}

/// A realm with a minimal root widget already attached -- this
/// module's own copy: `frame_pipeline_and_vsync::mount_root` is
/// private to that sibling module.
fn mount_root_here() -> UiRealm {
    let realm = UiRealm::for_test();
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(1.0, 1.0)))
        .expect("attach succeeds");
    realm
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
        let realm = UiRealm::for_test();
        if attach_root {
            realm
                .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
                .expect("attach succeeds");
        }
        // `attach_root_widget` itself calls `request_redraw()`,
        // which marks the wake bit too -- normalize it to exactly
        // this case's own `woken` input so the two axes stay
        // independent, rather than whatever attaching happened to
        // leave behind.
        let _ = realm.presentations.primary().take_redraw_pending();
        if woken {
            realm.presentations.primary().mark_redraw_pending();
        }

        let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

        assert_eq!(
            realm.presentations.primary().flush_count() == 1,
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
    let realm = super::new_runtime(wake).expect("runtime");
    let controller = AnimationController::new(
        Duration::from_secs(1),
        &flui_scheduler::UpdateScheduler::new(),
    );
    realm.vsync().register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    let clock = realm.presentations.primary().clock();
    clock.set_max_in_flight(1);
    clock.record_submit(); // at capacity for the whole loop below
    wake_count.store(0, Ordering::Relaxed);

    let mut backend = ScriptedSink::always_presents();

    for tick in 1..=5 {
        realm.set_now_secs_for_test(0.01 * f64::from(tick));
        let _ = realm.render_frame(&mut backend);
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
    realm.set_now_secs_for_test(0.10);
    let _ = realm.render_frame(&mut backend);
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        2,
        "the produce-and-continue callback re-arms in the SAME callback now that the \
         continuation check runs after mark_rendered(), which itself runs after \
         poll() already reset the latch"
    );

    controller.dispose();
}

// ================================================================
// Hidden-surface gating: `FrameClock::set_hidden` wired to
// `UiRealm::set_presentation_hidden`, and the ungate->wake edge
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
    let realm = super::new_runtime(wake).expect("runtime");
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("attach succeeds");
    let presentation_id = realm.presentations.primary().id();
    let mut backend = ScriptedSink::always_presents();

    // Settle the attach's own first paint.
    realm.set_now_secs_for_test(0.0);
    let _ = realm.render_frame(&mut backend);
    let produced_before = realm.presentations.primary().clock().produced_count();
    let submits_before = backend.submit_calls;

    realm.set_presentation_hidden(presentation_id, true);
    wake_count.store(0, Ordering::Relaxed);

    // Dirty the real render tree once while hidden -- a genuine
    // repaint demand, not just the wake-only redraw bit, so the
    // eventual unhide pump below actually has something to paint.
    // This ALSO fires the render pipeline's own pre-existing wake
    // wire (`PipelineOwner::set_on_need_visual_update` -> the
    // scheduler's `ensure_visual_update` -> the `on_frame_scheduled`
    // hook, the realm's `wake` -- unrelated to this slice's FrameClock
    // gate) exactly once for this clean->dirty transition -- expected,
    // not the property under test. What IS under test is the DELTA at
    // unhide below: one MORE wake beyond whatever this mark already
    // caused.
    realm.presentations.primary().pipeline().with_mut(|owner| {
        if let Some(root_id) = owner.root_id() {
            owner.mark_needs_paint(root_id);
        }
    });

    for pump in 1..=3 {
        realm.set_now_secs_for_test(0.016 * f64::from(pump));
        let presented = realm.render_frame(&mut backend);
        assert!(!presented, "pump {pump}: must not present while hidden");
    }
    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
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
    realm.set_presentation_hidden(presentation_id, false);
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wake_count_before_unhide + 1,
        "unhiding with retained demand must wake the platform loop exactly once more"
    );

    // ...and produce exactly once on the very next pump, with no
    // further external input.
    realm.set_now_secs_for_test(0.1);
    let presented = realm.render_frame(&mut backend);
    assert!(presented, "the unhide pump must present");
    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
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
    realm.set_now_secs_for_test(0.2);
    let _ = realm.render_frame(&mut backend);
    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
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
    use flui_interaction::events::{PointerType, make_down_event};

    for verdict in [SubmitVerdict::SurfaceStale, SubmitVerdict::Retry] {
        let realm = mount_root_here();
        let primary_id = realm.presentations.primary().id();

        let down = make_down_event(Offset::new(0.0, 0.0), PointerType::Mouse);
        realm.enter(|realm| {
            realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
        });

        // First attempt fails temporarily. A retry is armed, and the
        // failed attempt's own snapshot must still see the epoch that
        // was pending (diagnostic value), but must NOT consume it.
        let mut failing_backend = ScriptedSink::new(move |_, _| verdict);
        let presented = realm.render_frame(&mut failing_backend);
        assert!(!presented, "a failed attempt never reaches present()");
        let after_failure = realm.presentations.primary().clock().frames_since(None);
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

        assert!(realm.needs_redraw(), "retry must wake without new input");
        let mut succeeding_backend = ScriptedSink::always_presents();
        let presented = realm.render_frame(&mut succeeding_backend);
        assert!(presented, "the retry must actually reach present()");

        let after_retry = realm.presentations.primary().clock().frames_since(None);
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
