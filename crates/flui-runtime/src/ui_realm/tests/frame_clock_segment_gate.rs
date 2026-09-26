use std::sync::Mutex as StdMutex;

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer as TracingLayer, Registry};

use super::*;

type RecordedFields = Vec<(String, String)>;
type RecordedEvents = Vec<RecordedFields>;

#[derive(Clone, Default)]
struct FrameEventCapture(Arc<StdMutex<RecordedEvents>>);

struct FrameFieldVisitor<'a>(&'a mut RecordedFields);

impl Visit for FrameFieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.push((field.name().to_owned(), value.to_string()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.push((field.name().to_owned(), value.to_string()));
    }
}

impl<S> TracingLayer<S> for FrameEventCapture
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        if event.metadata().target() != "flui.frame" {
            return;
        }
        let mut fields = Vec::new();
        event.record(&mut FrameFieldVisitor(&mut fields));
        self.0
            .lock()
            .expect("BUG: frame capture is locked only by this test")
            .push(fields);
    }
}

fn table_constraints() -> BoxConstraints {
    BoxConstraints::tight(flui_types::Size::new(px(800.0), px(600.0)))
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

/// Companion: with nothing dirty a second pump must not re-run the
/// segment either (the table's `false/false` row still holds after
/// a settled first frame, not just from a fresh realm).
#[test]
fn a_settled_presentation_stays_at_zero_flushes_across_repeated_idle_pumps() {
    let realm = UiRealm::for_test();
    for _ in 0..5 {
        let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    }
    assert_eq!(realm.presentations.primary().flush_count(), 0);
}

/// The equivalence table above only varies `woken`/`has_pending_work`
/// with an undeferred clock; this proves first-frame deferral does
/// not perturb the segment-runs answer for the SAME demand -- the
/// regression this exact companion exists to catch is a `poll`
/// that treats deferral as a reason to skip the segment (it must
/// only ever withhold the submit; see `.flutter/packages/flutter/
/// lib/src/rendering/binding.dart:582-599`).
#[test]
fn segment_still_runs_by_the_same_predicate_while_deferred() {
    let realm = mount_root_here();
    realm.presentations.primary().clock().defer();

    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

    assert_eq!(
        realm.presentations.primary().flush_count(),
        1,
        "attach's own pending build must still run the segment even though deferred"
    );
}

/// Item 8 (wake-bit -> clock-latch survival across a full
/// defer/lift round trip): a wake bit marked before deferral begins
/// is consumed by the very next (withheld) pump -- the segment
/// genuinely runs on it -- and neither an already-settled pump
/// during the deferral, nor lifting with nothing newly dirty,
/// produces a second, spurious flush.
#[test]
fn a_wake_bit_marked_before_deferral_is_consumed_by_exactly_one_flush() {
    let realm = UiRealm::for_test(); // no attach: the wake bit is the ONLY demand source
    let presentation = realm.presentations.primary();

    presentation.mark_redraw_pending();
    presentation.clock().defer();

    // Deferred, but the segment still runs on the marked wake bit
    // (Flutter parity) -- this pump's demand is consumed here.
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert_eq!(
        presentation.flush_count(),
        1,
        "the wake bit marked before deferral must be consumed by exactly one flush"
    );

    // Nothing newly dirty: settled, still deferred.
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert_eq!(
        presentation.flush_count(),
        1,
        "still deferred and settled -- no new flush"
    );

    presentation.clock().lift();
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert_eq!(
        presentation.flush_count(),
        1,
        "lifting with nothing newly marked must not conjure a spurious extra flush -- \
         the earlier withheld poll already consumed the only demand there was"
    );
}

/// Item 9 (the `mark_first_frame_sent` timing decision, pinned): an
/// idle pump (genuinely nothing dirty, so the segment never even
/// runs) must NOT latch "first frame confirmed sent" -- only a
/// real, successful `Produce` does. If an idle pump wrongly
/// latched, a `defer_first_frame` issued only AFTER that idle pump
/// would already be permanently inert; this proves it is not.
#[test]
fn an_idle_pump_while_deferred_does_not_latch_first_frame_sent() {
    let realm = UiRealm::for_test(); // nothing attached: no demand at all
    realm.defer_first_frame();

    let mut backend = ScriptedSink::always_presents();
    let presented = realm.render_frame(&mut backend);
    assert!(!presented, "nothing to present with no root attached");

    // If the idle pump above had (wrongly) latched first_frame_sent,
    // this deferral would already be permanently inert. Attach a
    // root now (real demand) and confirm it is STILL deferred.
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(1.0, 1.0)))
        .expect("attach succeeds");
    let presented = realm.render_frame(&mut backend);
    assert!(
        !presented,
        "the earlier idle pump must not have latched first_frame_sent -- this \
         presentation is still deferred"
    );
    // But the segment DID run (Flutter parity): the attach's own
    // pending build was serviced even though withheld.
    assert_eq!(realm.presentations.primary().flush_count(), 1);

    realm.allow_first_frame();
    let presented = realm.render_frame(&mut backend);
    assert!(
        presented,
        "lifting must finally let the withheld content through"
    );
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

/// Driver-loop hybrid: a running controller with
/// NO other tree-visible dirty state must still flush the segment
/// on every tick it's running -- Flutter's own `Ticker`-driven
/// `scheduleFrame` behavior, where the running ticker alone is
/// sufficient (a listener's own `notifyListeners` is not required
/// for the frame to be scheduled). Before this slice, `Animation`
/// demand was never marked in production wiring (`draw_frame_
/// entered`'s vsync-continuation loop only called `wake_frame()`,
/// never `presentation.clock().mark_demand`), so a bare running
/// controller with nothing else dirty would silently never flush
/// -- this is the regression this test exists to catch.
#[test]
fn a_running_controller_with_no_other_dirty_state_still_flushes_every_tick() {
    use std::time::Duration;

    use flui_animation::AnimationController;

    let realm = UiRealm::for_test();
    let controller = AnimationController::new(
        Duration::from_millis(100),
        &flui_scheduler::UpdateScheduler::new(),
    );
    realm.vsync().register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    realm.set_now_secs_for_test(0.0);
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert_eq!(
        realm.presentations.primary().flush_count(),
        1,
        "a running controller alone, with no widget tree attached and no other \
         dirty state, must still flush this pump's segment"
    );

    realm.set_now_secs_for_test(0.05);
    let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    assert_eq!(
        realm.presentations.primary().flush_count(),
        2,
        "and again on the next mid-run tick"
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

/// The SAME invariant as the test above, but through the actual
/// PRODUCTION call sequence rather than `draw_frame_entered` alone:
/// `bootstrap_desktop`'s `on_request_frame` closure calls
/// `UiRealm::record_compositor_tick` unconditionally, on every
/// single pump, before ever reaching `draw_frame_entered`. An
/// earlier version of `FrameClock::record_compositor_tick` marked
/// `DemandKind::Host` unconditionally, which -- because this
/// exact sequence runs on every desktop pump -- made `poll`
/// permanently unable to return `Skip(NoDemand)` there: the
/// demand-mask gate, the entire point of this clock, was inert on
/// the one path that matters. This test is what would have caught that;
/// the test above alone does not, because it never calls
/// `record_compositor_tick`.
#[test]
fn an_idle_presentation_stays_idle_through_the_production_call_sequence_with_compositor_ticks() {
    let realm = UiRealm::for_test();
    let mut now = web_time::Instant::now();

    for pump in 0..50 {
        realm.set_now_secs_for_test(f64::from(pump) * 0.016);
        now += std::time::Duration::from_millis(16);
        realm.record_compositor_tick(now);
        let (_, outcome, _) = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
        assert!(
            matches!(outcome, FramePaintOutcome::Idle),
            "pump {pump}: recording a compositor tick every single pump, exactly as \
             production does, must not by itself make an idle presentation produce \
             anything but Idle"
        );
    }

    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
        0,
        "recording a compositor tick every pump, through the real production \
         sequence, must not grant a single produce on an otherwise-idle presentation"
    );
    assert_eq!(
        realm.presentations.primary().flush_count(),
        0,
        "and must never run the segment either"
    );
}

/// The instant-response half of the same invariant: after an idle
/// stretch with a settled tree, dirtying the root render node
/// produces exactly one frame on the very next pump -- kills
/// "wake-to-first-frame costs an extra pump".
#[test]
fn a_single_dirty_mark_after_an_idle_stretch_produces_on_the_very_next_pump() {
    let realm = UiRealm::for_test();
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("attach succeeds");
    // Settle: the attach itself is one produce: the FIRST pump below
    // paints it, and every pump after that finds the tree quiescent.
    for pump in 0..20 {
        realm.set_now_secs_for_test(f64::from(pump) * 0.016);
        let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    }
    let produced_while_settled = realm.presentations.primary().clock().produced_count();
    assert_eq!(
        produced_while_settled, 1,
        "sanity: exactly the attach's own first paint, then genuinely idle"
    );

    // Dirty the root render node directly (the same real signal
    // `has_pending_work` reads), then pump exactly once more.
    realm.presentations.primary().pipeline().with_mut(|owner| {
        if let Some(root_id) = owner.root_id() {
            owner.mark_needs_paint(root_id);
        }
    });
    let (_, outcome, _) = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));

    assert!(
        matches!(outcome, FramePaintOutcome::Painted(_)),
        "the very next pump after the dirty mark must produce a painted frame, \
         not merely go dirty and wait"
    );
    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
        produced_while_settled + 1,
        "exactly one additional produce -- no wake-to-first-frame delay"
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

/// A running controller (`Animation` demand, marked unconditionally
/// every pump per the vsync-continuation wiring) must not spin a
/// hidden presentation: the gate short-circuits before the demand
/// mask is even consulted. Kills "a gated presentation with a
/// running controller keeps producing".
#[test]
fn gated_and_animating_presentation_does_not_spin() {
    use std::time::Duration;

    use flui_animation::AnimationController;

    let realm = UiRealm::for_test();
    let presentation_id = realm.presentations.primary().id();
    let controller = AnimationController::new(
        Duration::from_secs(1),
        &flui_scheduler::UpdateScheduler::new(),
    );
    realm.vsync().register(controller.clone());
    controller.forward().expect("fresh controller forwards");

    realm.set_presentation_hidden(presentation_id, true);

    for pump in 0..20 {
        realm.set_now_secs_for_test(f64::from(pump) * 0.01);
        let _ = realm.enter(|realm| realm.draw_frame_entered(table_constraints()));
    }

    assert_eq!(
        realm.presentations.primary().flush_count(),
        0,
        "a gated presentation with a running controller must not spin -- \
         its segment must never run"
    );
    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
        0,
        "and must never grant a produce"
    );

    controller.dispose();
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

/// Unhiding with an EMPTY mask (nothing was ever dirtied while
/// hidden) must not spuriously wake the platform -- the wake is
/// conditioned on retained demand, not unconditional on every
/// unhide.
#[test]
fn unoccluding_an_undirtied_presentation_does_not_wake() {
    let (wake, wake_count) = super::counting_wake();
    let realm = super::new_runtime(wake).expect("runtime");
    let presentation_id = realm.presentations.primary().id();

    realm.set_presentation_hidden(presentation_id, true);
    wake_count.store(0, Ordering::Relaxed);

    realm.set_presentation_hidden(presentation_id, false);

    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        0,
        "unhiding an idle (never-dirtied) presentation must not wake the platform"
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

/// Gesture-arena deadlines are input-correctness, not frame
/// production (plan-level rule): they remain a realm pre-segment
/// step that runs on every logical pump regardless of this
/// presentation's own `FrameClock` gate. A gated presentation with
/// an in-flight long-press must still resolve it, through the
/// arena's own callback, with zero GPU submits -- kills "deadline
/// rides the produce gate".
#[test]
fn gated_long_press_resolves_through_the_arenas_own_callback_with_zero_submits() {
    use std::time::Duration;

    use flui_interaction::{
        GestureRecognizer, GestureSettings, LongPressGestureRecognizer, PointerId,
    };

    let realm = mount_root_here();
    let presentation_id = realm.presentations.primary().id();
    let mut backend = ScriptedSink::always_presents();

    // Settle the attach's own first paint before gating -- baselines
    // captured AFTER settling, not absolute zero.
    let _ = realm.render_frame(&mut backend);
    realm.set_presentation_hidden(presentation_id, true);
    let flush_count_before = realm.presentations.primary().flush_count();
    let produced_before = realm.presentations.primary().clock().produced_count();
    let submits_before = backend.submit_calls;

    // Arm a real long-press against the SAME arena
    // `draw_frame_entered`'s gesture-deadline tick polls -- not a
    // scripted stand-in.
    let arena = realm.presentations.primary().gestures().arena().clone();
    let fired = Arc::new(AtomicBool::new(false));
    let fired_for_callback = Arc::clone(&fired);
    let recognizer = LongPressGestureRecognizer::with_settings(
        arena,
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(30)),
    )
    .with_on_long_press(move || fired_for_callback.store(true, Ordering::SeqCst));
    let pointer = PointerId::new(2).expect("nonzero pointer id");
    recognizer.add_pointer(
        pointer,
        flui_types::Offset::new(px(10.0), px(10.0)),
        flui_types::Offset::new(px(10.0), px(10.0)),
    );

    // Real wall-clock wait past the deadline: the arena's own clock
    // is the real OS clock here (`GestureBinding::new()`), same as
    // production.
    std::thread::sleep(Duration::from_millis(60));

    // Pump while still hidden: `draw_frame_entered`'s gesture tick
    // runs unconditionally, resolving the long-press through its
    // OWN callback, even though the segment gate skips this
    // presentation's build/layout/paint/submit entirely.
    for pump in 0..3 {
        realm.set_now_secs_for_test(f64::from(pump) * 0.016);
        let presented = realm.render_frame(&mut backend);
        assert!(!presented, "pump {pump}: must not present while hidden");
    }

    assert!(
        fired.load(Ordering::SeqCst),
        "the long-press must fire through the arena's own poll_deadlines tick even \
         though this presentation is gated"
    );
    assert_eq!(
        realm.presentations.primary().flush_count(),
        flush_count_before,
        "the gated presentation's segment must never run"
    );
    assert_eq!(
        realm.presentations.primary().clock().produced_count(),
        produced_before,
        "the gated presentation must never grant a produce"
    );
    assert_eq!(
        backend.submit_calls, submits_before,
        "zero GPU submits -- the long-press resolves through the arena's own \
         callback, never through the produce path"
    );
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
        flui_types::Offset::new(px(10.0), px(10.0)),
        flui_types::Offset::new(px(10.0), px(10.0)),
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
        flui_types::Offset::new(px(20.0), px(20.0)),
        flui_types::Offset::new(px(20.0), px(20.0)),
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

/// `GestureArena::next_deadline`'s own min-over-members level,
/// discriminated -- production topology is any number of arena
/// members competing on one presentation's arena (e.g. a detector
/// combining long-press and double-tap on the same region), so
/// this is the level a single-presentation, single-recognizer test
/// cannot see. Two DIFFERENT recognizer kinds on the SAME arena, a
/// far long-press timeout and a near double-tap give-up: the
/// aggregate must reflect the near one.
#[test]
fn gesture_arena_next_deadline_is_the_min_across_two_recognizers_on_one_arena() {
    // `web_time::Instant`, not std's: on wasm32 they are DIFFERENT
    // types (std's panics there, so the deadline machinery uses
    // web-time's `performance.now()`-backed one), and comparing a
    // std instant against what `next_wake`/`next_deadline` return
    // does not compile for that target. Identical on native, which
    // is why this only surfaced once the lib-test target was built
    // for wasm32.
    use web_time::{Duration, Instant};

    use flui_interaction::{
        DoubleTapGestureRecognizer, GestureRecognizer, GestureSettings, LongPressGestureRecognizer,
        PointerId,
    };

    let realm = UiRealm::for_test();
    let arena = realm.presentations.primary().gestures().arena().clone();

    let long_press = LongPressGestureRecognizer::with_settings(
        arena.clone(),
        GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_secs(5)),
    )
    .with_on_long_press(|| {});
    long_press.add_pointer(
        PointerId::new(2).expect("nonzero pointer id"),
        flui_types::Offset::new(px(10.0), px(10.0)),
        flui_types::Offset::new(px(10.0), px(10.0)),
    );

    let before_double_tap = Instant::now();
    let double_tap =
        DoubleTapGestureRecognizer::new(arena.clone()).with_on_double_tap_cancel(|_| {});
    let pointer = PointerId::new(3).expect("nonzero pointer id");
    let position = flui_types::Offset::new(px(20.0), px(20.0));
    double_tap.add_pointer(pointer, position, position);
    double_tap.handle_event(flui_interaction::PointerDispatch::at_root(
        &flui_interaction::events::make_up_event(
            position,
            flui_interaction::events::PointerType::Touch,
        ),
    ));

    let next_deadline = arena
        .next_deadline()
        .expect("two armed deadlines pending -- next_deadline must be Some");
    let until_deadline = next_deadline.saturating_duration_since(before_double_tap);
    assert!(
        until_deadline < Duration::from_secs(1),
        "GestureArena::next_deadline must reflect the double-tap's near give-up \
         window, not the long-press's far one -- got {until_deadline:?} until deadline"
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

    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::geometry::{Offset, Pixels};

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

    let down = make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Mouse);
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

/// Two inputs dispatched before one produce: coalescing honesty --
/// both survive with distinct latencies, the older one larger.
/// Companion to the single-input test above, at the SAME
/// production-sequence level (through `handle_input_addressed`, not
/// `FrameClock::stamp_input_epoch` called directly).
#[test]
fn two_dispatched_inputs_before_one_produce_both_attributed_older_larger() {
    use std::thread;
    use std::time::Duration;

    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::geometry::{Offset, Pixels};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let first = make_down_event(Offset::new(Pixels(1.0), Pixels(1.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(first));
    });
    thread::sleep(Duration::from_millis(8));

    let second = make_down_event(Offset::new(Pixels(2.0), Pixels(2.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(second));
    });

    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));

    let snapshots = realm.presentations.primary().clock().frames_since(None);
    assert_eq!(snapshots.len(), 1);
    let mut latencies: Vec<_> = snapshots[0].latencies().collect();
    assert_eq!(
        latencies.len(),
        2,
        "both dispatched inputs must survive into the same produced frame's record \
         -- coalescing must not be last-input-wins"
    );
    // Order as pushed: first dispatched (older) is index 0.
    latencies.sort_by_key(|(id, _)| id.get());
    let (_, older_latency) = latencies[0];
    let (_, newer_latency) = latencies[1];
    assert!(
        older_latency > newer_latency,
        "the earlier-dispatched input must show the larger latency: \
         older={older_latency:?} newer={newer_latency:?}"
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

#[test]
fn successful_submit_emits_structured_frame_telemetry_with_tail_quality() {
    let realm = mount_root_here();
    let capture = FrameEventCapture::default();
    let subscriber = Registry::default().with(capture.clone());
    let mut backend = ScriptedSink::always_presents();

    // Disarm `tracing`'s process-global callsite-interest cache first: it is
    // computed on whichever thread reaches a callsite FIRST, so without this a
    // sibling test can have it cached as `never` and silently empty this capture.
    // See `flui_testing::log_capture`.
    flui_testing::log_capture::disarm_interest_cache();
    tracing::subscriber::with_default(subscriber, || {
        assert!(realm.render_frame(&mut backend));
    });

    let events = capture
        .0
        .lock()
        .expect("BUG: frame capture is locked only by this test")
        .clone();
    let fields = events
        .iter()
        .find(|fields| {
            fields
                .iter()
                .any(|(field, value)| field == "event" && value == "frame_telemetry")
        })
        .expect("the successful submit must emit one frame_telemetry event");
    let field = |name: &str| {
        fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value.as_str()))
    };

    assert_eq!(field("event"), Some("frame_telemetry"));
    assert_eq!(field("present_outcome"), Some("presented"));
    assert_eq!(field("input_epochs_overflowed"), Some("false"));
    assert_eq!(field("produces_deferred"), Some("0"));
    assert_eq!(field("frames_dropped"), Some("0"));
    assert!(field("presentation").is_some());
    assert!(field("frame_id").is_some());
    assert!(field("produce_to_present_us").is_some());
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

    use flui_interaction::events::{PointerType, make_move_event};
    use flui_types::geometry::{Offset, Pixels};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let dispatched = flui_scheduler::MAX_COALESCED_INPUT_EPOCHS + 4;
    for i in 0..dispatched {
        let position = Offset::new(Pixels(i as f32), Pixels(i as f32));
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

/// Finding: `submit_at` used to be sampled BEFORE `render_scene`
/// was called, so any time the call spent blocked inside a
/// synchronous `present()` — which the production backend does on
/// the Vulkan/Wayland path under `Fifo` — was invisible to the
/// recorded latency, and the shipped
/// `input_to_present_histogram`/`produce_to_present_histogram`
/// names would then be lying about what they measure (submit, not
/// present). This backend simulates that in-call cost with a real
/// sleep, so the ordering pinned here holds for any backend whose
/// present takes measurable time — including one that waits on a
/// drawable rather than on the present itself; a `submit_at` sampled
/// before the call would report a latency that does NOT include it.
#[test]
fn submit_latency_includes_time_spent_inside_render_scene_not_just_before_it() {
    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::geometry::{Offset, Pixels};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
    });

    // The sleep stands in for the time a production backend's
    // `output.present()` can spend inside the call — under the
    // default `Fifo` presentation mode that is a wait for the next
    // vsync on the Vulkan/Wayland path — proving `submit_at` is
    // sampled AFTER `render_scene` returns, not before it.
    let sleep = std::time::Duration::from_millis(30);
    let mut backend = ScriptedSink::new(move |_, _| {
        std::thread::sleep(sleep);
        SubmitVerdict::Presented
    });
    assert!(
        realm.render_frame(&mut backend),
        "precondition: the frame actually reached present()"
    );

    let snapshots = realm.presentations.primary().clock().frames_since(None);
    assert_eq!(snapshots.len(), 1);
    let (_, latency) = snapshots[0]
        .latencies()
        .next()
        .expect("the dispatched input's epoch must be attributed to this frame");
    assert!(
        latency >= sleep,
        "recorded input->present latency ({latency:?}) must include the time \
         render_scene spent blocked inside its own present() call (simulated here as \
         a {sleep:?} sleep) -- a submit_at sampled BEFORE render_scene would \
         understate this by up to the sleep duration"
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
    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::geometry::{Offset, Pixels};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
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

/// The `DeviceLost` counterpart to
/// `surface_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame`
/// above: before this change, `DeviceLost` used `EpochDisposition::Drain`
/// unconditionally AND never armed a retry, so this pin would have been
/// meaningless either way — a mistaken `Drain` on this arm is caught
/// here, not just by inspection. Drives the same
/// input -> failure -> retry -> attribution sequence, substituting
/// `DeviceLost` for `SurfaceLost`.
#[test]
fn device_lost_retry_preserves_the_original_input_epoch_for_the_presented_frame() {
    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::geometry::{Offset, Pixels};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
    });

    // First attempt: the device is lost. A retry is armed, and the
    // failed attempt's own snapshot must still see the epoch that
    // was pending (diagnostic value), but must NOT consume it.
    let mut failing_backend = ScriptedSink::new(|_, _| SubmitVerdict::DeviceLost);
    let presented = realm.render_frame(&mut failing_backend);
    assert!(!presented, "DeviceLost never reaches present()");
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

    // Retry: as of #637's fix this hand mark is no longer required
    // to reach `render_scene` again (`render_frame`'s own
    // `retry_needed` arm now does that -- see the `SurfaceLost` pin
    // above's comment for the non-vacuous test that covers it). Kept
    // here standing in for the renderer owner's recovery + wake, to
    // isolate this test's own epoch-attribution invariant rather
    // than actually rebuilding a device.
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

/// The `SurfaceValidation` counterpart to both pins above. Named
/// finding: flipping `EpochDisposition::Retain` back to `Drain` on
/// the `SurfaceValidation` arm specifically was NOT caught by
/// anything in the suite before this test existed — 281/281 stayed
/// green under that flip, because `surface_validation_keeps_needs_
/// redraw_armed_for_a_retry` only pins the `needs_redraw` half of
/// the fix, never epoch attribution. Same input -> failure -> retry
/// -> attribution sequence as the two pins above, substituting
/// `SurfaceValidation` for `DeviceLost`/`SurfaceLost`.
#[test]
fn surface_validation_retry_preserves_the_original_input_epoch_for_the_presented_frame() {
    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::geometry::{Offset, Pixels};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
    });

    // First attempt: the surface fails validation. A retry is
    // armed, and the failed attempt's own snapshot must still see
    // the epoch that was pending (diagnostic value), but must NOT
    // consume it.
    let mut failing_backend = ScriptedSink::new(|_, _| SubmitVerdict::SurfaceStale);
    let presented = realm.render_frame(&mut failing_backend);
    assert!(!presented, "SurfaceValidation never reaches present()");
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

    // Retry: as of #637's fix this hand mark is no longer required
    // to reach `render_scene` again (`render_frame`'s own
    // `retry_needed` arm now does that -- see the `SurfaceLost` pin
    // above's comment for the non-vacuous test that covers it). Kept
    // here standing in for the external reconfigure a later wake
    // performs, to isolate this test's own epoch-attribution
    // invariant rather than actually reconfiguring a surface.
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

/// Finding: `DragDrop` is stamped unconditionally before the match
/// on input kind, even though its own arm traces the event as
/// dropped without routing it anywhere or requesting a frame -- a
/// phantom epoch that would sit in the ring until eviction,
/// attributed to whatever unrelated frame happens to produce next.
/// Dispatches a dropped `DragDrop` event followed by a real,
/// routed `Pointer` event and confirms only the pointer's epoch
/// survives to the produced frame.
#[test]
fn drag_drop_input_is_not_stamped_since_it_is_dropped_not_routed() {
    use flui_interaction::events::{PointerType, make_down_event};
    use flui_types::geometry::{Offset, Pixels};

    let realm = mount_root_here();
    let primary_id = realm.presentations.primary().id();

    let drag_drop = PlatformInput::DragDrop(DragDropEvent::Exited {
        id: flui_foundation::DataTransferId::new(1),
    });
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, drag_drop);
    });

    let down = make_down_event(Offset::new(Pixels(0.0), Pixels(0.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(primary_id, PlatformInput::Pointer(down));
    });

    let mut backend = ScriptedSink::always_presents();
    assert!(realm.render_frame(&mut backend));

    let snapshots = realm.presentations.primary().clock().frames_since(None);
    assert_eq!(snapshots.len(), 1);
    let latencies: Vec<_> = snapshots[0].latencies().collect();
    assert_eq!(
        latencies.len(),
        1,
        "only the routed pointer event's epoch must survive -- the dropped drag-drop \
         event must never have been stamped at all"
    );
    assert_eq!(
        latencies[0].0.get(),
        0,
        "the pointer event must have minted the FIRST epoch id on this clock -- if \
         drag-drop had wrongly been stamped first, the pointer's own id would be 1"
    );
}
