//! A gesture's answer is what happened, whichever user code around it panics (ADR-0287,
//! ADR-0400 §2). A wake of the platform that panics as an act is held, or as its input is
//! queued, leaves nothing to play; and when two wakes of one turn panic, the act's and a later
//! one (the window's, which still holds acts, or a parked wait's), the first continues.
//!
//! The headless host's wake cannot fail, so these drive a runtime of their own, on a manual
//! clock, whose wake panics while the test says so, each time with its own number.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock};
use flui_protocol::method;
use flui_protocol::{
    act::ActOutcome, act::ActParams, error::ErrorCode, input::InputPath, input::InputPolicy,
    input::Journal, input::JournalParams, input::PressKeysParams, input::TapParams,
    locator::Locator, locator::Target, snapshot::Snapshot, wait::Condition, wait::WaitOutcome,
    wait::WaitParams, wire::ActionName,
};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_view::dev_agent::{AgentAnswer, AgentWindow, InputLease};
use flui_view::prelude::*;
use flui_widgets::{Column, RawButton, Text};

/// "Go", which counts its presses, and "Gated", which has no handler and so is disabled.
#[derive(Clone, StatelessView)]
struct Pads(Arc<AtomicU32>);

impl StatelessView for Pads {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let go = Arc::clone(&self.0);
        Column::new(vec![
            RawButton::new(Text::new("Go"))
                .on_pressed(move |_: &mut EventContext<'_>| {
                    go.fetch_add(1, Ordering::SeqCst);
                })
                .into_view()
                .boxed(),
            RawButton::new(Text::new("Gated")).into_view().boxed(),
        ])
    }
}

struct Discard;

impl FrameSink for Discard {
    fn surface_size(&mut self) -> (u32, u32) {
        (200, 200)
    }

    fn submit(
        &mut self,
        _epoch: flui_foundation::FrameEpoch,
        _scene: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

/// [`Pads`] shown by a runtime of its own, its agent window and a lease on its input. While
/// `failing` is set, its wake panics with `wake <n>`, `n` counting the wakes that panicked.
struct Rig {
    runtime: UiRuntime,
    clock: ManualClock,
    failing: Arc<AtomicBool>,
    window: AgentWindow,
    lease: InputLease,
    go: Arc<AtomicU32>,
}

impl Rig {
    fn new() -> Self {
        let clock = ManualClock::new();
        let failing = Arc::new(AtomicBool::new(false));
        let wake = {
            let failing = Arc::clone(&failing);
            let failed = AtomicU32::new(0);
            Arc::new(move || {
                if failing.load(Ordering::SeqCst) {
                    let n = failed.fetch_add(1, Ordering::SeqCst) + 1;
                    std::panic::panic_any(format!("wake {n}"));
                }
            })
        };
        let runtime = UiRuntime::try_new(
            crate::owner_publication::window(),
            1.0,
            RuntimeHostServices::new(
                wake,
                Arc::new(AtomicBool::new(false)),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Manual(clock.clone()),
            ),
        )
        .expect("runtime");
        let go = Arc::new(AtomicU32::new(0));
        runtime
            .attach_root_widget(&Pads(Arc::clone(&go)))
            .expect("mount");
        let window = runtime
            .dev_agent_window(runtime.presentation_id())
            .expect("the runtime hosts its primary presentation");
        let lease = window
            .input_lease()
            .expect("an open window leases its input");
        let mut rig = Self {
            runtime,
            clock,
            failing,
            window,
            lease,
            go,
        };
        for _ in 0..4 {
            rig.frame();
        }
        rig
    }

    fn frame(&mut self) {
        self.clock.advance(Duration::from_millis(16));
        let _ = self.runtime.pump(
            &mut flui_runtime::pump::SampledClock(self.clock.now()),
            &mut Discard,
        );
    }

    /// An owner turn with no frame whose wakes fail: the panic it continues with.
    fn failing_turn(&mut self) -> String {
        self.failing.store(true, Ordering::SeqCst);
        let turned = catch_unwind(AssertUnwindSafe(|| self.runtime.pump_background()));
        self.failing.store(false, Ordering::SeqCst);
        let payload = turned.expect_err("a panic continues out of the turn");
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "a payload of another type".to_owned())
    }

    /// The window's input journal, read within 30 frames.
    fn journal(&mut self) -> Journal {
        let mut read = self
            .window
            .call::<method::InputJournal>(JournalParams::new(self.window.id()))
            .expect("the inbox has room");
        for _ in 0..30 {
            if let Some(journal) = read.try_take() {
                return journal.expect("the journal is read");
            }
            self.frame();
        }
        panic!("the journal is not read within 30 frames");
    }

    /// No input went out since the journal held `journaled` entries, over 20 frames, and "Go"
    /// was never pressed.
    fn played_nothing(&mut self, journaled: usize, case: &str) {
        for _ in 0..20 {
            self.frame();
        }
        assert_eq!(self.go.load(Ordering::SeqCst), 0, "{case}: nothing pressed");
        assert_eq!(
            self.journal().entries.len(),
            journaled,
            "{case}: no input of it went out"
        );
    }
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
        let mut rig = Rig::new();
        let journaled = rig.journal().entries.len();
        let mut asked = ask(&rig);
        assert_eq!(rig.failing_turn(), "wake 1", "{case}");
        let Some(answer) = asked.try_take() else {
            panic!("{case}: answered in the turn that served it");
        };
        let refused = answer.expect_err("the wake panicked");
        assert_eq!(refused.code, ErrorCode::Platform, "{case}: {refused:?}");
        rig.played_nothing(journaled, case);
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
        let mut rig = Rig::new();
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
        rig.played_nothing(journaled, case);
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
        let mut rig = Rig::new();
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
        assert_eq!(rig.go.load(Ordering::SeqCst), 1, "{case}: pressed once");
    }
}
