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
#[test]
fn segment_runs_iff_woken_or_has_pending_work_over_the_full_table() {
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

/// Close-mid-animation probe: a presentation closing
/// while a controller registered on ITS OWN `Vsync` is still running
/// must not panic, must stop ticking that controller immediately
/// (its registry dies with the presentation), and must leave a
/// SIBLING presentation's own clock producing exactly as before.
#[test]
fn closing_a_presentation_mid_animation_stops_its_ticks_without_panicking_or_touching_the_sibling()
{
    use std::time::Duration;

    use flui_animation::{Animation, AnimationController};

    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    let controller = AnimationController::new(
        Duration::from_millis(200),
        &flui_scheduler::UpdateScheduler::new(),
    );
    {
        let b = realm.presentations.get(b_id).expect("B installed");
        b.vsync().register(controller.clone());
    }
    controller.forward().expect("fresh controller forwards");

    realm.set_now_secs_for_test(0.0);
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    realm.set_now_secs_for_test(0.05);
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    let value_before_close = controller.value();
    assert!(
        value_before_close > 0.0,
        "sanity: the controller must have advanced before B closes"
    );

    // A produces independently of B -- establish the baseline this
    // probe checks survives B's teardown.
    let a = realm.presentations.get(a_id).expect("A installed");
    a.mark_redraw_pending();
    let a_produced_before = a.clock().produced_count();
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .clock()
            .produced_count()
            > a_produced_before,
        "sanity: A's own clock must produce independently of B before the close"
    );

    assert!(
        realm.close_presentation_entered(b_id),
        "B must have been installed and removable -- no panic reaching this line \
         is itself part of the probe"
    );

    // B is gone from the forest; nothing ticks its controller
    // further, no matter how far the virtual clock advances.
    realm.set_now_secs_for_test(0.5);
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert_eq!(
        controller.value(),
        value_before_close,
        "B's own Vsync registry died with its presentation -- the controller must \
         not be ticked any further, not even to completion"
    );

    // A's own clock is unaffected by B's teardown.
    let a = realm.presentations.get(a_id).expect("A installed");
    a.mark_redraw_pending();
    let a_produced_before_second = a.clock().produced_count();
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .clock()
            .produced_count()
            > a_produced_before_second,
        "A's own clock must keep producing normally after B's presentation closes"
    );

    controller.dispose();
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
#[test]
fn n_ticks_under_backpressure_wake_the_platform_exactly_once_then_rearm() {
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

/// END-STATE INVARIANT (the demand-driven-idle headline):
/// with an empty demand mask and no gating, `poll` returns
/// `Skip(NoDemand)` every pump and the segment never runs -- zero
/// produces, zero pipeline flushes -- no matter how many times the
/// realm is pumped for purely logical reasons (no attach, no
/// redraw request, nothing ever dirtied).
#[test]
fn an_idle_presentation_produces_and_flushes_exactly_zero_over_many_pumps() {
    let realm = UiRealm::for_test();

    for pump in 0..50 {
        realm.set_now_secs_for_test(f64::from(pump) * 0.016);
        let (_, outcome, _) = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
        assert!(
            matches!(outcome, FramePaintOutcome::Idle),
            "pump {pump}: an idle presentation must never produce anything but Idle"
        );
    }

    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
        0,
        "an idle presentation's clock must grant zero produces over any number of pumps"
    );
    assert_eq!(
        realm.presentations.primary().flush_count(),
        0,
        "an idle presentation's segment must never run -- zero pipeline flushes"
    );
}

// ================================================================
// Hidden-surface gating: `FrameClock::set_hidden` wired to
// `UiRealm::set_presentation_hidden`, and the ungate->wake edge
// that keeps a demand mark from stranding the presentation.
// ================================================================

/// END-STATE INVARIANT: a hidden presentation's segment never runs
/// and its backend never receives a submit, however many times it
/// is dirtied and pumped.
#[test]
fn gated_presentation_produces_zero_segments_and_zero_submits_while_hidden() {
    let realm = mount_root_here();
    let presentation_id = realm.presentations.primary().id();
    let mut backend = ScriptedSink::always_presents();

    // Settle the initial attach paint first, so the assertions below
    // isolate exactly what happens WHILE hidden -- baselines
    // captured AFTER settling, not absolute zero (the settle itself
    // is one legitimate submit).
    let _ = realm.render_frame(&mut backend);
    realm.set_presentation_hidden(presentation_id, true);
    let flush_count_before = realm.presentations.primary().flush_count();
    let produced_before = realm.presentations.primary().clock().produced_count();
    let submits_before = backend.submit_calls;

    for pump in 0..10 {
        realm.set_now_secs_for_test(f64::from(pump) * 0.016);
        // Dirty the tree every pump -- demand exists, but the
        // presentation-level gate must still withhold the segment.
        realm.presentations.primary().pipeline().with_mut(|owner| {
            if let Some(root_id) = owner.root_id() {
                owner.mark_needs_paint(root_id);
            }
        });
        let presented = realm.render_frame(&mut backend);
        assert!(
            !presented,
            "pump {pump}: a hidden presentation must never present"
        );
    }

    assert_eq!(
        realm.presentations.primary().flush_count(),
        flush_count_before,
        "a hidden presentation's segment must never run, however often it is dirtied"
    );
    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
        produced_before,
        "a hidden presentation must never grant a produce"
    );
    assert_eq!(
        backend.submit_calls, submits_before,
        "a hidden presentation must never reach the raster backend -- zero GPU submits"
    );
}

/// The liveness-critical ungate->wake edge: a demand mark that
/// arrives while hidden is retained by the mask but produces
/// nothing; unhiding must both (a) let the very next pump produce
/// exactly once, and (b) actually WAKE the platform loop itself --
/// not merely be observable if some unrelated caller happens to
/// pump again. Without the unconditional wake in
/// `set_presentation_hidden`, this demand would strand: nothing
/// self-wakes an idle `ControlFlow::Wait` loop.
#[test]
fn occlude_then_dirty_then_unocclude_wakes_exactly_once_and_produces_exactly_once() {
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

/// The ungate->wake edge's OWN defining hazard, discriminated: the
/// unhide branch must wake on RETAINED DEMAND, not on
/// `try_arm_redraw_request`'s own latch state -- and the two only
/// disagree when the latch was armed BEFORE the hide (from an
/// earlier, unrelated produce) and never consumed since, because
/// nothing polled while hidden. Set up exactly that ordering: a
/// visible pump first arms the latch (a running controller's own
/// continuation-wake, `render_frame`'s tail) and leaves
/// demand retained (the controller keeps running), THEN hide, THEN
/// unhide with no further pump in between. A version gated on
/// `try_arm_redraw_request()` instead of `demand_mask().is_empty()`
/// finds the latch ALREADY armed (from the visible pump) and
/// silently declines to wake -- stranding the presentation despite
/// genuinely retained demand.
#[test]
fn unhide_wakes_on_retained_demand_even_when_the_latch_was_armed_before_the_hide() {
    use std::time::Duration;

    use flui_animation::AnimationController;

    let (wake, wake_count) = super::counting_wake();
    let realm = super::new_runtime(wake).expect("runtime");
    let presentation_id = realm.presentations.primary().id();
    let mut backend = ScriptedSink::always_presents();

    let controller = AnimationController::new(
        Duration::from_secs(1),
        &flui_scheduler::UpdateScheduler::new(),
    );
    realm.vsync().register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    // One VISIBLE pump: the segment produces off the controller's
    // own Animation demand, and render_frame's tail --
    // since the controller is still running -- marks a FRESH
    // Animation demand for the next pump and arms
    // try_arm_redraw_request's latch (the ONLY production call
    // site of that method). After this call: demand mask is
    // nonempty (Animation, retained), and the latch is armed
    // (true) -- both facts this test's ordering depends on.
    realm.set_now_secs_for_test(0.0);
    let _ = realm.render_frame(&mut backend);
    assert_eq!(
        realm.presentations.primary().clock().demand_mask(),
        flui_scheduler::DemandMask::ANIMATION,
        "precondition: the running controller must leave Animation demand retained"
    );

    // Hide with NO further pump -- the latch stays armed from the
    // visible pump above; nothing consumes it while hidden.
    realm.set_presentation_hidden(presentation_id, true);
    wake_count.store(0, Ordering::Relaxed);

    // Unhide: retained demand is still nonempty, so this must wake
    // -- regardless of the latch's own (already-armed, therefore
    // falsely "nothing to arm") state.
    realm.set_presentation_hidden(presentation_id, false);

    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        1,
        "unhiding with retained demand must wake even when \
         try_arm_redraw_request's latch was already armed before the hide -- a \
         latch-gated implementation would find it pre-armed and stay silent, \
         stranding the presentation"
    );

    controller.dispose();
}

/// `UiRealm::next_wake`'s own min-over-presentations level,
/// discriminated. A realm supports N resident presentations even
/// though secondary widget content and frame submission remain
/// unwired, so a cross-realm-only mutant test cannot see this level.
/// With a far deadline on the primary and a near one on a secondary,
/// the aggregate must reflect the near one regardless of which
/// presentation is primary.
#[test]
fn next_wake_is_the_min_deadline_across_two_presentations_of_one_realm() {
    // `web_time::Instant`, not std's: on wasm32 they are DIFFERENT
    // types (std's panics there, so the deadline machinery uses
    // web-time's `performance.now()`-backed one), and comparing a
    // std instant against what `next_wake`/`next_deadline` return
    // does not compile for that target. Identical on native, which
    // is why this only surfaced once the lib-test target was built
    // for wasm32.
    use web_time::{Duration, Instant};

    use flui_interaction::{
        GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
    };

    let mut realm = UiRealm::for_test();
    let b_id = realm.install_second_presentation_for_test();
    let a_id = realm.presentations.primary().id();

    let arena_a = realm
        .presentations
        .get(a_id)
        .unwrap()
        .gestures()
        .arena()
        .clone();
    let recognizer_a = LongPressGestureRecognizer::with_settings(
        arena_a,
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_secs(5)),
    )
    .with_on_long_press(|| {});
    recognizer_a.add_pointer(
        PointerId::new(2).expect("nonzero pointer id"),
        flui_foundation::geometry::Offset::new(10.0, 10.0),
        flui_foundation::geometry::Offset::new(10.0, 10.0),
    );

    let before_b = Instant::now();
    let arena_b = realm
        .presentations
        .get(b_id)
        .unwrap()
        .gestures()
        .arena()
        .clone();
    let recognizer_b = LongPressGestureRecognizer::with_settings(
        arena_b,
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(50)),
    )
    .with_on_long_press(|| {});
    recognizer_b.add_pointer(
        PointerId::new(3).expect("nonzero pointer id"),
        flui_foundation::geometry::Offset::new(20.0, 20.0),
        flui_foundation::geometry::Offset::new(20.0, 20.0),
    );

    let next_wake = realm
        .next_wake()
        .expect("two armed deadlines pending -- next_wake must be Some");
    let until_wake = next_wake.saturating_duration_since(before_b);
    assert!(
        until_wake < Duration::from_secs(1),
        "next_wake must reflect presentation B's near (50ms) deadline, not A's far \
         (5s) one, even though A is the primary -- got {until_wake:?} until wake"
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

/// A dispatched pointer event's arrival survives into the exported
/// `FrameSnapshot` with a bounded, real, non-zero latency — value,
/// not merely presence. Kills "epoch field exists but is zero/None"
/// and "latency is a distribution with no attribution".
#[test]
fn dispatched_input_is_attributed_end_to_end_in_the_exported_frame_record() {
    use std::thread;
    use std::time::Duration;

    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerType, make_down_event};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();
    assert_eq!(
        realm
            .presentations
            .primary()
            .clock()
            .frames_since(None)
            .len(),
        0,
        "precondition: nothing recorded before this test's own dispatch+produce"
    );

    let down = make_down_event(Offset::new(4.0, 6.0), PointerType::Mouse);
    let before_dispatch = std::time::Instant::now();
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
    });

    // A measurable, non-flaky real gap between dispatch and produce.
    thread::sleep(Duration::from_millis(5));

    let mut backend = ScriptedSink::always_presents();
    let presented = realm.render_frame(&mut backend);
    assert!(
        presented,
        "precondition: the frame actually reached present()"
    );
    let wall_clock_elapsed = before_dispatch.elapsed();

    let snapshots = realm.presentations.primary().clock().frames_since(None);
    assert_eq!(snapshots.len(), 1, "exactly one frame must be recorded");
    let latencies: Vec<_> = snapshots[0].latencies().collect();
    assert_eq!(
        latencies.len(),
        1,
        "the dispatched input's epoch must survive into the exported record"
    );
    let (_, latency) = latencies[0];
    assert!(
        latency >= Duration::from_millis(5),
        "attributed latency must be at least the real sleep between dispatch and \
         present -- got {latency:?}"
    );
    assert!(
        latency <= wall_clock_elapsed + Duration::from_millis(200),
        "attributed latency ({latency:?}) must not exceed the real wall-clock time \
         actually elapsed since dispatch ({wall_clock_elapsed:?}) by more than \
         measurement slop -- a stray wall-clock read from an unrelated instant would \
         blow this bound"
    );
}

/// `Skip(Backpressure)` vs `frames_dropped`, pinned at the REAL
/// production path: a clock-level backpressure episode (simulated
/// here since no production caller saturates `FrameClock`'s
/// in-flight counter yet -- see `FrameClock::record_submit`'s own
/// doc) must leave `UiRealm::frames_dropped` untouched, while a
/// GENUINE submit failure through `render_frame` (a real
/// `SurfaceLost`) must increment it. Two different mechanisms,
/// asserted against the same realm, so a mutant that folds either
/// counter into the other is caught either way.
#[test]
fn backpressure_episode_is_not_counted_as_a_dropped_frame_but_a_real_submit_error_is() {
    let realm = mount_root_here();
    let presentation = realm.presentations.primary();

    // Saturate capacity so the very next poll defers rather than
    // produces -- `has_capacity` requires `in_flight < max`.
    presentation.clock().set_max_in_flight(1);
    presentation.clock().record_submit();
    presentation.mark_redraw_pending();

    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

    assert_eq!(
        presentation.clock().backpressure_deferrals(),
        1,
        "the saturated clock must defer this pump's demand"
    );
    assert_eq!(
        realm.frames_dropped(),
        0,
        "a backpressure deferral must never be counted as a dropped frame"
    );

    // Free capacity so the retained demand can actually flow
    // through a real submit attempt next.
    presentation.clock().record_retire();
    presentation.mark_redraw_pending();

    let mut backend = ScriptedSink::new(|_, _| SubmitVerdict::SurfaceStale);
    let presented = realm.render_frame(&mut backend);

    assert!(!presented, "SurfaceLost never reaches present()");
    assert_eq!(
        realm.frames_dropped(),
        1,
        "a genuine submit failure must increment frames_dropped exactly once"
    );
    assert_eq!(
        presentation.clock().backpressure_deferrals(),
        1,
        "the real submit failure must not also inflate the deferral counter -- \
         the two stay structurally separate"
    );
}

/// Multi-presentation attribution, red-exploit for the bug where
/// `record_submit_telemetry` hardcoded `primary()`: on a pump where
/// the PRIMARY's segment is skipped (nothing dirty) and a SECONDARY's
/// segment produces, the recorded `FrameSnapshot` must land on the
/// SECONDARY's own clock, carrying THIS pump's own fresh segment
/// span — never the primary's, and never a stale span the primary
/// itself latched on an earlier pump. Reproduces the production
/// topology `runner.rs`'s `install_presentation_alongside` (behind
/// `open_secondary_window`) creates: more than one presentation
/// resident in one realm.
///
/// Before the fix, this failed two ways at once: A gained a SECOND,
/// wrongly-attributed snapshot (guarded only by A's own
/// `last_segment_span().is_some()`, which stayed `Some` forever
/// once pump 1 set it — proving "a segment ran on SOME pump", not
/// "this pump"), with a `frame_id` colliding with pump 1's own
/// (A's `produced_count` never advanced this pump), while B — the
/// presentation that actually produced — recorded nothing at all.
#[test]
fn a_pump_where_the_primary_skips_and_a_secondary_produces_attributes_telemetry_to_the_secondary_not_the_stale_primary()
 {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    // Pump 1: only A has content. A produces and submits,
    // establishing a REAL segment span and one recorded
    // `FrameSnapshot` on A's own clock — the exact state a latched-
    // forever span could later be misread as "still valid" once it
    // goes stale.
    realm
        .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
        .expect("A attaches");
    let mut backend = ScriptedSink::always_presents();
    assert!(
        realm.render_frame(&mut backend),
        "pump 1: A alone must produce and present"
    );
    let a_snapshots_after_pump_1 = realm
        .presentations
        .get(a_id)
        .expect("A installed")
        .clock()
        .frames_since(None);
    assert_eq!(
        a_snapshots_after_pump_1.len(),
        1,
        "sanity: A's own first pump must record exactly one snapshot"
    );

    // Pump 2: A is now settled (its segment will be SKIPPED this
    // pump — nothing dirty), B gets real content attached and
    // produces instead.
    realm
        .attach_root_widget_to_for_test(b_id, &flui_widgets::SizedBox::new(20.0, 20.0))
        .expect("B attaches");
    assert!(
        realm.render_frame(&mut backend),
        "pump 2: B alone must produce and present"
    );

    let a = realm.presentations.get(a_id).expect("A installed");
    let b = realm.presentations.get(b_id).expect("B installed");

    let a_snapshots_after_pump_2 = a.clock().frames_since(None);
    assert_eq!(
        a_snapshots_after_pump_2.len(),
        1,
        "A must NOT gain a second, wrongly-attributed snapshot from B's own pump"
    );
    assert_eq!(
        a_snapshots_after_pump_2[0].frame_id, a_snapshots_after_pump_1[0].frame_id,
        "A's own recorded snapshot must be byte-for-byte untouched by B's pump"
    );

    let b_snapshots = b.clock().frames_since(None);
    assert_eq!(
        b_snapshots.len(),
        1,
        "B's own pump must record exactly one snapshot, on B's own clock"
    );
    assert_eq!(
        b_snapshots[0].presentation, b_id,
        "the recorded snapshot must name B, not A, as its producer"
    );
}

/// More than `MAX_COALESCED_INPUT_EPOCHS` raw pointer events
/// dispatched before a single produce — routine for any drag
/// against a high-frequency device, not an edge case (the
/// `stamp_input_epoch` call in `handle_input_addressed` fires
/// before `GestureBinding::flush_pending_moves` coalesces anything,
/// so telemetry sees the RAW dispatch stream). The exported record
/// must retain the NEWEST arrivals, not misattribute the oldest,
/// longest-stale ones as if they were still representative of what
/// this frame carried.
#[test]
fn more_than_max_coalesced_inputs_before_one_produce_keeps_the_newest_arrivals() {
    use std::thread;
    use std::time::Duration;

    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerType, make_move_event};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let dispatched = flui_scheduler::MAX_COALESCED_INPUT_EPOCHS + 4;
    for i in 0..dispatched {
        let position = Offset::new(i as f64, i as f64);
        let event = make_move_event(position, PointerType::Mouse);
        realm.enter(|realm| {
            realm.handle_input_addressed(primary_id, PlatformInput::Pointer(event));
        });
        thread::sleep(Duration::from_micros(200));
    }

    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));

    let snapshots = realm.presentations.primary().clock().frames_since(None);
    assert_eq!(snapshots.len(), 1);
    let mut latencies: Vec<_> = snapshots[0].latencies().collect();
    assert_eq!(
        latencies.len(),
        flui_scheduler::MAX_COALESCED_INPUT_EPOCHS,
        "the ring must retain exactly its bounded capacity, never silently grow"
    );
    latencies.sort_by_key(|(id, _)| id.get());
    let expected_first_kept = (dispatched - flui_scheduler::MAX_COALESCED_INPUT_EPOCHS) as u64;
    let ids: Vec<u64> = latencies.iter().map(|(id, _)| id.get()).collect();
    assert_eq!(
        ids,
        (expected_first_kept..dispatched as u64).collect::<Vec<_>>(),
        "must keep the newest-dispatched inputs, not the oldest -- the discard \
         policy this test pins"
    );
}

/// Finding: on `SurfaceLost`, `record_submit_telemetry` used to
/// unconditionally DRAIN this presentation's pending input epochs
/// into the failed attempt's own `Errored` snapshot — but
/// `render_frame` arms a retry for exactly this outcome
/// (`retry_needed = true` -> `wake_frame()`), so the frame that
/// eventually reaches the screen would find the epoch buffer
/// already empty and carry no attribution at all. Drives the exact
/// sequence the finding names: input -> `SurfaceLost` -> retry ->
/// the presented frame still carries the original epoch.
#[test]
fn surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{PointerType, make_down_event};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let down = make_down_event(Offset::new(0.0, 0.0), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
    });

    // First attempt: the surface is lost. A retry is armed, and the
    // failed attempt's own snapshot must still see the epoch that
    // was pending (diagnostic value), but must NOT consume it.
    let mut failing_backend = ScriptedSink::new(|_, _| SubmitVerdict::SurfaceStale);
    let presented = realm.render_frame(&mut failing_backend);
    assert!(!presented, "SurfaceLost never reaches present()");
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

    // Retry: as of #637's fix, `render_frame`'s own
    // `retry_needed` arm already marks the root needs-paint (see
    // `mark_primary_needs_full_repaint`), so this hand mark is no
    // longer load-bearing for getting `render_scene` reached again
    // -- `a_mid_frame_submit_failure_retry_actually_reaches_
    // render_scene_again_on_a_static_tree` covers THAT invariant
    // without any hand-dirtying at all. This mark stays here to
    // isolate a DIFFERENT invariant this test is actually about
    // (epoch attribution across the retry), standing in for the
    // genuinely new external cause (input, animation, resize) a
    // real driver loop would eventually supply on top of the
    // fix's own repaint.
    let primary = realm.presentations.primary();
    primary.renderer().root_pipeline_owner().with_mut(|owner| {
        if let Some(root_id) = owner.root_id() {
            owner.mark_needs_layout(root_id);
        }
    });
    primary.mark_redraw_pending();
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

/// Issue #637's own oracle requirement: the epoch-preservation pins
/// above (`surface_lost_retry_preserves_...` and its two siblings)
/// dirty the tree BY HAND after the scripted failure
/// (`owner.mark_needs_layout` + `primary.mark_redraw_pending()`),
/// standing in for whatever external cause would normally re-dirty
/// it. That stand-in is exactly why they kept passing while the
/// PRODUCTION retry path was silently a no-op: `wake_frame()` alone
/// re-opens `draw_frame_entered`'s per-presentation segment gate,
/// but never touches `PipelineOwner`'s own independent dirty
/// tracking, so an otherwise-unchanged (static) tree produces
/// `FramePaintOutcome::Idle` on the retry pump and `render_scene` is
/// never reached again.
///
/// This test dirties nothing itself, ever — not before the failure,
/// not between the failure and the retry. Sequence: mount (the
/// mount's own attach genuinely leaves build/layout dirty — nothing
/// hand-poked), pump ONCE against a backend scripted to fail that
/// very first submit (so the pipeline's build/layout/paint work is
/// genuinely consumed producing real content, even though the
/// submit of that content then fails), then pump again with no
/// intervening dirtying of any kind. The static tree between the
/// failure and the retry is the whole point: on `main`, nothing
/// re-arms the pipeline's own dirty tracking after a submit
/// failure, so this second pump finds nothing dirty and never
/// reaches `render_scene` again. The assertion is a `render_scene`
/// call counter, not `is_ok()` or `needs_redraw()` — either of
/// which a no-op retry (gate reopened, pipeline never touched)
/// would also satisfy.
#[test]
fn a_mid_frame_submit_failure_retry_actually_reaches_render_scene_again_on_a_static_tree() {
    let realm = mount_root_here();

    // Pump 1: the mount's own dirty state is genuinely consumed --
    // build, layout, and paint all run, producing real content --
    // but the submit of that content fails.
    let mut backend = ScriptedSink::fails_once_then_presents(SubmitVerdict::SurfaceStale);
    let presented = realm.render_frame(&mut backend);
    assert!(!presented, "SurfaceLost never reaches present()");
    assert_eq!(
        backend.submit_calls, 1,
        "precondition: the failing pump actually reached render_scene once"
    );
    assert!(realm.needs_redraw(), "the failure must arm a retry");

    // Pump 2, the retry -- driven by the same wake this test never
    // hand-dirties around. The tree is unchanged (static) since the
    // failure: whatever reaches render_scene again must come from
    // the failure arm's own fix, not from anything this test did.
    let presented = realm.render_frame(&mut backend);
    assert!(presented, "the retry must actually reach present()");
    assert_eq!(
        backend.submit_calls, 2,
        "the retry must reach render_scene a SECOND time -- wake_frame() alone \
         re-opens the segment gate but leaves PipelineOwner's own dirty tracking \
         untouched, so an unmodified static tree produces Idle and render_scene is \
         never called again"
    );
}

/// Regression for a review finding on this fix's own first draft: an
/// earlier version called `self.mark_primary_needs_full_repaint()`
/// unconditionally from `render_frame`'s `retry_needed` arm
/// — but this method's own doc, at the `producer` binding, already
/// says `producer` (the presentation whose segment actually
/// produced the failed submit) is NEVER assumed to be `primary()`,
/// and every OTHER decision in the method reads `producer`
/// specifically. With more than one presentation resident (the
/// production topology `install_presentation_alongside`, behind
/// `open_secondary_window`, creates), a primary-only mark repaints
/// the WRONG tree whenever a non-primary presentation is the real
/// producer — the actually-failing presentation's own pipeline is
/// never re-dirtied, so ITS retry still parks, while the untouched
/// primary spuriously repaints instead.
///
/// Drives exactly that: A (primary) produces and settles, then B
/// (a second, resident presentation) gets real content and becomes
/// the producer whose submit fails. The oracle checks BOTH
/// presentations' own clocks, not just a call counter — a call
/// counter alone cannot distinguish "B's retry succeeded" from "A's
/// stale content got spuriously re-marked, re-submitted, and
/// coincidentally reached render_scene too", which is exactly what
/// the primary-only draft of this fix would produce here: A's own
/// segment would wrongly reopen (marked dirty by mistake) and
/// produce a second, spurious snapshot while B — the one presentation
/// that actually needed the retry — never got remarked and stayed at
/// one snapshot (its own failed attempt, forever unretried).
#[test]
fn a_mid_frame_submit_failure_retry_repaints_the_actual_producer_not_the_primary() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    // Pump 1: only A (primary) has content. A produces, presents,
    // and settles -- from here on A has nothing left to redo.
    realm
        .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
        .expect("A attaches");
    let mut warm_up = ScriptedSink::always_presents();
    assert!(
        realm.render_frame(&mut warm_up),
        "pump 1: A alone must produce and present"
    );
    assert_eq!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .clock()
            .frames_since(None)
            .len(),
        1,
        "precondition: A recorded exactly its own pump-1 snapshot"
    );

    // B gets real content; A stays static/settled from here on.
    realm
        .attach_root_widget_to_for_test(b_id, &flui_widgets::SizedBox::new(20.0, 20.0))
        .expect("B attaches");

    // Pump 2: B is the producer (A's segment skips -- nothing dirty
    // there), and B's submit fails.
    let mut backend = ScriptedSink::fails_once_then_presents(SubmitVerdict::SurfaceStale);
    let presented = realm.render_frame(&mut backend);
    assert!(!presented, "SurfaceLost never reaches present()");
    assert_eq!(
        backend.submit_calls, 1,
        "precondition: B's segment actually reached render_scene once"
    );
    assert!(realm.needs_redraw(), "the failure must arm a retry");

    // Pump 3, the retry -- no hand-dirtying of either presentation.
    let presented = realm.render_frame(&mut backend);
    assert!(presented, "the retry must actually reach present()");
    assert_eq!(
        backend.submit_calls, 2,
        "the retry must reach render_scene a second time"
    );

    let a_snapshots = realm
        .presentations
        .get(a_id)
        .expect("A installed")
        .clock()
        .frames_since(None)
        .len();
    let b_snapshots = realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .clock()
        .frames_since(None)
        .len();
    assert_eq!(
        a_snapshots, 1,
        "A (the primary) must stay untouched by a retry that belongs to B -- \
         marking A's pipeline dirty instead of the real producer's is exactly \
         the bug this test guards against"
    );
    assert_eq!(
        b_snapshots, 2,
        "B (the actual producer) must record both the failed attempt and the \
         presented retry on its OWN clock -- a call counter alone cannot tell \
         this apart from A's stale content being spuriously re-marked instead"
    );
    // The submit-count assertions above are one-sided: `frames_since`
    // counts SUBMITS, and only the per-pump `producer` submits, so an
    // extra segment run on a NON-producing presentation is invisible
    // to them. Marking A *in addition to* B — rather than instead of
    // it — passes both of the above while A really does run a
    // spurious full build/layout/paint. Its flush count is what sees
    // that: 1 when only B is marked, 2 when A is marked too.
    assert_eq!(
        realm.presentations.primary().flush_count(),
        1,
        "A must not run a second segment at all -- the assertions above \
         cannot see this, because a non-producing presentation submits \
         nothing for `frames_since` to count"
    );
}
