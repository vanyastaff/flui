//! The scenario `Tester` over its headless connection (ADR-0400): a read runs no frame, a chord
//! leaves a key a gesture holds as it was, and the end of a test runs or skips its checks as
//! §7 says, losing no failure.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{AnimationController, DrivenController, RepeatMode};
use flui_foundation::panic::payload_text;
use flui_interaction::focus::KeyEventResult;
use flui_platform::input::keyboard::{Key, KeyEvent, KeyPress, KeyState, Modifiers};
use flui_protocol::client::Connection as _;
use flui_protocol::input::{ActionSequence, InputSource, KeyAction};
use flui_testing::log_capture::{CapturedLog, capture};
use flui_testing::scenario::{TestError, Tester, find, matcher};
use flui_testing::widgets::SharedSignal;
use flui_view::environment::VsyncScope;
use flui_view::prelude::*;
use flui_widgets::{SizedBox, Text};

/// Shows the number a test sets, as text.
#[derive(Clone, StatefulView)]
struct Shown {
    number: SharedSignal<u32>,
}

struct ShownState {
    number: SharedSignal<u32>,
}

impl StatefulView for Shown {
    type State = ShownState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ShownState {
            number: self.number.clone(),
        }
    }
}

impl ViewState<Shown> for ShownState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.number.bind(ctx);
    }

    fn build(&self, _view: &Shown, ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.number.get(ctx).to_string())
    }
}

/// A widget whose disposal hands its running controller to the test instead of disposing it:
/// the ticker it leaves keeps the application busy once the root is unmounted.
#[derive(Clone, StatefulView)]
struct LeftTicking {
    kept: Rc<RefCell<Vec<DrivenController>>>,
}

struct LeftTickingState {
    controller: Option<DrivenController>,
    kept: Rc<RefCell<Vec<DrivenController>>>,
}

impl StatefulView for LeftTicking {
    type State = LeftTickingState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        LeftTickingState {
            controller: Some(AnimationController::builder(Duration::from_secs(1)).build_on(None)),
            kept: Rc::clone(&self.kept),
        }
    }
}

impl ViewState<LeftTicking> for LeftTickingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let controller = self
            .controller
            .as_mut()
            .expect("the controller is held until disposal");
        controller
            .rebind(VsyncScope::maybe_of(ctx).as_ref())
            .expect("the window's frame registry takes the controller");
        let _ = controller.controller().repeat(RepeatMode::Restart);
    }

    fn build(&self, _view: &LeftTicking, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("Ticking")
    }

    fn dispose(&mut self) {
        if let Some(controller) = self.controller.take() {
            self.kept.borrow_mut().push(controller);
        }
    }
}

/// A key event as `down`/`up`/`repeat`, its key, and `^` for each of Control and Shift held.
fn described(event: &KeyEvent) -> String {
    let state = match event.state {
        KeyState::Down(KeyPress::AutoRepeat) => "repeat",
        KeyState::Down(_) => "down",
        KeyState::Up => "up",
    };
    let key = match &event.key {
        Key::Named(named) => named.as_str().to_owned(),
        Key::Character(text) => text.as_str().to_owned(),
        other => format!("{other:?}"),
    };
    let held = [Modifiers::CONTROL, Modifiers::SHIFT]
        .into_iter()
        .filter(|modifier| event.modifiers.contains(*modifier))
        .count();
    format!("{state} {key}{}", "^".repeat(held))
}

/// What a focused element heard, each key event [`described`].
fn key_log() -> (Tester, Rc<RefCell<Vec<String>>>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    let heard = Rc::clone(&log);
    let t = Tester::mount(
        flui_widgets::Focus::new(SizedBox::new(40.0, 40.0))
            .autofocus(true)
            .on_key_event(move |_ctx, event| {
                heard.borrow_mut().push(described(event));
                KeyEventResult::Handled
            }),
    );
    (t, log)
}

/// Records every key event its window's focus manager dispatches, [`described`], through a
/// global key handler that lives as long as the window: unlike a focused element, it still
/// hears the keys released after the root is unmounted.
#[derive(Clone, StatefulView)]
struct WindowKeys {
    heard: Rc<RefCell<Vec<String>>>,
}

struct WindowKeysState {
    heard: Rc<RefCell<Vec<String>>>,
}

impl StatefulView for WindowKeys {
    type State = WindowKeysState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        WindowKeysState {
            heard: Rc::clone(&self.heard),
        }
    }
}

impl ViewState<WindowKeys> for WindowKeysState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let heard = Rc::clone(&self.heard);
        ctx.focus_manager()
            .add_global_key_handler(Rc::new(move |event: &KeyEvent| {
                heard.borrow_mut().push(described(event));
                KeyEventResult::Ignored
            }));
    }

    fn build(&self, _view: &WindowKeys, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(40.0, 40.0)
    }
}

/// Shift pressed by a gesture of the test's own, and left down when it ends.
fn shift_down() -> ActionSequence {
    ActionSequence::new(vec![InputSource::Keys {
        id: "held".to_owned(),
        actions: vec![KeyAction::Down {
            key: "Shift".to_owned(),
        }],
    }])
}

/// The checks the end of a test reported as not passed, by name.
fn unpassed_checks(log: &CapturedLog) -> Vec<String> {
    log.records()
        .iter()
        .filter(|record| record.message == "an end check did not pass")
        .filter_map(|record| record.field("check").map(str::to_owned))
        .collect()
}

/// Why the end checks did not run, when the end said so.
fn skipped_because(log: &CapturedLog) -> Option<String> {
    log.records()
        .iter()
        .find(|record| record.message == "the end checks did not run")
        .and_then(|record| record.field("reason").map(str::to_owned))
}

/// A read is an owner turn with no frame (ADR-0400 §4): with a frame due, the read leaves the
/// frame number where it was and reads the tree that frame has not replaced yet.
///
/// Fails against a read that runs the frame loop to get its answer.
#[test]
fn a_read_leaves_the_frame_number_unchanged() {
    let number = SharedSignal::new(10);
    let mut t = Tester::mount(Shown {
        number: number.clone(),
    });
    number.set(12);
    let before = t.connection().now().frame;
    let node = t.read(&find::text("10"));
    assert_eq!(node.name.as_deref(), Some("10"));
    assert_eq!(
        t.connection().now().frame,
        before,
        "a read presents no frame, though one is due"
    );
    t.pump();
    t.expect(&find::text("12"), matcher::exists());
    assert!(t.connection().now().frame > before, "the frame was due");
}

/// A verb that raised and whose panic the test caught poisons the tester: `Drop` checks
/// nothing, says why, and the failure is the panic's.
///
/// Fails against a tester whose raise does not poison it: its `Drop` would run the checks and
/// report the ticker the test left.
#[test]
fn a_caught_panic_then_drop_skips_the_end_checks() {
    let kept = Rc::new(RefCell::new(Vec::new()));
    let ((), log) = capture(|| {
        let mut t = Tester::mount(LeftTicking {
            kept: Rc::clone(&kept),
        });
        let caught = catch_unwind(AssertUnwindSafe(|| t.read(&find::text("Missing"))));
        let payload = caught.expect_err("a read of a missing text raises");
        let message = payload_text(&*payload).expect("a raised error panics with its text");
        assert!(
            message.contains("Missing"),
            "the panic is the failure: {message}"
        );
        drop(t);
    });
    let skipped = skipped_because(&log).expect("the end says its checks did not run");
    assert!(
        skipped.contains("poisoned") && skipped.contains("Missing"),
        "the reason names the poison and the first failure: {skipped}"
    );
    assert_eq!(unpassed_checks(&log), Vec::<String>::new());
}

/// An error a `try_` verb returned, travelling through `?`, is alive when the tester drops:
/// `Drop` checks nothing, and the error reaches the caller.
///
/// Fails against a report that does not count a returned error as alive.
#[test]
fn an_error_through_the_question_mark_then_drop_skips_the_end_checks() {
    fn body(kept: &Rc<RefCell<Vec<DrivenController>>>) -> Result<(), TestError> {
        let mut t = Tester::mount(LeftTicking {
            kept: Rc::clone(kept),
        });
        t.try_read(&find::text("Missing"))?;
        Ok(())
    }

    let kept = Rc::new(RefCell::new(Vec::new()));
    let (result, log) = capture(|| body(&kept));
    assert!(
        matches!(result, Err(TestError::NotFound { .. })),
        "the error reaches the caller: {result:?}"
    );
    let skipped = skipped_because(&log).expect("the end says its checks did not run");
    assert!(skipped.contains("still alive"), "{skipped}");
    assert_eq!(unpassed_checks(&log), Vec::<String>::new());
}

/// An error the test expected and dropped leaves the tester whole: `Drop` runs the end checks,
/// which find the ticker the root left running while the runtime is still alive.
///
/// Fails against an end that skips its checks for an error no longer alive, and against an end
/// check that reads the list after the runtime retired.
#[test]
fn an_expected_error_dropped_before_a_leaked_ticker_reports_the_ticker() {
    let kept = Rc::new(RefCell::new(Vec::new()));
    let ((), log) = capture(|| {
        let mut t = Tester::mount(LeftTicking {
            kept: Rc::clone(&kept),
        });
        assert!(t.try_read(&find::text("Missing")).is_err());
        drop(t);
    });
    assert_eq!(skipped_because(&log), None);
    assert!(
        unpassed_checks(&log)
            .iter()
            .any(|check| check == "left_behind"),
        "the leaked ticker fails the end: {:?}",
        unpassed_checks(&log)
    );
    assert_eq!(
        kept.borrow().len(),
        1,
        "the widget left its controller running"
    );
}

/// A gesture that leaves a key down holds it in the agent's lease: the end of the test releases
/// it, the window hears the key come up before it closes, and the end checks pass.
///
/// Fails against an end that keeps the lease, or that drops it but runs no turn to play the
/// release it queued.
#[test]
fn a_test_that_ends_holding_a_key_passes_its_end_checks() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let mut t = Tester::mount(WindowKeys {
        heard: Rc::clone(&heard),
    });
    t.connection_mut()
        .perform(&shift_down())
        .expect("the agent presses Shift");
    assert_eq!(*heard.borrow(), ["down Shift^"]);
    if let Err(report) = t.try_finish() {
        panic!("{report}");
    }
    assert_eq!(
        *heard.borrow(),
        ["down Shift^", "up Shift"],
        "the end releases the key the test left down"
    );
}

/// A chord naming a key a gesture holds leaves that key as it was: no second down, no repeat,
/// and no release (ADR-0400 §2); the other keys of the chord are pressed and released.
///
/// Fails against a chord that presses each of its keys whatever the lease holds.
#[test]
fn a_chord_after_a_gesture_that_left_a_key_down_sends_no_second_press() {
    let (mut t, log) = key_log();
    t.connection_mut()
        .perform(&shift_down())
        .expect("the agent presses Shift");
    t.press_keys(&["Shift+Tab"]);
    assert_eq!(*log.borrow(), ["down Shift^", "down Tab^", "up Tab^"]);
}
