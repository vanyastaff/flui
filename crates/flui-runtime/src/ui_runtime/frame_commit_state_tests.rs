use std::cell::Cell;
use std::rc::Rc;

use flui_widgets::SizedBox;

use super::{SegmentPhase, UiRuntime};
use crate::epoch::{FrameCommitState, TreeRevision};
use crate::sink::{FrameSettlement, Settled, SubmitVerdict};
use crate::testing::ScriptedSink;

fn with_quiet_panics<R>(f: impl FnOnce() -> R) -> R {
    type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;

    struct HookRestore(Option<PanicHook>);

    impl Drop for HookRestore {
        fn drop(&mut self) {
            if let Some(hook) = self.0.take() {
                std::panic::set_hook(hook);
            }
        }
    }

    let _restore = HookRestore(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_| {}));
    f()
}

fn mount_box() -> UiRuntime {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .attach_root_widget(&SizedBox::new(20.0, 20.0))
        .expect("root attaches");
    ui_runtime
}

fn primary_state(ui_runtime: &UiRuntime) -> FrameCommitState {
    ui_runtime.presentations.primary().frame_commit_state()
}

fn assert_uncommitted_since(ui_runtime: &UiRuntime, since: TreeRevision) {
    assert_eq!(
        primary_state(ui_runtime),
        FrameCommitState::Uncommitted { since }
    );
}

fn arm_one_shot_build_panic(ui_runtime: &UiRuntime) {
    let is_armed = Rc::new(Cell::new(true));
    let probe_is_armed = Rc::clone(&is_armed);
    ui_runtime.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(move |_| {
            assert!(
                !probe_is_armed.replace(false),
                "build probe — intentional one-shot panic"
            );
        })),
    );
}

pub(crate) fn errored_frame_is_uncommitted() {
    let ui_runtime = mount_box();
    arm_one_shot_build_panic(&ui_runtime);
    let mut backend = ScriptedSink::always_presents();

    assert!(!with_quiet_panics(|| ui_runtime.render_frame(&mut backend)));
    assert_uncommitted_since(&ui_runtime, TreeRevision::ZERO.next());
    assert_eq!(backend.submit_calls, 0);
}

pub(crate) fn deferred_painted_frame_waits_for_the_later_present_to_commit() {
    let ui_runtime = mount_box();
    ui_runtime.defer_first_frame();
    let mut backend = ScriptedSink::always_presents();

    assert!(!ui_runtime.render_frame(&mut backend));
    assert_uncommitted_since(&ui_runtime, TreeRevision::ZERO.next());
    assert_eq!(backend.submit_calls, 0);

    ui_runtime.allow_first_frame();
    assert!(ui_runtime.render_frame(&mut backend));
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
}

/// A frame withheld from the sink takes no epoch: the first frame the sink
/// sees is its presentation's first.
pub(crate) fn a_withheld_frame_takes_no_epoch() {
    let ui_runtime = mount_box();
    ui_runtime.defer_first_frame();
    let mut backend = ScriptedSink::always_presents();
    assert!(!ui_runtime.render_frame(&mut backend));
    assert!(!ui_runtime.render_frame(&mut backend));
    ui_runtime.allow_first_frame();
    assert!(ui_runtime.render_frame(&mut backend));
    assert_eq!(
        backend.epochs,
        vec![flui_foundation::FrameEpoch::ZERO.next()],
        "the frames painted while deferred never reached the sink"
    );
}

/// The runtime numbers the frames of all its presentations in one sequence:
/// a frame a secondary presentation produces takes the epoch after the
/// primary's last, so the epochs one sink is handed only grow.
pub(crate) fn every_presentation_takes_the_next_epoch_of_its_runtime() {
    let mut ui_runtime = mount_box();
    let mut backend = ScriptedSink::always_presents();
    assert!(ui_runtime.render_frame(&mut backend));
    let secondary = ui_runtime.install_second_presentation_for_test();
    ui_runtime
        .attach_root_widget_to_for_test(secondary, &SizedBox::new(10.0, 10.0))
        .expect("secondary root");
    assert!(ui_runtime.render_frame(&mut backend));
    let secondary = ui_runtime
        .presentations
        .get(secondary)
        .expect("the secondary is installed");
    assert!(
        secondary.tree_revision() > TreeRevision::ZERO
            && secondary.frame_commit_state() == FrameCommitState::Committed,
        "the secondary painted the second frame and the sink's verdict committed it"
    );
    let first = flui_foundation::FrameEpoch::ZERO.next();
    assert_eq!(backend.epochs, vec![first, first.next()]);
}

/// Paints and queues two frames, the second after the tree moved on, and returns their epochs
/// and the revisions they were painted from.
fn queue_two_frames(
    ui_runtime: &UiRuntime,
    sink: &mut ScriptedSink,
) -> [(flui_foundation::FrameEpoch, TreeRevision); 2] {
    let primary = || ui_runtime.presentations.primary();
    assert!(
        !ui_runtime.render_frame(sink),
        "a queued frame is not presented yet"
    );
    let first = primary().tree_revision();
    ui_runtime.mark_needs_full_repaint_for(primary());
    assert!(!ui_runtime.render_frame(sink));
    let second = primary().tree_revision();
    assert!(
        second > first,
        "the second frame was painted from a later tree"
    );
    let [one, two] = sink.epochs[..] else {
        panic!("two frames were submitted, got {:?}", sink.epochs);
    };
    [(one, first), (two, second)]
}

fn settle(ui_runtime: &mut UiRuntime, epoch: flui_foundation::FrameEpoch) -> Settled {
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };
    ui_runtime.settle_frame(FrameSettlement::new(
        address,
        epoch,
        SubmitVerdict::Presented,
    ))
}

/// A queued frame that settles after a later one was painted commits the revision it was painted
/// from, not the tree as it is when its settlement arrives.
pub(crate) fn a_settlement_commits_the_revision_its_frame_was_painted_from() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let [(first, first_revision), (second, _)] = queue_two_frames(&ui_runtime, &mut sink);
    assert_uncommitted_since(&ui_runtime, TreeRevision::ZERO.next());

    assert_eq!(settle(&mut ui_runtime, first), Settled::Applied);
    assert_uncommitted_since(&ui_runtime, first_revision.next());
    assert_eq!(settle(&mut ui_runtime, second), Settled::Applied);
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
}

/// A settlement of a later frame folds the frames queued before it and still pending, which get
/// no settlement of their own: one that still arrives is stale.
pub(crate) fn a_later_settlement_folds_the_frames_it_superseded() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let [(first, _), (second, _)] = queue_two_frames(&ui_runtime, &mut sink);

    assert_eq!(settle(&mut ui_runtime, second), Settled::Applied);
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
    assert!(
        ui_runtime
            .presentations
            .primary()
            .pending_frames
            .borrow()
            .is_empty(),
        "the superseded frame's record folded into the settled one"
    );
    assert_eq!(settle(&mut ui_runtime, first), Settled::Stale);
}

/// While a frame is in flight the owner sleeps no longer than the floor, so a settlement whose
/// wake was lost is drained within it; once nothing is pending the floor goes.
pub(crate) fn a_frame_in_flight_bounds_the_owners_sleep() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let primary = |ui_runtime: &UiRuntime| ui_runtime.presentations.primary().in_flight_floor();
    let [_, (second, _)] = queue_two_frames(&ui_runtime, &mut sink);
    // The wake is read first: the floor read after it lies no earlier on the same clock.
    let wake = ui_runtime.next_wake();
    let floor = primary(&ui_runtime).expect("a frame in flight keeps a floor");
    let submitted = ui_runtime
        .presentations
        .primary()
        .pending_frames
        .borrow()
        .front()
        .expect("BUG: two frames are pending")
        .submitted_at();
    assert_eq!(
        floor.duration_since(submitted).as_nanos() % super::IN_FLIGHT_FLOOR.as_nanos(),
        0,
        "the floor counts whole intervals from the oldest frame's submit, not from each read"
    );
    let now = ui_runtime.presentations.primary().clock().now();
    assert!(floor <= now + super::IN_FLIGHT_FLOOR);
    assert!(
        wake.is_some_and(|wake| wake <= floor),
        "the owner sleeps no longer than the floor"
    );
    assert_eq!(settle(&mut ui_runtime, second), Settled::Applied);
    assert_eq!(
        primary(&ui_runtime),
        None,
        "with nothing in flight the floor goes"
    );
}

/// Work that reached a presentation at its in-flight limit without demand on its clock (a tree
/// dirtied after the gated pump, whose wake the gate held back) is asked for by the settlement
/// that frees a place (ADR-0305 §5), unless a dropped frame's backoff still runs or the device
/// rebuild was given up: those presentations attempt no frame, and their deadline or a new
/// rebuild wakes them.
pub(crate) fn a_settlement_asks_for_work_the_gate_held_back() {
    #[derive(Debug, Clone, Copy)]
    enum Hold {
        Nothing,
        BackoffRunning,
        BackoffOver,
        DeviceAbandoned,
    }
    for (hold, wakes) in [
        (Hold::Nothing, true),
        (Hold::BackoffRunning, false),
        (Hold::BackoffOver, true),
        (Hold::DeviceAbandoned, false),
    ] {
        let mut ui_runtime = mount_box();
        // Two frames fill the sink; the older one's settlement frees a place without showing
        // the current tree, so it leaves an armed backoff in place.
        let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued)
            .holding(std::num::NonZeroUsize::new(2).expect("BUG: two is not zero"));
        let [(older, _), _] = queue_two_frames(&ui_runtime, &mut sink);
        let primary = ui_runtime.presentations.primary();
        assert!(primary.frames_in_flight().is_saturated(), "{hold:?}");
        ui_runtime.mark_needs_full_repaint_for(primary);
        ui_runtime.mark_rendered();
        let now = primary.clock().now();
        match hold {
            Hold::Nothing => {}
            Hold::BackoffRunning => {
                primary.arm_failure_retry(now + std::time::Duration::from_secs(60));
            }
            Hold::BackoffOver => {
                primary.arm_failure_retry(now);
                // The ended backoff is no wake while the sink is full: no frame would follow.
                assert!(
                    ui_runtime
                        .next_wake_excluding_agent_waits()
                        .is_none_or(|wake| wake > now),
                    "an ended backoff of a saturated presentation is no wake"
                );
            }
            Hold::DeviceAbandoned => {
                primary.replace_device_recovery(Some(
                    crate::frame_failure::DeviceRecovery::Abandoned { attempts: 1 },
                ));
            }
        }
        assert!(
            primary.clock().demand_mask().is_empty() && !ui_runtime.needs_redraw(),
            "{hold:?}: the dirt carries no clock demand and no wake"
        );
        assert_eq!(settle(&mut ui_runtime, older), Settled::Applied, "{hold:?}");
        assert_eq!(
            ui_runtime.needs_redraw(),
            wakes,
            "{hold:?}: the freed place asks for the dirty tree's frame unless the presentation \
             may attempt none"
        );
    }
}

fn held_pointer_count(ui_runtime: &UiRuntime) -> usize {
    ui_runtime
        .presentations
        .primary()
        .held_pointer_input()
        .borrow()
        .len()
}

/// Input held while queued frames wait replays only once the screen shows the tree the latest
/// of them was painted from: an older frame's settlement commits an older revision, and the
/// input keeps waiting.
pub(crate) fn held_input_waits_for_the_settlement_that_commits_the_tree() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let [(first, _), (second, _)] = queue_two_frames(&ui_runtime, &mut sink);
    let id = ui_runtime.presentation_id();
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(
            id,
            flui_platform::PlatformInput::Pointer(
                flui_interaction::events::make_down_event(
                    flui_foundation::geometry::Offset::new(4.0, 6.0),
                    flui_interaction::events::PointerKind::Touch,
                )
                .expect("finite test position"),
            ),
        );
    });
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the Down waits for a commit"
    );

    assert_eq!(settle(&mut ui_runtime, first), Settled::Applied);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the older frame's commit leaves the tree uncommitted, so the Down still waits"
    );
    assert_eq!(settle(&mut ui_runtime, second), Settled::Applied);
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        0,
        "the commit of the latest tree replays the Down"
    );
}

/// A frame the surface withheld commits nothing (ADR-0335 §1): the tree stays uncommitted, so a
/// press waits for the frame that shows it, and that frame's commit replays the press.
///
/// Fails when a withheld frame commits its tree, which dispatches the press against a tree the
/// screen does not show.
pub(crate) fn a_withheld_frame_commits_nothing_and_holds_input_for_the_frame_that_shows_it() {
    let ui_runtime = mount_box();
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);
    assert!(!ui_runtime.render_frame(&mut withheld));
    assert_eq!(withheld.submit_calls, 1, "the scene reached the sink");
    assert_uncommitted_since(&ui_runtime, TreeRevision::ZERO.next());
    press_primary(&ui_runtime);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the Down waits for the frame that shows the tree"
    );

    assert!(ui_runtime.needs_redraw(), "the withheld frame is retried");
    assert!(ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        0,
        "the shown frame's commit replays the Down"
    );
}

/// The withheld retry is bounded (ADR-0335 §2): `MAX_NOT_SHOWN_RETRIES` attempts follow each
/// other, and past them every attempt waits out the dropped-frame backoff, which grows, so a
/// drawable that stays withdrawn costs at most one paint per backoff interval while the tree it
/// owes, and the input held for it, still reach the screen once the surface returns.
///
/// Fails when the budget parks the frame (no wait is armed, and the held press waits for an
/// unrelated frame), when an attempt past the budget runs before its wait ends, or when the wait
/// does not grow.
pub(crate) fn the_withheld_retry_is_bounded_and_then_waits_out_the_backoff() {
    let budget = super::MAX_NOT_SHOWN_RETRIES;
    let ui_runtime = mount_box();
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);
    for _ in 0..=budget {
        assert!(
            !ui_runtime.render_frame(&mut withheld),
            "a withheld frame never reports a present"
        );
    }
    assert_eq!(
        withheld.submit_calls,
        budget + 1,
        "{budget} retained attempts follow each other, then the attempt that exhausts the budget"
    );
    let presentation = || ui_runtime.presentations.primary();
    let mut delays = Vec::new();
    for attempt in 0..2 {
        if attempt > 0 {
            assert!(ui_runtime.wait_out_failure_retry_for_test());
            assert!(!ui_runtime.render_frame(&mut withheld));
        }
        let at = presentation()
            .failure_retry_at()
            .expect("an attempt past the budget waits out the dropped-frame backoff");
        delays.push(at.saturating_duration_since(presentation().clock().now()));
        let submits = withheld.submit_calls;
        for _ in 0..8 {
            let _ = ui_runtime.render_frame(&mut withheld);
        }
        assert_eq!(
            withheld.submit_calls, submits,
            "no attempt runs before its wait ends"
        );
    }
    assert_eq!(withheld.submit_calls, budget + 2);
    assert!(
        delays[0] < delays[1],
        "the wait grows while the surface keeps withholding: {delays:?}"
    );

    press_primary(&ui_runtime);
    assert_eq!(held_pointer_count(&ui_runtime), 1, "the Down waits");
    assert_eq!(
        presentation().failure_retry_at(),
        None,
        "a press ends the wait early, as for any dropped frame"
    );
    assert!(
        ui_runtime.render_frame(&mut ScriptedSink::always_presents()),
        "the surface returned: the retry shows the tree"
    );
    assert_eq!(
        held_pointer_count(&ui_runtime),
        0,
        "its commit replays the Down"
    );
    assert_eq!(presentation().failure_retry_at(), None);
    assert_eq!(presentation().frame_failure_streak(), 0);
}

/// A subscriber, scoped to the current thread, that panics when the span named `name` is created
/// (`on_exit` false) or when it exits (`on_exit` true).
struct PanicsOnSpan {
    name: &'static str,
    on_exit: bool,
}

impl tracing::Subscriber for PanicsOnSpan {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let named = attributes.metadata().name() == self.name;
        assert!(
            !named || self.on_exit,
            "a subscriber that cannot open {}",
            self.name
        );
        tracing::span::Id::from_u64(if named { 2 } else { 1 })
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, id: &tracing::span::Id) {
        assert!(
            !self.on_exit || id.into_u64() != 2,
            "a subscriber that cannot close {}",
            self.name
        );
    }
}

/// Runs `render` under `subscriber` and returns whether it panicked.
fn render_under(
    subscriber: impl tracing::Subscriber + Send + Sync + 'static,
    render: impl FnOnce(),
) -> bool {
    // A second dispatcher keeps the callsite interest from being cached against this thread's
    // subscriber alone.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let unwound = with_quiet_panics(|| {
        tracing::subscriber::with_default(subscriber, || {
            tracing::callsite::rebuild_interest_cache();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(render))
        })
    });
    tracing::callsite::rebuild_interest_cache();
    unwound.is_err()
}

/// The sink showed the scene, so the frame is committed even when the subscriber cannot open the
/// `commit` span; the subscriber's panic then continues.
///
/// Fails when the span is opened before the commit and its panic skips it.
#[test]
fn a_subscriber_that_panics_opening_the_commit_span_does_not_skip_the_commit() {
    let ui_runtime = mount_box();
    let mut backend = ScriptedSink::always_presents();

    let panicked = render_under(
        PanicsOnSpan {
            name: "commit",
            on_exit: false,
        },
        || {
            ui_runtime.render_frame(&mut backend);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert_eq!(
        primary_state(&ui_runtime),
        FrameCommitState::Committed,
        "the sink showed the scene, so the displayed tree is committed"
    );
}

/// A retryable verdict keeps its repaint obligation when the subscriber panics as the `submit`
/// span closes: the next frame still has paint work to resubmit.
///
/// Fails when the span's panic resumes before the retry work is applied.
#[test]
fn a_subscriber_that_panics_as_the_submit_span_closes_keeps_the_retry() {
    let ui_runtime = mount_box();
    let mut failing = ScriptedSink::new(|_, _| SubmitVerdict::Retry);

    let panicked = render_under(
        PanicsOnSpan {
            name: "submit",
            on_exit: true,
        },
        || {
            ui_runtime.render_frame(&mut failing);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert!(ui_runtime.needs_redraw(), "the retry is armed");
    let mut succeeding = ScriptedSink::always_presents();
    assert!(
        ui_runtime.render_frame(&mut succeeding),
        "the retry has paint work to resubmit and reaches the sink"
    );
}

/// A subscriber that cannot open the `submit` span keeps the sink from ever seeing the scene, and
/// the pipeline has already spent that scene's paint work: the repaint is owed before the panic
/// resumes, or the next frame finds nothing to draw.
///
/// Fails when only the staged agent reads are restored.
#[test]
fn a_subscriber_that_panics_opening_the_submit_span_keeps_the_repaint_owed() {
    let ui_runtime = mount_box();
    let mut backend = ScriptedSink::always_presents();

    let panicked = render_under(
        PanicsOnSpan {
            name: "submit",
            on_exit: false,
        },
        || {
            ui_runtime.render_frame(&mut backend);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert_eq!(backend.submit_calls, 0, "the sink never saw the scene");
    assert!(ui_runtime.needs_redraw(), "the retry is armed");
    let mut succeeding = ScriptedSink::always_presents();
    assert!(
        ui_runtime.render_frame(&mut succeeding),
        "the lost scene is painted again"
    );
    assert_eq!(
        succeeding.epochs,
        vec![flui_foundation::FrameEpoch::ZERO.next()],
        "the scene the subscriber stopped before the sink took no epoch"
    );
}

/// A withheld frame whose trace fails still records the withholding and arms its retry: the
/// subscriber's panic waits for the verdict's own obligations.
///
/// Fails when the trace runs before the retry is armed.
#[test]
fn a_subscriber_that_panics_on_the_trace_of_a_withheld_frame_keeps_the_retry() {
    let ui_runtime = mount_box();
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);

    let panicked = render_under(
        PanicsOnEvent {
            needle: "never shown",
        },
        || {
            ui_runtime.render_frame(&mut withheld);
        },
    );
    assert!(panicked, "the subscriber's panic continues");
    assert!(ui_runtime.needs_redraw(), "the retry is armed");
    let mut succeeding = ScriptedSink::always_presents();
    assert!(
        ui_runtime.render_frame(&mut succeeding),
        "the withheld scene is painted again"
    );
}

/// A surface that never shows a frame backs off on the whole schedule: past the budget each wait
/// doubles until it reaches `FAILURE_RETRY_CAP` and stays there, every attempt a deadline, and
/// once a frame presents the next withdrawal starts over from the display rate.
///
/// Fails when a wait shrinks while the surface keeps withholding, when the waits stop short of
/// the ceiling or pass it, or when a present leaves the next withdrawal at the ceiling.
pub(crate) fn a_withholding_surface_backs_off_to_the_ceiling_and_starts_over_once_it_shows() {
    let budget = super::MAX_NOT_SHOWN_RETRIES;
    let ui_runtime = mount_box();
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);
    let presentation = || ui_runtime.presentations.primary();
    let wait = || {
        presentation()
            .failure_retry_at()
            .map(|at| at.saturating_duration_since(presentation().clock().now()))
    };
    for _ in 0..=budget {
        let _ = ui_runtime.render_frame(&mut withheld);
    }
    let mut waits = vec![wait().expect("past the budget an attempt waits")];
    while waits.len() < 16 {
        assert!(ui_runtime.wait_out_failure_retry_for_test());
        assert!(!ui_runtime.render_frame(&mut withheld));
        waits.push(wait().expect("every attempt past the budget waits"));
    }
    let cap = crate::frame_failure::FAILURE_RETRY_CAP;
    // Each wait is read against the clock a moment after it was armed.
    let slack = std::time::Duration::from_millis(1);
    assert!(
        waits.windows(2).all(|pair| pair[0] <= pair[1] + slack),
        "the wait never shrinks while the surface withholds: {waits:?}"
    );
    assert!(
        waits.iter().all(|wait| *wait <= cap),
        "never past the ceiling: {waits:?}"
    );
    assert!(
        waits.last().is_some_and(|last| *last + slack >= cap),
        "the schedule reaches its ceiling: {waits:?}"
    );

    press_primary(&ui_runtime);
    assert!(ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
    assert_eq!(wait(), None, "the present ends the wait");
    // The shown tree is clean: the next withdrawal starts with a repaint.
    ui_runtime.mark_needs_full_repaint_for(presentation());
    for _ in 0..budget {
        let _ = ui_runtime.render_frame(&mut withheld);
    }
    assert_eq!(
        wait(),
        None,
        "a new withdrawal is ridden out at the display rate again"
    );
    let _ = ui_runtime.render_frame(&mut withheld);
    let restarted = wait().expect("past the new budget an attempt waits");
    assert!(
        restarted < cap,
        "and its first wait starts the schedule over, not at the ceiling: {restarted:?}"
    );
}

/// A subscriber, scoped to the current thread, that keeps the `host_reports_occlusion` of each
/// `surface_withholds_frames` event.
#[derive(Clone, Default)]
struct WithheldReports(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl tracing::Subscriber for WithheldReports {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        #[derive(Default)]
        struct Report {
            withholds: bool,
            reports_occlusion: String,
        }
        impl tracing::field::Visit for Report {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "event" {
                    self.withholds = value == "surface_withholds_frames";
                }
            }
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "host_reports_occlusion" {
                    self.reports_occlusion = format!("{value:?}");
                }
            }
        }
        let mut report = Report::default();
        event.record(&mut report);
        if report.withholds {
            self.0
                .lock()
                .expect("the report log")
                .push(report.reports_occlusion);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A surface that keeps withholding frames is reported once per presentation, when the streak
/// leaves the budget, saying whether the window's host tells a covering (an unseen covering is
/// the usual cause where it cannot); the attempts after it are not reported again.
///
/// Fails when the report is left out, when it repeats on every attempt past the budget, or when
/// it leaves out whether the host reports occlusion.
pub(crate) fn a_surface_that_keeps_withholding_frames_is_reported_once() {
    let budget = super::MAX_NOT_SHOWN_RETRIES;
    // A window that reports no covering (`WindowCapabilities::NONE`), kept alive here: the
    // presentation holds it weakly.
    let window: std::rc::Rc<dyn flui_platform::PlatformWindow> =
        std::rc::Rc::new(crate::testing::TestWindow::new());
    let ui_runtime = UiRuntime::for_test_with_window(std::rc::Rc::clone(&window));
    ui_runtime
        .attach_root_widget(&SizedBox::new(20.0, 20.0))
        .expect("root attaches");
    let reports = WithheldReports::default();
    let log = std::sync::Arc::clone(&reports.0);
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    tracing::subscriber::with_default(reports, || {
        tracing::callsite::rebuild_interest_cache();
        for _ in 0..budget {
            let _ = ui_runtime.render_frame(&mut withheld);
        }
        assert!(
            log.lock().expect("the report log").is_empty(),
            "a withdrawal inside the budget is a transient, not reported"
        );
        let _ = ui_runtime.render_frame(&mut withheld);
        for _ in 0..3 {
            assert!(ui_runtime.wait_out_failure_retry_for_test());
            let _ = ui_runtime.render_frame(&mut withheld);
        }
    });
    tracing::callsite::rebuild_interest_cache();
    assert_eq!(withheld.submit_calls, budget + 4);
    assert_eq!(
        *log.lock().expect("the report log"),
        ["Some(false)"],
        "one report, past the budget, saying the host tells no covering"
    );
    drop(window);
}

/// A subscriber, scoped to the current thread, that panics on the event whose message holds
/// `needle`.
struct PanicsOnEvent {
    needle: &'static str,
}

impl tracing::Subscriber for PanicsOnEvent {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        assert!(
            !message.0.contains(self.needle),
            "a subscriber that cannot record {}",
            self.needle
        );
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A settlement whose verdict arms a retry keeps it when a subscriber panics on the verdict's
/// own trace: the panic resumes once the repaint and the wake are in place.
///
/// Fails when the settlement resumes the panic before its tail arms the retry.
#[test]
fn a_subscriber_that_panics_in_a_settlement_keeps_its_retry() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    assert!(!ui_runtime.render_frame(&mut sink));
    let [epoch] = sink.epochs[..] else {
        panic!("one frame was queued, got {:?}", sink.epochs);
    };
    assert!(!ui_runtime.needs_redraw());
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };

    let panicked = render_under(
        PanicsOnEvent {
            needle: "never shown",
        },
        || {
            let _ = ui_runtime.settle_frame(FrameSettlement::new(
                address,
                epoch,
                SubmitVerdict::NotShown,
            ));
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert!(ui_runtime.needs_redraw(), "the retry is armed");
    let mut succeeding = ScriptedSink::always_presents();
    assert!(
        ui_runtime.render_frame(&mut succeeding),
        "the withheld scene is painted again"
    );
}

/// A frame the sink answers inline supersedes the frames it queued before: a settlement of
/// theirs that arrives after it is stale and cannot commit an older tree.
#[test]
fn an_inline_frame_supersedes_the_frames_queued_before_it() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|call, _| {
        if call == 0 {
            SubmitVerdict::Queued
        } else {
            SubmitVerdict::Presented
        }
    });
    assert!(!ui_runtime.render_frame(&mut sink));
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    assert!(
        ui_runtime.render_frame(&mut sink),
        "the second frame presents inline"
    );
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
    assert!(
        ui_runtime
            .presentations
            .primary()
            .pending_frames
            .borrow()
            .is_empty(),
        "the queued frame's record went with the inline one"
    );
    assert_eq!(settle(&mut ui_runtime, sink.epochs[0]), Settled::Stale);
}

/// A lost device's retry is armed before code the runtime does not own runs: a subscriber that
/// panics on the frame's telemetry leaves the paced retry in place.
///
/// Fails when the telemetry runs before the retry is armed.
#[test]
fn a_subscriber_that_panics_on_a_lost_devices_telemetry_keeps_its_retry() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    assert!(!ui_runtime.render_frame(&mut sink));
    let [epoch] = sink.epochs[..] else {
        panic!("one frame was queued, got {:?}", sink.epochs);
    };
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };

    let panicked = render_under(
        PanicsOnEvent {
            needle: "frame telemetry",
        },
        || {
            let _ = ui_runtime.settle_frame(FrameSettlement::new(
                address,
                epoch,
                SubmitVerdict::DeviceLost,
            ));
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert!(
        ui_runtime
            .presentations
            .primary()
            .failure_retry_at()
            .is_some(),
        "the paced retry is armed"
    );
}

/// A frame the sink answers inline with a verdict that commits nothing leaves the frames it
/// queued before to their own settlements: one that still shows commits its revision.
#[test]
fn a_frame_answered_inline_without_a_commit_leaves_queued_frames_to_their_settlements() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|call, _| {
        if call == 0 {
            SubmitVerdict::Queued
        } else {
            SubmitVerdict::Retry
        }
    });
    assert!(!ui_runtime.render_frame(&mut sink));
    let queued_revision = ui_runtime.presentations.primary().tree_revision();
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    assert!(
        !ui_runtime.render_frame(&mut sink),
        "the second frame is refused"
    );
    assert_eq!(settle(&mut ui_runtime, sink.epochs[0]), Settled::Applied);
    assert_uncommitted_since(&ui_runtime, queued_revision.next());
}

/// Queues `frames` more frames, each painted from a later tree.
fn queue_more(ui_runtime: &UiRuntime, sink: &mut ScriptedSink, frames: usize) {
    for _ in 0..frames {
        ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
        assert!(!ui_runtime.render_frame(sink));
    }
}

/// Every queued frame keeps its record until a settlement names it or a later frame: a frame
/// the raster side may be rendering is never folded on a guess, whichever frame the inbox
/// superseded. A settlement of the second of three folds only the first.
///
/// Fails when queueing a third frame folds a record.
#[test]
fn a_queued_frame_keeps_its_record_until_a_settlement_reaches_it() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let [(first, _), (second, _)] = queue_two_frames(&ui_runtime, &mut sink);
    queue_more(&ui_runtime, &mut sink, 1);
    let third = *sink.epochs.last().expect("three frames were queued");

    assert_eq!(settle(&mut ui_runtime, second), Settled::Applied);
    assert_eq!(settle(&mut ui_runtime, first), Settled::Stale);
    assert_eq!(settle(&mut ui_runtime, third), Settled::Applied);
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
}

/// Queues one frame through a sink that answers `verdict`, and returns the frame's stamp.
fn queue_one(ui_runtime: &UiRuntime, verdict: SubmitVerdict) -> flui_foundation::FrameEpoch {
    let mut sink = ScriptedSink::new(move |_, _| verdict);
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    assert!(!ui_runtime.render_frame(&mut sink));
    *sink.epochs.last().expect("the frame was submitted")
}

/// A raster side that renders one frame for long while the owner keeps producing replaces each
/// unrendered frame in its inbox and names it: that record folds by its stamp, so the ring stays
/// at the frame rendering and the one waiting. When the rendering frame settles undelivered and
/// the executor takes the waiting one, the next frame queues beside them and folds nothing: the
/// frame on the GPU keeps its record and settles `Applied`.
///
/// Fails when a replaced frame's record waits for a later settlement, or when a frame queued
/// into a long pile folds the one the executor took.
#[test]
fn a_replaced_frame_folds_by_its_stamp_and_never_the_frame_being_rendered() {
    let mut ui_runtime = mount_box();
    let rendering = queue_one(&ui_runtime, SubmitVerdict::Queued);
    let mut waiting = queue_one(&ui_runtime, SubmitVerdict::Queued);
    let mut replaced = Vec::new();
    for _ in 0..super::settlement::MAX_PENDING_FRAMES {
        replaced.push(waiting);
        waiting = queue_one(&ui_runtime, SubmitVerdict::QueuedReplacing(waiting));
    }
    let pending = || {
        ui_runtime
            .presentations
            .primary()
            .pending_frames
            .borrow()
            .len()
    };
    assert_eq!(pending(), 2, "the frame rendering and the one waiting");
    // The executor took the waiting frame; the next one finds the inbox empty.
    let next = queue_one(&ui_runtime, SubmitVerdict::Queued);
    assert_eq!(pending(), 3);

    assert_eq!(settle(&mut ui_runtime, rendering), Settled::Applied);
    for replaced in replaced {
        assert_eq!(settle(&mut ui_runtime, replaced), Settled::Stale);
    }
    assert_eq!(
        settle(&mut ui_runtime, waiting),
        Settled::Applied,
        "the frame the executor took keeps its record"
    );
    assert_eq!(settle(&mut ui_runtime, next), Settled::Applied);
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
}

/// A subscriber that panics while a replaced frame's record folds leaves the frame that replaced
/// it queued and finished: its record waits for its settlement, the replaced one is gone, and
/// the panic continues after the frame's tail.
///
/// Fails when the fold runs uncontained, so its panic leaves the frame before its tail.
#[test]
fn a_subscriber_that_panics_on_a_replaced_frame_leaves_the_replacing_frame_queued() {
    let mut ui_runtime = mount_box();
    let rendering = queue_one(&ui_runtime, SubmitVerdict::Queued);
    let replaced = queue_one(&ui_runtime, SubmitVerdict::Queued);
    let mut sink = ScriptedSink::new(move |_, _| SubmitVerdict::QueuedReplacing(replaced));
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());

    let panicked = render_under(
        PanicsOnEvent {
            needle: "abandoned before it was shown",
        },
        || {
            let _ = ui_runtime.render_frame(&mut sink);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert!(
        !ui_runtime.needs_redraw(),
        "the frame was queued before the panic continued"
    );
    let replacing = *sink.epochs.last().expect("the frame was submitted");
    assert_eq!(settle(&mut ui_runtime, replaced), Settled::Stale);
    assert_eq!(settle(&mut ui_runtime, rendering), Settled::Applied);
    assert_eq!(settle(&mut ui_runtime, replacing), Settled::Applied);
}

/// A sink that answers `Queued` past the ring's bound without naming the frame it replaced
/// broke its contract: the newest record, the frame it most likely replaced, folds, and the
/// oldest keeps its record. A subscriber that panics on the fold's error leaves the frame to
/// finish: it settles as rendered before the panic continues.
///
/// Fails when the fold drops the frame being rendered, or when its panic leaves the frame
/// before its tail.
#[test]
fn a_subscriber_that_panics_on_a_folded_frame_leaves_the_frame_finished() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let [(rendering, _), _] = queue_two_frames(&ui_runtime, &mut sink);
    queue_more(
        &ui_runtime,
        &mut sink,
        super::settlement::MAX_PENDING_FRAMES - 2,
    );
    let superseded = *sink.epochs.last().expect("frames were queued");
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());

    let panicked = render_under(
        PanicsOnEvent {
            needle: "the newest record folds",
        },
        || {
            let _ = ui_runtime.render_frame(&mut sink);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert_eq!(
        ui_runtime
            .presentations
            .primary()
            .pending_frames
            .borrow()
            .len(),
        super::settlement::MAX_PENDING_FRAMES,
        "the ring keeps its bound"
    );
    assert!(
        !ui_runtime.needs_redraw(),
        "the frame settled as rendered before the panic continued"
    );
    assert_eq!(
        settle(&mut ui_runtime, rendering),
        Settled::Applied,
        "the frame being rendered keeps its record"
    );
    assert_eq!(
        settle(&mut ui_runtime, superseded),
        Settled::Stale,
        "the frame the inbox replaced folded"
    );
    let newest = *sink.epochs.last().expect("frames were queued");
    assert_eq!(settle(&mut ui_runtime, newest), Settled::Applied);
}

fn press_primary(ui_runtime: &UiRuntime) {
    let id = ui_runtime.presentation_id();
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(
            id,
            flui_platform::PlatformInput::Pointer(
                flui_interaction::events::make_down_event(
                    flui_foundation::geometry::Offset::new(4.0, 6.0),
                    flui_interaction::events::PointerKind::Touch,
                )
                .expect("finite test position"),
            ),
        );
    });
}

/// Queues one frame, then answers the next inline with `later`.
fn queue_then_answer_inline(ui_runtime: &UiRuntime, later: SubmitVerdict) -> ScriptedSink {
    let mut sink = ScriptedSink::new(move |call, _| {
        if call == 0 {
            SubmitVerdict::Queued
        } else {
            later
        }
    });
    assert!(!ui_runtime.render_frame(&mut sink));
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    let _ = ui_runtime.render_frame(&mut sink);
    sink
}

/// Input held while an older frame settles, after the later frame failed inline, is not
/// stranded: the failed frame armed its paced retry, the older frame's commit leaves it standing,
/// and the retried frame's commit replays the input.
///
/// Fails when the failed frame arms no retry, or when the older frame's commit clears it.
#[test]
fn held_input_behind_a_failed_frame_asks_for_the_frame_that_shows_the_tree() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|call, _| {
        if call == 0 {
            SubmitVerdict::Queued
        } else {
            SubmitVerdict::Failed
        }
    });
    assert!(!ui_runtime.render_frame(&mut sink));
    // Pressed before the later frame fails, so the press ends no wait.
    press_primary(&ui_runtime);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the Down waits for a commit"
    );
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    assert!(!ui_runtime.render_frame(&mut sink));
    assert!(
        failure_retry_armed(&ui_runtime),
        "the failed frame is tried again after the backoff"
    );

    assert_eq!(settle(&mut ui_runtime, sink.epochs[0]), Settled::Applied);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the older frame does not show the tree the Down was aimed at"
    );
    assert!(
        failure_retry_armed(&ui_runtime),
        "the older frame's commit leaves the retry standing"
    );
    assert!(ui_runtime.wait_out_failure_retry_for_test());
    assert!(ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
    assert_eq!(
        held_pointer_count(&ui_runtime),
        0,
        "that frame's commit replays it"
    );
}

/// A frame that fails inline with nothing queued is painted again after the dropped-frame
/// backoff, which doubles while the frames keep failing, and a frame that presents resets it.
/// Without the retry the app would keep its last frame until an unrelated one.
///
/// Fails when a failed frame arms no retry, or when its clean segment resets the streak the
/// backoff grows by.
#[test]
fn a_failed_frame_is_painted_again_after_a_growing_backoff() {
    let ui_runtime = mount_box();
    let mut failing = ScriptedSink::new(|_, _| SubmitVerdict::Failed);
    let presentation = || ui_runtime.presentations.primary();
    let mut delays = Vec::new();
    for attempt in 0..3 {
        if attempt > 0 {
            assert!(ui_runtime.wait_out_failure_retry_for_test());
        }
        assert!(!ui_runtime.render_frame(&mut failing));
        assert_eq!(
            failing.submit_calls,
            attempt + 1,
            "each attempt paints and submits the tree again"
        );
        let at = presentation()
            .failure_retry_at()
            .expect("a failed frame arms its retry");
        delays.push(at.saturating_duration_since(presentation().clock().now()));
    }
    assert!(
        delays[0] < delays[1] && delays[1] < delays[2],
        "the backoff doubles while frames keep failing: {delays:?}"
    );

    assert!(ui_runtime.wait_out_failure_retry_for_test());
    assert!(ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
    assert_eq!(presentation().failure_retry_at(), None);
    assert_eq!(presentation().frame_failure_streak(), 0);
}

/// The inputs a queued frame was cut after are that frame's to report: a frame answered inline
/// with a failure while it waits reads them without taking them.
///
/// Fails when the failed frame drains the queued frame's inputs.
#[test]
fn a_failed_frame_leaves_a_queued_frames_inputs_to_its_settlement() {
    let mut ui_runtime = mount_box();
    let clock = ui_runtime.presentations.primary().clock();
    let input = clock.stamp_input_epoch(clock.now());
    let sink = queue_then_answer_inline(&ui_runtime, SubmitVerdict::Failed);

    assert_eq!(settle(&mut ui_runtime, sink.epochs[0]), Settled::Applied);
    let frames = ui_runtime
        .presentations
        .primary()
        .clock()
        .frames_since(None);
    let presented = frames
        .iter()
        .find(|frame| frame.present_outcome == flui_scheduler::PresentOutcome::Presented)
        .expect("the queued frame was recorded when it settled");
    assert!(
        presented.input_epochs.iter().any(|epoch| epoch.id == input),
        "the queued frame reports the input it was cut after: {frames:?}"
    );
}

/// A subscriber that panics on the trace of a superseded frame's cleanup leaves the frame that
/// superseded it to finish: it commits, the input held for it replays, and the panic continues
/// after.
///
/// Fails when the superseded frame's cleanup runs uncontained, at the settlement or inline.
#[test]
fn a_subscriber_that_panics_on_a_superseded_frame_leaves_the_settled_frame_finished() {
    let needle = "abandoned before it was shown";

    // A settlement folds the frame queued before it.
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let [(first, _), (second, _)] = queue_two_frames(&ui_runtime, &mut sink);
    press_primary(&ui_runtime);
    let panicked = render_under(PanicsOnEvent { needle }, || {
        let _ = settle(&mut ui_runtime, second);
    });
    assert!(panicked, "the subscriber's panic continues");
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
    assert_eq!(held_pointer_count(&ui_runtime), 0, "the held Down replays");
    assert_eq!(settle(&mut ui_runtime, first), Settled::Stale);

    // An inline frame folds the frame queued before it.
    let ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|call, _| {
        if call == 0 {
            SubmitVerdict::Queued
        } else {
            SubmitVerdict::Presented
        }
    });
    assert!(!ui_runtime.render_frame(&mut sink));
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    press_primary(&ui_runtime);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the Down waits for a commit"
    );
    let panicked = render_under(PanicsOnEvent { needle }, || {
        let _ = ui_runtime.render_frame(&mut sink);
    });
    assert!(panicked, "the subscriber's panic continues");
    assert_eq!(primary_state(&ui_runtime), FrameCommitState::Committed);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        0,
        "the held Down replays before the panic continues"
    );
    assert!(
        ui_runtime
            .presentations
            .primary()
            .pending_frames
            .borrow()
            .is_empty()
    );
}

/// Input held while a queued frame waits stays held when that frame settles but the
/// presentation is still uncommitted for another reason (a text sizing replacement withdrew its
/// layout): the replay follows the rule that held the input.
///
/// Fails when the settlement replays against a presentation that is not committed.
#[test]
fn a_settlement_leaves_input_held_while_the_presentation_is_not_committed() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    assert!(!ui_runtime.render_frame(&mut sink));
    let (_policy, writer) = flui_painting::TextSizing::captured();
    assert!(
        ui_runtime.install_captured_text_sizing_for(ui_runtime.presentation_id(), writer.source())
    );
    press_primary(&ui_runtime);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the Down waits for a commit"
    );

    assert_eq!(settle(&mut ui_runtime, sink.epochs[0]), Settled::Applied);
    assert_ne!(primary_state(&ui_runtime), FrameCommitState::Committed);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the Down waits for the commit the text replacement owes"
    );
}

/// An input stamped after a queued frame was cut and before a frame that failed inline belongs
/// to the failed frame alone: it is reported once, not again by the next frame that presents.
///
/// Fails when the failed frame reads its inputs without taking them.
#[test]
fn a_failed_frame_takes_the_inputs_stamped_since_the_queued_frame() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|call, _| {
        if call == 0 {
            SubmitVerdict::Queued
        } else {
            SubmitVerdict::Failed
        }
    });
    assert!(!ui_runtime.render_frame(&mut sink));
    let clock = ui_runtime.presentations.primary().clock();
    let late = clock.stamp_input_epoch(clock.now());
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    assert!(!ui_runtime.render_frame(&mut sink));
    assert_eq!(settle(&mut ui_runtime, sink.epochs[0]), Settled::Applied);
    // The failed frame waits out its backoff before the tree is painted again.
    assert!(ui_runtime.wait_out_failure_retry_for_test());
    assert!(ui_runtime.render_frame(&mut ScriptedSink::always_presents()));

    let frames = ui_runtime
        .presentations
        .primary()
        .clock()
        .frames_since(None);
    let reporting = frames
        .iter()
        .filter(|frame| frame.input_epochs.iter().any(|epoch| epoch.id == late))
        .map(|frame| frame.present_outcome)
        .collect::<Vec<_>>();
    assert_eq!(
        reporting,
        [flui_scheduler::PresentOutcome::Errored],
        "the failed frame reports the input once: {frames:?}"
    );
}

/// A sink that panics loses its frame: the frame's cleanup and the repaint it owes run
/// contained, so a subscriber that panics on the cleanup's trace still leaves the repaint armed
/// before the sink's panic continues.
///
/// Fails when the cleanup runs uncontained before the repaint.
#[test]
fn a_subscriber_that_panics_on_a_lost_frames_cleanup_leaves_the_repaint_armed() {
    let ui_runtime = mount_box();
    let mut sink =
        ScriptedSink::new(|_, _| -> SubmitVerdict { panic!("sink — intentional panic") });
    let panicked = render_under(
        PanicsOnEvent {
            needle: "abandoned before it was shown",
        },
        || {
            let _ = ui_runtime.render_frame(&mut sink);
        },
    );
    assert!(panicked, "the sink's panic continues");
    assert!(
        ui_runtime.render_frame(&mut ScriptedSink::always_presents()),
        "the lost scene is painted again"
    );
}

/// A withheld frame of an older tree that settles after its retry budget is spent arms nothing of
/// its own (ADR-0335 §3): the wait a later failed frame armed for the current tree, its streak and
/// the pointer input held for that tree stand as they were.
///
/// Fails when the settlement clears the failed frame's retry, or arms a wait of its own over it.
#[test]
fn a_withheld_settlement_of_an_older_tree_leaves_the_failed_frames_retry() {
    let budget = super::MAX_NOT_SHOWN_RETRIES;
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(move |call, _| match call {
        call if call < budget => SubmitVerdict::NotShown,
        call if call == budget => SubmitVerdict::Queued,
        _ => SubmitVerdict::Failed,
    });
    for _ in 0..budget {
        assert!(
            !ui_runtime.render_frame(&mut sink),
            "each attempt is withheld"
        );
    }
    assert!(
        !ui_runtime.render_frame(&mut sink),
        "the next frame is queued"
    );
    let queued = *sink.epochs.last().expect("a frame was queued");
    // Pressed before the later frame fails, so the press ends no wait.
    press_primary(&ui_runtime);
    assert_eq!(
        held_pointer_count(&ui_runtime),
        1,
        "the Down waits for a commit"
    );
    ui_runtime.mark_needs_full_repaint_for(ui_runtime.presentations.primary());
    assert!(
        !ui_runtime.render_frame(&mut sink),
        "a later frame fails inline"
    );
    assert!(
        failure_retry_armed(&ui_runtime),
        "the failed frame is tried again after the backoff"
    );

    let wait = |ui_runtime: &UiRuntime| {
        let presentation = ui_runtime.presentations.primary();
        (
            presentation.failure_retry_at(),
            presentation.frame_failure_streak(),
        )
    };
    let armed = wait(&ui_runtime);

    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };
    assert_eq!(
        ui_runtime.settle_frame(FrameSettlement::new(
            address,
            queued,
            SubmitVerdict::NotShown,
        )),
        Settled::Applied
    );
    assert_eq!(
        wait(&ui_runtime),
        armed,
        "the withheld frame leaves the failed frame's wait and streak standing"
    );
    assert_eq!(held_pointer_count(&ui_runtime), 1);
}

/// A queued frame that commits an older tree asks for the frame that shows the current one when
/// no pending frame was painted from it, with no input held; a frame still pending from the
/// current tree owes it instead, and a given-up device holds the frame (ADR-0267).
///
/// Fails when the repaint is owed only to held input, when a later pending frame is ignored, or
/// when a given-up device is asked for a frame.
#[test]
fn a_settlement_of_an_older_tree_asks_for_the_current_one() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|_, _| SubmitVerdict::Queued);
    let [(first, _), (second, _)] = queue_two_frames(&ui_runtime, &mut sink);
    queue_more(&ui_runtime, &mut sink, 1);
    let third = *sink.epochs.last().expect("three frames were queued");
    assert_eq!(held_pointer_count(&ui_runtime), 0);
    assert_eq!(settle(&mut ui_runtime, first), Settled::Applied);
    assert!(
        !ui_runtime.needs_redraw(),
        "the pending frames painted from later trees owe it"
    );

    // The tree moves on with no frame queued from it, while the device is given up.
    let abandoned = crate::frame_failure::DeviceRecovery::Abandoned { attempts: 10 };
    ui_runtime.note_primary_device_recovery(Some(abandoned));
    ui_runtime.presentations.primary().advance_tree_revision();
    assert_eq!(settle(&mut ui_runtime, second), Settled::Applied);
    assert!(
        !ui_runtime.needs_redraw(),
        "a given-up device asks for no frame"
    );

    ui_runtime
        .presentations
        .primary()
        .replace_device_recovery(None);
    ui_runtime.presentations.primary().advance_tree_revision();
    assert_eq!(settle(&mut ui_runtime, third), Settled::Applied);
    assert!(
        ui_runtime.needs_redraw(),
        "the settlement asks for the frame that shows the current tree"
    );
    assert_eq!(held_pointer_count(&ui_runtime), 0);
}

/// Whether the primary presentation waits out a dropped frame's backoff.
fn failure_retry_armed(ui_runtime: &UiRuntime) -> bool {
    ui_runtime
        .presentations
        .primary()
        .failure_retry_at()
        .is_some()
}

/// Withholds `MAX_NOT_SHOWN_RETRIES` frames in a row, so the next withheld frame is past the
/// budget.
fn spend_the_withheld_budget(ui_runtime: &UiRuntime, sink: &mut ScriptedSink) {
    for _ in 0..super::MAX_NOT_SHOWN_RETRIES {
        assert!(!ui_runtime.render_frame(sink), "each attempt is withheld");
    }
    assert_eq!(
        ui_runtime.presentations.primary().failure_retry_at(),
        None,
        "the budget's attempts arm no wait"
    );
}

/// A window shown again after it was hidden starts a new withdrawal (ADR-0335 §2): its withheld
/// streak clears, so once the wait already running ends, its next withheld frame is retried at
/// the display rate. The wait and the dropped-frame streak stand, since the window cannot tell
/// which dropped frame armed them.
///
/// Fails when a hidden and shown window keeps the exhausted withheld streak, so each of its next
/// withheld frames waits out the backoff, or when showing it ends a wait it did not arm.
#[test]
fn a_window_shown_again_rides_out_its_next_withdrawal_at_the_display_rate() {
    let ui_runtime = mount_box();
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);
    spend_the_withheld_budget(&ui_runtime, &mut withheld);
    assert!(!ui_runtime.render_frame(&mut withheld));
    let presentation = || ui_runtime.presentations.primary();
    let wait = || {
        (
            presentation().failure_retry_at(),
            presentation().frame_failure_streak(),
        )
    };
    let armed = wait();
    assert!(armed.0.is_some(), "past the budget, the frame waits");

    let id = ui_runtime.presentation_id();
    ui_runtime.set_presentation_hidden(id, true);
    ui_runtime.set_presentation_hidden(id, false);
    assert_eq!(wait(), armed, "the wait and the dropped-frame streak stand");

    assert!(ui_runtime.wait_out_failure_retry_for_test());
    let waited = wait();
    let submits = withheld.submit_calls;
    assert!(!ui_runtime.render_frame(&mut withheld));
    assert_eq!(withheld.submit_calls, submits + 1, "the wait ran out");
    assert_eq!(
        wait(),
        (None, waited.1),
        "the next withheld frame is in a new budget and arms no wait"
    );
    assert!(
        ui_runtime.needs_redraw(),
        "it is retried at the display rate"
    );
}

/// Past the budget, a withheld frame of a presentation whose device rebuild was given up arms no
/// wait (ADR-0335 §3): the given-up device holds its frames (ADR-0267).
///
/// Fails when the withheld frame arms the backoff against a given-up device.
#[test]
fn a_withheld_frame_past_its_budget_on_a_given_up_device_arms_no_wait() {
    let mut ui_runtime = mount_box();
    let mut sink = ScriptedSink::new(|call, _| {
        if call < super::MAX_NOT_SHOWN_RETRIES {
            SubmitVerdict::NotShown
        } else {
            SubmitVerdict::Queued
        }
    });
    spend_the_withheld_budget(&ui_runtime, &mut sink);
    assert!(
        !ui_runtime.render_frame(&mut sink),
        "the next frame is queued"
    );
    let queued = *sink.epochs.last().expect("a frame was queued");
    let abandoned = crate::frame_failure::DeviceRecovery::Abandoned { attempts: 10 };
    ui_runtime.note_primary_device_recovery(Some(abandoned));
    let streak = ui_runtime.presentations.primary().frame_failure_streak();

    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };
    assert_eq!(
        ui_runtime.settle_frame(FrameSettlement::new(
            address,
            queued,
            SubmitVerdict::NotShown,
        )),
        Settled::Applied
    );
    let presentation = ui_runtime.presentations.primary();
    assert_eq!(presentation.failure_retry_at(), None, "no wait is armed");
    assert_eq!(
        presentation.frame_failure_streak(),
        streak,
        "nothing counts into the streak"
    );
}

/// A subscriber that panics on the trace of a withheld frame past its budget leaves the wait armed:
/// the panic resumes once the wait and the repaint are in place.
///
/// Fails when the trace runs before the wait is armed.
#[test]
fn a_subscriber_that_panics_on_the_trace_of_a_withheld_frame_past_its_budget_keeps_the_wait() {
    let ui_runtime = mount_box();
    let mut withheld = ScriptedSink::new(|_, _| SubmitVerdict::NotShown);
    spend_the_withheld_budget(&ui_runtime, &mut withheld);

    let panicked = render_under(
        PanicsOnEvent {
            needle: "retrying after the dropped-frame backoff",
        },
        || {
            ui_runtime.render_frame(&mut withheld);
        },
    );

    assert!(panicked, "the subscriber's panic continues");
    assert!(
        ui_runtime
            .presentations
            .primary()
            .failure_retry_at()
            .is_some(),
        "the wait is armed"
    );
}
