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
use flui_testing::drop_ledger::DropSentinel;
use flui_testing::log_capture::{CapturedLog, capture};
use flui_testing::scenario::{TestError, TestReport, Tester, find, matcher};
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

/// The end-check names a [`TestReport`] lists.
fn report_checks(report: &TestReport) -> Vec<String> {
    report
        .unpassed
        .iter()
        .map(|unpassed| unpassed.check().name.clone())
        .collect()
}

/// An error a `try_` verb returned skips the end checks only while the tester drops, where it
/// may be on its way out through `?`. An explicit finish is the test saying it is done: it runs
/// the checks beside a kept error, and they find the ticker the root left.
///
/// Fails against an end that skips its checks for a kept error on every exit.
#[test]
fn a_kept_error_skips_the_end_checks_only_when_the_tester_drops() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum End {
        Finish,
        Drop,
    }
    let rows = [
        ("kept, finish", true, End::Finish, true),
        ("kept, drop", true, End::Drop, false),
        ("dropped, finish", false, End::Finish, true),
        ("dropped, drop", false, End::Drop, true),
    ];
    for (row, keep, end, checked) in rows {
        let kept = Rc::new(RefCell::new(Vec::new()));
        let (checks, log) = capture(|| {
            let mut t = Tester::mount(LeftTicking {
                kept: Rc::clone(&kept),
            });
            let error = t
                .try_read(&find::text("Missing"))
                .expect_err("nothing is named so");
            let held = keep.then_some(error);
            let checks = match end {
                End::Finish => {
                    let report = t.try_finish().expect_err("the ticker fails the end");
                    report_checks(&report)
                }
                End::Drop => {
                    drop(t);
                    Vec::new()
                }
            };
            drop(held);
            checks
        });
        let ran = match end {
            End::Finish => checks.iter().any(|check| check == "left_behind"),
            End::Drop => unpassed_checks(&log)
                .iter()
                .any(|check| check == "left_behind"),
        };
        assert_eq!(ran, checked, "{row}: whether the end checks ran");
        assert_eq!(
            skipped_because(&log).is_some(),
            end == End::Drop && !checked,
            "{row}: whether the end says it skipped"
        );
    }
}

/// Holds the [`DropSentinel`] the test hands it, and lets it go as it is disposed.
#[derive(Clone, StatefulView)]
struct Holding {
    slot: Rc<RefCell<Option<DropSentinel>>>,
}

struct HoldingState {
    slot: Rc<RefCell<Option<DropSentinel>>>,
}

impl StatefulView for Holding {
    type State = HoldingState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        HoldingState {
            slot: Rc::clone(&self.slot),
        }
    }
}

impl ViewState<Holding> for HoldingState {
    fn build(&self, _view: &Holding, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("Holding")
    }

    fn dispose(&mut self) {
        drop(self.slot.borrow_mut().take());
    }
}

/// The end unmounts the root before it reads the ledger: a sentinel the tree holds until it is
/// disposed is dropped by then, and one the test keeps is reported as not dropped.
///
/// Fails against an end that reads the ledger before it unmounts the root, or never unmounts it.
#[test]
fn the_end_reads_the_ledger_after_the_root_is_unmounted() {
    for (row, test_keeps, clean) in [
        ("the tree holds it", false, true),
        ("the test keeps it", true, false),
    ] {
        let slot = Rc::new(RefCell::new(None));
        let t = Tester::mount(Holding {
            slot: Rc::clone(&slot),
        });
        let sentinel = t.ledger().plant();
        let kept = if test_keeps {
            Some(sentinel)
        } else {
            *slot.borrow_mut() = Some(sentinel);
            None
        };
        let result = t.try_finish();
        match result {
            Ok(()) => assert!(clean, "{row}: the end passed"),
            Err(report) => {
                assert!(!clean, "{row}: {report}");
                assert_eq!(report_checks(&report), ["drop_ledger"], "{row}: {report}");
            }
        }
        drop(kept);
    }
}

/// A test body that panics with a live tester unwinds through its `Drop`: the end checks do not
/// run, the process does not abort, and the panic that reaches the test is the body's own. The
/// body runs on a thread of its own, so the panic is a real unwind out of the body.
///
/// Fails against an end that runs its checks while the thread unwinds: it reports the ticker the
/// body's root left.
#[test]
fn a_test_that_panics_with_a_live_tester_keeps_its_own_failure() {
    let (message, skipped, unpassed) = std::thread::spawn(|| {
        let (caught, log) = capture(|| {
            catch_unwind(AssertUnwindSafe(|| {
                let kept = Rc::new(RefCell::new(Vec::new()));
                let _t = Tester::mount(LeftTicking {
                    kept: Rc::clone(&kept),
                });
                panic!("the body's own failure");
            }))
        });
        let payload = caught.expect_err("the body panics");
        let message = payload_text(&*payload).unwrap_or_default().to_owned();
        (message, skipped_because(&log), unpassed_checks(&log))
    })
    .join()
    .expect("the thread ends without an abort or a second panic");
    assert_eq!(message, "the body's own failure");
    assert!(
        skipped
            .as_deref()
            .is_some_and(|reason| reason.contains("unwinding")),
        "{skipped:?}"
    );
    assert_eq!(unpassed, Vec::<String>::new());
}

/// `finish_unchecked` ends the test without its checks, saying why: the ticker its root left
/// fails nothing.
///
/// Fails against a `finish_unchecked` that runs the checks.
#[test]
fn finish_unchecked_ends_without_the_checks_and_says_why() {
    let kept = Rc::new(RefCell::new(Vec::new()));
    let ((), log) = capture(|| {
        let t = Tester::mount(LeftTicking {
            kept: Rc::clone(&kept),
        });
        t.finish_unchecked("the widget keeps its ticker on purpose");
    });
    assert!(
        log.records().iter().any(|record| {
            record.message == "a test ended without its end checks"
                && record.field("reason") == Some("the widget keeps its ticker on purpose")
        }),
        "{log}"
    );
    assert_eq!(unpassed_checks(&log), Vec::<String>::new());
}

/// A find nothing matches lists the elements whose text shares a word with the one sought:
/// "Save draft" for "Save", not "e", whose letter merely occurs in the find's description.
///
/// Fails against candidates chosen by comparing names with the debug form of the find.
#[test]
fn a_missing_text_lists_the_elements_that_share_its_text() {
    let mut t = Tester::mount(flui_widgets::Column::new((
        Text::new("Save draft"),
        Text::new("e"),
    )));
    let Err(TestError::NotFound { diagnosis, .. }) = t.try_read(&find::text("Save")) else {
        panic!("nothing is named \"Save\"");
    };
    assert!(
        diagnosis
            .candidates
            .iter()
            .any(|line| line.contains("\"Save draft\"")),
        "{:?}",
        diagnosis.candidates
    );
    assert!(
        !diagnosis
            .candidates
            .iter()
            .any(|line| line.contains("\"e\"")),
        "{:?}",
        diagnosis.candidates
    );
    drop(diagnosis);
    t.finish();
}

/// A strict read over more matches than it names reports how many match, though it asks the
/// window for no more than it names. (The bound on what the window sends is not observable
/// through the tester: a read without it names the same candidates.)
///
/// Fails against a strict read that counts only the matches reported.
#[test]
fn an_ambiguous_read_counts_every_match_and_names_ten() {
    let rows: Vec<BoxedView> = (0..12)
        .map(|_| Text::new("Row").into_view().boxed())
        .collect();
    let mut t = Tester::mount(flui_widgets::Column::new(rows));
    let Err(TestError::Ambiguous {
        matched, diagnosis, ..
    }) = t.try_read(&find::text("Row"))
    else {
        panic!("twelve elements are named \"Row\"");
    };
    assert_eq!(matched, 12);
    assert_eq!(diagnosis.candidates.len(), 10);
    drop(diagnosis);
    t.finish();
}

/// A soft failure a panic in the `checks` body left unraised is written to the test's output when
/// the test ends with `finish_unchecked`, which runs no end checks but loses no failure.
///
/// Fails against a `finish_unchecked` that drops the report's soft failures.
#[test]
fn finish_unchecked_writes_a_soft_failure_left_unraised() {
    let ((), log) = capture(|| {
        let mut t = Tester::mount(Text::new("Shown"));
        let caught = catch_unwind(AssertUnwindSafe(|| {
            t.checks(|c| {
                c.expect(&find::text("Missing"), matcher::exists());
                panic!("the body fails after a soft failure");
            });
        }));
        assert!(caught.is_err());
        t.finish_unchecked("the body's panic is the failure under test");
    });
    let unraised: Vec<&str> = log
        .records()
        .iter()
        .filter(|record| record.message == "a soft failure was never raised")
        .filter_map(|record| record.field("failure"))
        .collect();
    assert!(
        unraised.len() == 1 && unraised[0].contains("Missing"),
        "{log}"
    );
}

/// A value whose destructor panics: what an application's state may hold.
struct PanicsOnDrop;

impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        panic!("a drop failed");
    }
}

/// Holds a [`PanicsOnDrop`] in its state, so the state's drop panics as the end unmounts it.
#[derive(Clone, StatefulView)]
struct PanicsOnDispose;

struct PanicsOnDisposeState {
    _held: PanicsOnDrop,
}

impl StatefulView for PanicsOnDispose {
    type State = PanicsOnDisposeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        PanicsOnDisposeState {
            _held: PanicsOnDrop,
        }
    }
}

impl ViewState<PanicsOnDispose> for PanicsOnDisposeState {
    fn build(&self, _view: &PanicsOnDispose, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("Disposing")
    }
}

/// A panic in the application's code as the end unmounts the root is the test's failure, raised
/// after the end still releases the key the test left down, and what the checks before it found
/// (an unnamed field failing the audit) is written to the test's output.
///
/// Fails against an end whose unmount unwinds past the release and the report of the audit.
#[test]
fn a_panic_as_the_root_unmounts_still_releases_and_keeps_the_audit() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (caught, log) = capture(|| {
        let mut t = Tester::mount(flui_widgets::Column::new((
            WindowKeys {
                heard: Rc::clone(&heard),
            },
            PanicsOnDispose,
            SizedBox::new(200.0, 40.0).child(flui_widgets::EditableText::new(
                flui_widgets::TextEditingController::new(),
                flui_interaction::focus::FocusNode::with_debug_label("unnamed"),
            )),
        )));
        t.connection_mut()
            .perform(&shift_down())
            .expect("the agent presses Shift");
        catch_unwind(AssertUnwindSafe(move || t.try_finish()))
    });
    let payload = caught.expect_err("the drop's panic fails the end");
    let message = payload_text(&*payload).unwrap_or_default().to_owned();
    assert!(message.contains("a drop failed"), "{message}");
    assert_eq!(
        *heard.borrow(),
        ["down Shift^", "up Shift"],
        "the end released the key after the panic"
    );
    assert_eq!(unpassed_checks(&log), ["audit"], "{log}");
}
