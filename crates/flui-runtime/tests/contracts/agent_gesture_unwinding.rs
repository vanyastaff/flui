//! A gesture's answer is what happened, whichever user code around it panics (ADR-0287,
//! ADR-0400 §2). A wake of the platform that panics as an act is held, or as its input is
//! queued, leaves nothing to play; and when two wakes of one turn panic, the act's and a later
//! one (the window's, which still holds acts, or a parked wait's), the first continues.
//!
//! The headless host's wake cannot fail, so these drive [`Rig`], a runtime of their own whose wake
//! panics while the test says so.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use flui_protocol::method;
use flui_protocol::{
    act::ActOutcome, act::ActParams, error::ErrorCode, input::InputPath, input::InputPolicy,
    input::PressKeysParams, input::ScrollUntilParams, input::TapParams, locator::Locator,
    locator::Target, snapshot::Snapshot, wait::Condition, wait::FramesParams, wait::WaitOutcome,
    wait::WaitParams, wire::ActionName,
};
use flui_view::dev_agent::AgentAnswer;
use flui_view::prelude::*;
use flui_widgets::{Column, ListView, RawButton, SizedBox, Stack, Text};

use crate::agent_rig::Rig;

/// "Go", which counts its presses, "Gated", which has no handler and so is disabled, and "Items",
/// a list of 100 rows 20 px tall in a viewport 60 px tall.
#[derive(Clone, StatelessView)]
struct Pads(Arc<AtomicU32>);

impl StatelessView for Pads {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let go = Arc::clone(&self.0);
        let rows = ListView::builder(100, 20.0, |index| {
            Some(
                SizedBox::new(200.0, 20.0)
                    .child(Text::new(format!("Item {index}")))
                    .into_view()
                    .boxed(),
            )
        })
        .semantics_label("Items");
        Column::new(vec![
            RawButton::new(Text::new("Go"))
                .on_pressed(move |_: &mut EventContext<'_>| {
                    go.fetch_add(1, Ordering::SeqCst);
                })
                .into_view()
                .boxed(),
            RawButton::new(Text::new("Gated")).into_view().boxed(),
            SizedBox::new(200.0, 60.0).child(rows).into_view().boxed(),
        ])
    }
}

/// [`Pads`] shown by a [`Rig`], and how many times "Go" was pressed.
fn rig() -> (Rig, Arc<AtomicU32>) {
    let go = Arc::new(AtomicU32::new(0));
    (Rig::showing(&Pads(Arc::clone(&go))), go)
}

/// No input went out since the journal held `journaled` entries, over 20 frames, and "Go" was
/// never pressed.
fn played_nothing(rig: &mut Rig, go: &AtomicU32, journaled: usize, case: &str) {
    for _ in 0..20 {
        rig.frame();
    }
    assert_eq!(go.load(Ordering::SeqCst), 0, "{case}: nothing pressed");
    assert_eq!(
        rig.journal().entries.len(),
        journaled,
        "{case}: no input of it went out"
    );
}

fn named(name: &str) -> Target {
    Target::Locator(Locator::named(name))
}

type Ask = fn(&Rig) -> AgentAnswer<ActOutcome>;

/// A wake that panics in the turn that serves a gesture leaves nothing held and nothing queued:
/// the gesture is answered that the window panicked, the panic continues, and no input of it is
/// played afterwards. Whether the gesture is held for its target or its keys are queued at once,
/// a wake that ran after the act was held or queued would answer `platform` and play it anyway.
#[test]
fn a_wake_that_panics_as_a_gesture_is_held_or_queued_leaves_nothing_to_play() {
    let rows: [(&str, Ask); 2] = [
        ("a tap held for its target", |rig| {
            rig.lease
                .call::<method::ElementTap>(TapParams::new(named("Go")))
                .expect("the inbox has room")
        }),
        ("keys queued at once", |rig| {
            let params: PressKeysParams = serde_json::from_value(serde_json::json!({
                "window": rig.window.id(),
                "keys": ["a"],
            }))
            .expect("press keys reads");
            rig.lease
                .call::<method::ElementPressKeys>(params)
                .expect("the inbox has room")
        }),
    ];
    for (case, ask) in rows {
        let (mut rig, go) = rig();
        let journaled = rig.journal().entries.len();
        let mut asked = ask(&rig);
        assert_eq!(rig.failing_turn(), "wake 1", "{case}");
        let Some(answer) = asked.try_take() else {
            panic!("{case}: answered in the turn that served it");
        };
        let refused = answer.expect_err("the wake panicked");
        assert_eq!(refused.code, ErrorCode::Platform, "{case}: {refused:?}");
        played_nothing(&mut rig, &go, journaled, case);
    }
}

/// On a turn without a frame a held tap whose deadline passed is attempted once more, passes, and
/// the wake that asks for the frame playing it panics. The tap is answered that the window
/// panicked and nothing of it plays. A second wake of the same turn that panics, the window's
/// (it still holds an act) or a parked wait's, does not take the first one's place: `wake 1`
/// continues.
#[test]
fn the_first_wake_that_panics_in_a_turn_continues_and_its_act_plays_nothing() {
    /// What the window holds beside the tap: its answer, kept awaited for the turn.
    type Beside = fn(&Rig) -> Box<dyn std::any::Any>;
    let rows: [(&str, Beside); 2] = [
        ("the window still holds an act", |rig| {
            let mut tap = TapParams::new(named("Gated"));
            tap.options.timeout_ms = Some(60_000);
            let held: AgentAnswer<ActOutcome> = rig
                .lease
                .call::<method::ElementTap>(tap)
                .expect("the inbox has room");
            Box::new(held)
        }),
        ("a wait is parked", |rig| {
            let wait =
                WaitParams::new(rig.window.id(), Condition::Frames(1_000)).timeout_ms(60_000);
            let parked: AgentAnswer<WaitOutcome> = rig
                .window
                .call::<method::WaitUntil>(wait)
                .expect("the inbox has room");
            Box::new(parked)
        }),
    ];
    for (case, beside) in rows {
        let (mut rig, go) = rig();
        let journaled = rig.journal().entries.len();
        let mut tap = TapParams::new(named("Go"));
        tap.options.timeout_ms = Some(50);
        let mut tap = rig
            .lease
            .call::<method::ElementTap>(tap)
            .expect("the inbox has room");
        let beside = beside(&rig);
        rig.runtime.pump_background();
        assert!(tap.try_take().is_none(), "{case}: held for stability");
        rig.clock.advance(Duration::from_millis(100));
        assert_eq!(rig.failing_turn(), "wake 1", "{case}");
        let Some(answer) = tap.try_take() else {
            panic!("{case}: answered before its panic continued");
        };
        let answered = answer.expect_err("its wake panicked");
        assert_eq!(answered.code, ErrorCode::Platform, "{case}: {answered:?}");
        assert_eq!(
            answered.message, "the window panicked while answering",
            "{case}: the wake's panic, not a step before it"
        );
        played_nothing(&mut rig, &go, journaled, case);
        drop(beside);
    }
}

/// A semantics action that owes a reading has run once the reading is parked, so a wake that
/// panics after it does not
/// answer the act `platform`: the reading answers it with what ran, once, and the panic
/// continues. A held tap on the semantic path, attempted on a turn without a frame past its
/// deadline, and a `semantics.act` served at once each press "Go" and answer that they did.
#[test]
fn a_wake_that_panics_after_a_semantics_action_ran_leaves_its_answer_true() {
    let rows: [(&str, Ask); 2] = [
        ("a held tap on the semantic path", |rig| {
            let mut tap = TapParams::new(named("Go"));
            tap.options.timeout_ms = Some(50);
            tap.options.via = Some(InputPolicy::require(InputPath::Semantic));
            tap.options.snapshot = Snapshot::Delta;
            rig.lease
                .call::<method::ElementTap>(tap)
                .expect("the inbox has room")
        }),
        ("semantics.act", |rig| {
            rig.window
                .call::<method::SemanticsAct>(
                    ActParams::new(named("Go"), ActionName::Invoke).snapshot(Snapshot::Delta),
                )
                .expect("the inbox has room")
        }),
    ];
    for (case, ask) in rows {
        let (mut rig, go) = rig();
        let mut asked = ask(&rig);
        if case.starts_with("a held tap") {
            rig.runtime.pump_background();
            assert!(asked.try_take().is_none(), "{case}: held for stability");
            rig.clock.advance(Duration::from_millis(100));
        }
        assert_eq!(rig.failing_turn(), "wake 1", "{case}");
        let mut answer = None;
        for _ in 0..30 {
            answer = asked.try_take();
            if answer.is_some() {
                break;
            }
            rig.frame();
        }
        let Some(answer) = answer else {
            panic!("{case}: answered");
        };
        let outcome = match answer {
            Ok(outcome) => outcome,
            Err(error) => panic!("{case}: the action ran: {error:?}"),
        };
        assert_eq!(outcome.via, InputPath::Semantic, "{case}: {outcome:?}");
        for _ in 0..10 {
            rig.frame();
        }
        assert_eq!(go.load(Ordering::SeqCst), 1, "{case}: pressed once");
    }
}

/// A held `element.scroll_until` steps at frame boundaries, each step's input queued after the
/// wake that asks for the frame playing it. A wake that panics there leaves the step unqueued and
/// answers the search that the window panicked, not that resolving its scrollable did, and
/// nothing of the step plays.
#[test]
fn a_wake_that_panics_as_a_scroll_step_is_queued_answers_the_search_and_plays_nothing() {
    let (mut rig, go) = rig();
    let mut until = ScrollUntilParams::new(named("Items"), Locator::named("Item 99"));
    until.options.timeout_ms = Some(60_000);
    let mut until = rig
        .lease
        .call::<method::ElementScrollUntil>(until)
        .expect("the inbox has room");
    // Served, held, and its first step queued and played while the wake holds.
    for _ in 0..3 {
        rig.frame();
    }
    assert!(until.try_take().is_none(), "the search holds");
    let mut answer = None;
    for _ in 0..60 {
        let _ = rig.failing_frame();
        answer = until.try_take();
        if answer.is_some() {
            break;
        }
    }
    let Some(answer) = answer else {
        panic!("a step was queued through a wake that panicked");
    };
    let refused = answer.expect_err("its step's wake panicked");
    assert_eq!(refused.code, ErrorCode::Platform, "{refused:?}");
    assert_eq!(
        refused.message, "the window panicked while answering",
        "{refused:?}"
    );
    // What went out before the panic is the earlier steps'; the step it stopped never plays.
    let journaled = rig.journal().entries.len();
    played_nothing(&mut rig, &go, journaled, "a scroll step");
}

/// A `wait.frames` asks for the frames it counts before it is parked: a wake that panics there
/// parks nothing, and the wait is answered once, that the window panicked; nothing keeps it
/// counting or asking for frames afterwards.
#[test]
fn a_wake_that_panics_as_a_frames_wait_is_parked_answers_it_once() {
    let mut rig = Rig::showing(&Text::new("Still"));
    let frames: FramesParams = serde_json::from_value(serde_json::json!({
        "window": rig.window.id(),
        "frames": 1_000,
        "timeout_ms": 60_000,
    }))
    .expect("frames params read");
    let mut parked = rig
        .window
        .call::<method::WaitFrames>(frames)
        .expect("the inbox has room");
    assert_eq!(rig.failing_turn(), "wake 1");
    let Some(answer) = parked.try_take() else {
        panic!("answered in the turn that served it");
    };
    let refused = answer.expect_err("the wake panicked");
    assert_eq!(refused.code, ErrorCode::Platform, "{refused:?}");
    assert_eq!(
        refused.message, "the window panicked while answering",
        "{refused:?}"
    );
    for _ in 0..4 {
        rig.frame();
    }
    let now = flui_foundation::MonotonicClock::now(&rig.clock);
    let mut unsettled = flui_runtime::settledness::Unsettled::new(now);
    rig.runtime.unsettled(&mut unsettled);
    assert!(
        unsettled
            .causes()
            .all(|cause| cause.cleared_by != flui_protocol::wait::ClearedBy::NextFrame),
        "no parked wait keeps asking for frames: {unsettled:?}"
    );
}

/// A wait answer pass contains each window's wake: when the wake panics for the first window's
/// waits, which keep frames coming, the next window's waits are still answered (one past its
/// timeout ends `unsatisfied`), and the first panic, `wake 1`, continues once every window had
/// its turn.
#[test]
fn a_wake_that_panics_for_one_window_leaves_the_next_windows_waits_answered() {
    let (mut rig, _go) = rig();
    let second = rig.runtime.install_presentation(
        rig.runtime
            .assemble_presentation(crate::owner_publication::window()),
    );
    rig.runtime
        .attach_root_widget_to(second, &Text::new("Second"))
        .expect("mount the second window");
    let other = rig
        .runtime
        .dev_agent_window(second)
        .expect("the runtime hosts its second presentation");
    for _ in 0..4 {
        rig.frame();
    }
    let wait = |window: &flui_view::dev_agent::AgentWindow, timeout_ms: u64| {
        let wait = WaitParams::new(window.id(), Condition::Frames(1_000)).timeout_ms(timeout_ms);
        window
            .call::<method::WaitUntil>(wait)
            .expect("the inbox has room")
    };
    let _first: AgentAnswer<WaitOutcome> = wait(&rig.window, 60_000);
    let mut due: AgentAnswer<WaitOutcome> = wait(&other, 50);
    let _held: AgentAnswer<WaitOutcome> = wait(&other, 60_000);
    rig.runtime.pump_background();
    assert!(due.try_take().is_none(), "parked until its timeout");
    rig.clock.advance(Duration::from_millis(100));
    assert_eq!(rig.failing_turn(), "wake 1");
    let Some(answer) = due.try_take() else {
        panic!("the second window's wait past its timeout was answered");
    };
    let outcome = answer.expect("a timeout is an answer");
    assert_eq!(
        outcome.end,
        flui_protocol::wait::WaitEnd::Unsatisfied,
        "{outcome:?}"
    );
}

/// A box 120 × 40 whose hit-test panics, as a render object that cannot be hit does.
#[derive(Debug, Default)]
struct PanicsWhenHitBox;

impl flui_foundation::inspect::Inspect for PanicsWhenHitBox {
    fn kind(&self) -> &'static str {
        "PanicsWhenHitBox"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl flui_rendering::traits::RenderBox for PanicsWhenHitBox {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut flui_rendering::BoxLayoutContext<'_, flui_foundation::Leaf, Self::ParentData>,
    ) -> flui_rendering::RenderResult<flui_foundation::geometry::Size> {
        Ok(flui_foundation::geometry::Size::new(120.0, 40.0))
    }

    fn paint(&self, _ctx: &mut flui_rendering::PaintContext<'_, flui_foundation::Leaf>) {}

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::BoxHitTestContext<'_, flui_foundation::Leaf, Self::ParentData>,
    ) -> bool {
        panic!("a render object that cannot be hit")
    }
}

#[derive(Clone)]
struct PanicsWhenHit;

impl flui_view::RenderView for PanicsWhenHit {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = PanicsWhenHitBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> PanicsWhenHitBox {
        PanicsWhenHitBox
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut PanicsWhenHitBox,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for PanicsWhenHit {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// "Show", whose press brings "Trap", a button under a box whose hit-test panics; and "Fails",
/// whose handler panics.
#[derive(Clone, StatefulView)]
struct Traps;

struct TrapsState {
    shown: flui_view::Signal<bool>,
}

impl StatefulView for Traps {
    type State = TrapsState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        TrapsState {
            shown: ctx.signal(false),
        }
    }
}

impl ViewState<Traps> for TrapsState {
    fn build(&self, _view: &Traps, ctx: &dyn BuildContext) -> impl IntoView {
        let shown = self.shown;
        let trap = if shown.get(ctx) {
            Stack::new(vec![
                RawButton::new(Text::new("Trap"))
                    .on_pressed(|_| {})
                    .into_view()
                    .boxed(),
                PanicsWhenHit.into_view().boxed(),
            ])
            .into_view()
            .boxed()
        } else {
            SizedBox::new(120.0, 40.0).into_view().boxed()
        };
        Column::new(vec![
            RawButton::new(Text::new("Show"))
                .on_pressed(move |ctx| shown.update(ctx, |on| *on = true))
                .into_view()
                .boxed(),
            trap,
            RawButton::new(Text::new("Fails"))
                .on_pressed(|_: &mut EventContext<'_>| {
                    // Typed as a handler that returns, as a handler that fails sometimes is.
                    if std::hint::black_box(true) {
                        std::panic::panic_any("a handler that fails".to_owned());
                    }
                })
                .into_view()
                .boxed(),
        ])
    }
}

/// App code an attempt of a held act reaches at a frame boundary panics: the act is answered
/// once, with the code and the step that failed, and the panic continues out of the frame. A tap
/// held until its target appears, whose hit-test then panics, answers that the hit-test did; a
/// tap held on the semantic path for its target to hold still, whose handler then panics,
/// answers that the handler did, which may have run.
#[test]
fn app_code_that_panics_in_a_held_acts_attempt_answers_the_act_once() {
    type Asked = fn(&mut Rig) -> AgentAnswer<ActOutcome>;
    let rows: [(&str, Asked, &str, &str); 2] = [
        (
            "a hit-test",
            |rig| {
                let mut tap = TapParams::new(named("Trap"));
                tap.options.timeout_ms = Some(60_000);
                let tap = rig
                    .lease
                    .call::<method::ElementTap>(tap)
                    .expect("the inbox has room");
                rig.runtime.pump_background();
                let mut show = rig
                    .window
                    .call::<method::SemanticsAct>(ActParams::new(named("Show"), ActionName::Invoke))
                    .expect("the inbox has room");
                rig.runtime.pump_background();
                show.try_take()
                    .expect("pressed at the drain")
                    .expect("pressed");
                tap
            },
            "the hit-test at element",
            "a render object that cannot be hit",
        ),
        (
            "a semantics handler",
            |rig| {
                let mut tap = TapParams::new(named("Fails"));
                tap.options.timeout_ms = Some(60_000);
                tap.options.via = Some(InputPolicy::require(InputPath::Semantic));
                let tap = rig
                    .lease
                    .call::<method::ElementTap>(tap)
                    .expect("the inbox has room");
                rig.runtime.pump_background();
                tap
            },
            "the handler of element",
            "a handler that fails",
        ),
    ];
    for (case, ask, says, payload) in rows {
        let mut rig = Rig::showing(&Traps);
        let mut asked = ask(&mut rig);
        assert!(asked.try_take().is_none(), "{case}: held");
        let mut continued = None;
        for _ in 0..10 {
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| rig.frame())) {
                continued = Some(
                    panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                        .unwrap_or_else(|| "a payload of another type".to_owned()),
                );
                break;
            }
        }
        assert_eq!(
            continued.as_deref(),
            Some(payload),
            "{case}: the panic continues"
        );
        let Some(answer) = asked.try_take() else {
            panic!("{case}: answered before the panic continued");
        };
        let refused = answer.expect_err("app code panicked");
        assert_eq!(refused.code, ErrorCode::Platform, "{case}: {refused:?}");
        assert!(refused.message.contains(says), "{case}: {refused:?}");
        for _ in 0..4 {
            let _ = catch_unwind(AssertUnwindSafe(|| rig.frame()));
        }
        assert!(asked.try_take().is_none(), "{case}: answered once");
    }
}

/// A call whose serving catches a panic and hands its reply on (a tap whose hit-test panics as it
/// is held) continues the panic out of the drain, and wakes the owner first when calls are still
/// queued behind it, so they are drained at the next turn; with none queued it wakes no more than
/// holding the act did.
#[test]
fn a_call_that_panics_in_the_drain_wakes_the_owner_for_the_calls_behind_it() {
    let mut extra_wakes = Vec::new();
    for queued_behind in [false, true] {
        let mut rig = Rig::showing(&Traps);
        let mut show = rig
            .window
            .call::<method::SemanticsAct>(ActParams::new(named("Show"), ActionName::Invoke))
            .expect("the inbox has room");
        rig.runtime.pump_background();
        show.try_take()
            .expect("pressed at the drain")
            .expect("pressed");
        for _ in 0..3 {
            rig.frame();
        }
        let mut tap = TapParams::new(named("Trap"));
        tap.options.timeout_ms = Some(60_000);
        let mut tap = rig
            .lease
            .call::<method::ElementTap>(tap)
            .expect("the inbox has room");
        let behind = queued_behind.then(|| {
            rig.window
                .call::<method::InputJournal>(flui_protocol::input::JournalParams::new(
                    rig.window.id(),
                ))
                .expect("the inbox has room")
        });
        // Counted once the calls are queued: what the turn that drains them wakes.
        let before = rig.wakes();
        let drained = catch_unwind(AssertUnwindSafe(|| rig.runtime.pump_background()));
        assert!(
            drained.is_err(),
            "queued {queued_behind}: the hit-test's panic continues"
        );
        let refused = tap
            .try_take()
            .expect("answered before the panic continued")
            .expect_err("its hit-test panicked");
        assert_eq!(refused.code, ErrorCode::Platform, "{refused:?}");
        extra_wakes.push(rig.wakes() - before);
        drop(behind);
    }
    assert_eq!(
        extra_wakes[1],
        extra_wakes[0] + 1,
        "one more wake when a call waits behind the panic: {extra_wakes:?}"
    );
}
