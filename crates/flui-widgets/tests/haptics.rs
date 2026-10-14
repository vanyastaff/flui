//! Widget haptics through `LifecycleContext::haptics` (ADR-0210 §6, §10):
//! the user preference and `HapticsMode` suppress both tiers, and a
//! frame-aligned request plays with the frame the runtime presents.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_interaction::HapticsHandle;
use flui_platform_api::haptics::{
    HapticOutcome, HapticPattern, HapticRequest, HapticTicket, HapticTiming,
};
use flui_platform_api::{HapticFeedback, SystemPreferences};
use flui_view::element::ElementKind;
use flui_view::{
    BuildContext, IntoView, LifecycleContext, StatefulView, View, ViewExt as _, ViewState,
};
use flui_widgets::{Column, GestureDetector, HapticsMode, HitTestBehavior, SizedBox};

use crate::common::harness::mount;

type Slot = Rc<RefCell<Option<HapticsHandle>>>;

/// Keeps the haptics handle its element acquired, re-acquiring it when an
/// enclosing `HapticsMode` changes.
#[derive(Clone)]
struct Probe {
    slot: Slot,
}

struct ProbeState {
    slot: Slot,
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> ProbeState {
        ProbeState {
            slot: Rc::clone(&self.slot),
        }
    }
}

impl ViewState<Probe> for ProbeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.slot.borrow_mut() = ctx.haptics();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        *self.slot.borrow_mut() = ctx.haptics();
    }

    fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }
}

impl View for Probe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

fn probe() -> (Probe, Slot) {
    let slot = Slot::default();
    (
        Probe {
            slot: Rc::clone(&slot),
        },
        slot,
    )
}

fn handle(slot: &Slot) -> HapticsHandle {
    slot.borrow()
        .clone()
        .expect("a mounted element acquires the presentation's haptics")
}

fn pattern() -> HapticPattern {
    HapticPattern::builder()
        .transient(Duration::ZERO, 0.8, 0.4)
        .build()
        .expect("valid")
}

/// Both tiers at both timings.
fn round(handle: &HapticsHandle) -> [HapticOutcome; 4] {
    let now = HapticRequest::new(HapticTiming::Immediate);
    let frame = HapticRequest::new(HapticTiming::FramePresented);
    [
        handle.perform(HapticFeedback::Confirm, now),
        handle.play(&pattern(), HapticFeedback::Confirm, now),
        handle.perform(HapticFeedback::Confirm, frame),
        handle.play(&pattern(), HapticFeedback::Confirm, frame),
    ]
}

fn plays(outcomes: [HapticOutcome; 4]) -> bool {
    matches!(
        outcomes,
        [
            HapticOutcome::Submitted,
            HapticOutcome::Submitted,
            HapticOutcome::Queued(_),
            HapticOutcome::Queued(_)
        ]
    )
}

fn suppressed(outcomes: [HapticOutcome; 4]) -> bool {
    outcomes == [HapticOutcome::Suppressed; 4]
}

fn the_user_preference_suppresses_both_tiers() {
    let (view, slot) = probe();
    let harness = mount(view);
    let handle = handle(&slot);
    assert!(plays(round(&handle)), "unobserved: haptics play");
    harness.set_system_preferences(SystemPreferences::default().with_haptics(false));
    let before = harness.window().haptic_actuator().calls().len();
    assert!(suppressed(round(&handle)));
    assert_eq!(
        harness.window().haptic_actuator().calls().len(),
        before,
        "a suppressed request never reaches the backend"
    );
    harness.set_system_preferences(SystemPreferences::default().with_haptics(true));
    assert!(plays(round(&handle)));
}

fn a_switched_off_subtree_suppresses_both_tiers() {
    let (outside, outside_slot) = probe();
    let (inside, inside_slot) = probe();
    let (nested, nested_slot) = probe();
    let harness = mount(Column::new(vec![
        outside.into_view().boxed(),
        HapticsMode::new(Column::new(vec![
            inside.into_view().boxed(),
            HapticsMode::new(nested).enabled(true).into_view().boxed(),
        ]))
        .enabled(false)
        .into_view()
        .boxed(),
    ]));
    assert!(plays(round(&handle(&outside_slot))));
    assert!(suppressed(round(&handle(&inside_slot))));
    assert!(
        suppressed(round(&handle(&nested_slot))),
        "a nested enabled mode cannot switch haptics back on"
    );
    assert!(
        handle(&inside_slot).capabilities(None) == handle(&outside_slot).capabilities(None),
        "the switch suppresses requests; the actuator is the same"
    );
    drop(harness);
}

fn turning_the_switch_back_on_reaches_the_mounted_element() {
    let (view, slot) = probe();
    let mut harness = mount(HapticsMode::new(view.clone()).enabled(false));
    assert!(suppressed(round(&handle(&slot))));
    harness.swap_root(HapticsMode::new(view).enabled(true));
    assert!(
        plays(round(&handle(&slot))),
        "did_change_dependencies re-acquired an enabled handle"
    );
}

#[test]
fn haptics_preference_and_subtree_switch_suppress_both_tiers() {
    crate::common::cases::run_cases(
        "haptics preference and subtree switch",
        &[
            (
                "the_user_preference_suppresses_both_tiers",
                the_user_preference_suppresses_both_tiers,
            ),
            (
                "a_switched_off_subtree_suppresses_both_tiers",
                a_switched_off_subtree_suppresses_both_tiers,
            ),
            (
                "turning_the_switch_back_on_reaches_the_mounted_element",
                turning_the_switch_back_on_reaches_the_mounted_element,
            ),
        ],
    );
}

type Delivered = Rc<RefCell<Vec<(HapticTicket, HapticOutcome)>>>;

fn observed(handle: &HapticsHandle) -> Delivered {
    let delivered = Delivered::default();
    let sink = Rc::clone(&delivered);
    handle.set_observer(move |ticket, outcome| sink.borrow_mut().push((ticket, outcome)));
    delivered
}

fn frame_request(handle: &HapticsHandle) -> HapticTicket {
    match handle.perform(
        HapticFeedback::DragLift,
        HapticRequest::new(HapticTiming::FramePresented),
    ) {
        HapticOutcome::Queued(ticket) => ticket,
        outcome => panic!("expected a queued request, got {outcome:?}"),
    }
}

fn a_presented_frame_plays_the_queued_request() {
    let (view, slot) = probe();
    let mut harness = mount(view);
    let handle = handle(&slot);
    let delivered = observed(&handle);
    let ticket = frame_request(&handle);
    assert_eq!(harness.window().haptic_actuator().calls(), []);
    // A visible change goes with the request; the frame that paints it
    // presents the request.
    harness.swap_root(Column::new(vec![
        Probe {
            slot: Rc::clone(&slot),
        }
        .into_view()
        .boxed(),
    ]));
    assert_eq!(
        harness.window().haptic_actuator().feedback(),
        [HapticFeedback::DragLift],
        "the request played with the presented frame"
    );
    harness.tick();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Submitted)]);
}

fn a_frame_never_presented_suppresses_the_request() {
    let (view, slot) = probe();
    let mut harness = mount(view);
    let handle = handle(&slot);
    let delivered = observed(&handle);
    let ticket = frame_request(&handle);
    // Nothing changed, so nothing paints; the deadline wake resolves it.
    harness.pump_for(Duration::from_millis(100));
    harness.tick();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Suppressed)]);
    assert_eq!(harness.window().haptic_actuator().calls(), []);
}

fn hiding_the_window_drains_the_queue() {
    let (view, slot) = probe();
    let mut harness = mount(view);
    let handle = handle(&slot);
    let delivered = observed(&handle);
    let ticket = frame_request(&handle);
    harness.set_window_visible(false);
    harness.tick();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Suppressed)]);
    assert_eq!(
        handle.perform(
            HapticFeedback::Drop,
            HapticRequest::new(HapticTiming::Immediate)
        ),
        HapticOutcome::Suppressed,
        "nothing plays while the window is hidden"
    );
    harness.set_window_visible(true);
    assert_eq!(harness.window().haptic_actuator().calls(), []);
}

fn a_request_in_dispatch_carries_its_sequence() {
    let slot = Slot::default();
    let tapped = Rc::clone(&slot);
    let harness = mount(Column::new(vec![
        Probe {
            slot: Rc::clone(&slot),
        }
        .into_view()
        .boxed(),
        GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(move |_| {
                let _ = handle(&tapped).perform(
                    HapticFeedback::SelectionClick,
                    HapticRequest::new(HapticTiming::Immediate),
                );
            })
            .child(SizedBox::square(40.0))
            .into_view()
            .boxed(),
    ]));
    let _ = handle(&slot).perform(
        HapticFeedback::LightImpact,
        HapticRequest::new(HapticTiming::Immediate),
    );
    harness.dispatch_pointer_down(20.0, 30.0);
    harness.dispatch_pointer_up(20.0, 30.0);
    let calls = harness.window().haptic_actuator().calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0].request.sequence, None, "outside dispatch");
    assert!(
        calls[1].request.sequence.is_some(),
        "the tap's handler ran inside its contact's sequence"
    );
}

#[test]
fn frame_presented_widget_haptics_follow_the_presentation() {
    crate::common::cases::run_cases(
        "frame-presented widget haptics",
        &[
            (
                "a_presented_frame_plays_the_queued_request",
                a_presented_frame_plays_the_queued_request,
            ),
            (
                "a_frame_never_presented_suppresses_the_request",
                a_frame_never_presented_suppresses_the_request,
            ),
            (
                "hiding_the_window_drains_the_queue",
                hiding_the_window_drains_the_queue,
            ),
            (
                "a_request_in_dispatch_carries_its_sequence",
                a_request_in_dispatch_carries_its_sequence,
            ),
        ],
    );
}
