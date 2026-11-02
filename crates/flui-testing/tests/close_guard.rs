//! A presentation's close guard as its host consults it: a held close is
//! recorded instead of carried out, and the pump after the last hold is
//! released carries it out once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_testing::widgets::{lay_out, tight};
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_view::{
    AppLifecycleState, BuildContext, CloseGuard, CloseHold, CloseReason, IntoView,
    LifecycleContext, StatefulView, View, ViewState,
};

/// What a [`Guarded`] view saw of its presentation's close.
#[derive(Clone, Default)]
struct Seen {
    /// The guard the view acquired.
    guard: Rc<RefCell<Option<CloseGuard>>>,
    /// The holds the view took, kept beyond the view's own life.
    holds: Rc<RefCell<Vec<CloseHold>>>,
    /// How many times its presentation was told it is detached.
    detached: Rc<Cell<usize>>,
}

/// Acquires its presentation's close guard, takes a hold over the close when
/// `hold` is set, and counts the presentation's Detached deliveries.
#[derive(Clone)]
struct Guarded {
    hold: bool,
    seen: Seen,
}

struct GuardedState {
    view: Guarded,
    observation: Option<flui_view::LifecycleSubscription>,
}

impl StatefulView for Guarded {
    type State = GuardedState;

    fn create_state(&self) -> Self::State {
        GuardedState {
            view: self.clone(),
            observation: None,
        }
    }
}

impl View for Guarded {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<Guarded> for GuardedState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let seen = self.view.seen.clone();
        let guard = cx.close_guard().expect("a hosted presentation has a guard");
        if self.view.hold {
            seen.holds.borrow_mut().push(guard.hold());
        }
        seen.guard.replace(Some(guard));
        let detached = Rc::clone(&seen.detached);
        let (_, observation) = cx
            .lifecycle_handle()
            .expect("a hosted presentation has a lifecycle")
            .subscribe(move |state| {
                if state == AppLifecycleState::Detached {
                    detached.set(detached.get() + 1);
                }
            })
            .expect("the presentation is open");
        self.observation = Some(observation);
    }

    fn build(&self, _view: &Guarded, _cx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// A close the guard holds is recorded, not carried out, and the pump after
/// the hold is released carries it out once, without asking again.
pub(crate) fn a_held_close_is_carried_out_once_the_hold_is_released() {
    let seen = Seen::default();
    let mut tree = lay_out(
        Guarded {
            hold: true,
            seen: seen.clone(),
        },
        tight(10.0, 10.0),
    );

    assert!(
        !tree.request_close(CloseReason::User),
        "the hold refuses it"
    );
    assert_eq!(seen.detached.get(), 0, "a refused close tears nothing down");
    assert!(!tree.is_closed(), "and does not close the presentation");
    let guard = seen.guard.borrow().clone().expect("guard acquired");
    assert_eq!(
        guard.pending().map(|pending| pending.reason()),
        Some(CloseReason::User)
    );

    tree.pump_for(Duration::ZERO);
    assert_eq!(seen.detached.get(), 0, "a held close stays recorded");

    seen.holds.borrow_mut().clear();
    tree.pump_for(Duration::ZERO);
    assert_eq!(seen.detached.get(), 1, "the released close is carried out");
    assert!(tree.is_closed(), "and the presentation is closed");
    tree.pump_for(Duration::ZERO);
    assert_eq!(seen.detached.get(), 1, "and carried out once");
}

/// A close waiting for the user's decision stays open until the decision:
/// kept open, a later release closes nothing; closed without saving, the
/// pump closes the presentation past the hold.
pub(crate) fn a_decided_close_is_carried_out_past_the_hold() {
    let seen = Seen::default();
    let mut tree = lay_out(
        Guarded {
            hold: false,
            seen: seen.clone(),
        },
        tight(10.0, 10.0),
    );
    let guard = seen.guard.borrow().clone().expect("guard acquired");
    let decision = guard.require_decision();

    assert!(!tree.request_close(CloseReason::User));
    guard
        .pending()
        .expect("the close is recorded")
        .stay_open()
        .expect("the recording is current");
    tree.pump_for(Duration::ZERO);
    assert!(!tree.is_closed(), "kept open");

    assert!(!tree.request_close(CloseReason::User));
    guard
        .pending()
        .expect("the close is recorded again")
        .discard_and_close()
        .expect("the recording is current");
    tree.pump_for(Duration::ZERO);
    assert!(tree.is_closed(), "closed past the hold");
    assert_eq!(seen.detached.get(), 1);
    drop(decision);
}

/// Each window of one UI runtime has a guard of its own: a hold in one window
/// keeps that window open and lets its sibling close.
pub(crate) fn each_window_of_one_runtime_holds_its_own_close() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(10, 10));
    let held = Seen::default();
    let free = Seen::default();
    let held_window = host.open_window(HeadlessWindow::new(10, 10));
    let free_window = host.open_window(HeadlessWindow::new(10, 10));
    host.attach_to(
        held_window,
        &Guarded {
            hold: true,
            seen: held.clone(),
        },
    )
    .expect("held root");
    host.attach_to(
        free_window,
        &Guarded {
            hold: false,
            seen: free.clone(),
        },
    )
    .expect("free root");
    let _ = host.pump(Duration::ZERO);

    assert!(
        host.request_close(free_window, CloseReason::User),
        "nothing holds the sibling's close"
    );
    assert!(
        !host.request_close(held_window, CloseReason::User),
        "the hold refuses its own window's close"
    );
    let _ = host.pump(Duration::ZERO);
    assert_eq!(free.detached.get(), 1);
    assert_eq!(held.detached.get(), 0, "the held window stays open");

    held.holds.borrow_mut().clear();
    let _ = host.pump(Duration::ZERO);
    assert_eq!(held.detached.get(), 1, "the released close is carried out");
}

/// The guard belongs to the presentation, not to the root: a hold taken
/// before the root is replaced still refuses the close, and the new root
/// finds the same recorded close.
pub(crate) fn a_guard_survives_a_root_remount() {
    let first = Seen::default();
    let mut tree = lay_out(
        Guarded {
            hold: true,
            seen: first.clone(),
        },
        tight(10.0, 10.0),
    );
    tree.pump_widget(flui_widgets::SizedBox::new(10.0, 10.0));
    let second = Seen::default();
    tree.pump_widget(Guarded {
        hold: false,
        seen: second.clone(),
    });

    assert!(
        !tree.request_close(CloseReason::User),
        "the hold from before the remount still refuses the close"
    );
    let guard = second.guard.borrow().clone().expect("guard acquired");
    assert_eq!(
        guard.pending().map(|pending| pending.reason()),
        Some(CloseReason::User),
        "the remounted root sees the same guard"
    );

    first.holds.borrow_mut().clear();
    tree.pump_for(Duration::ZERO);
    assert_eq!(
        second.detached.get(),
        1,
        "the released close is carried out"
    );
}
/// A session that ends past a close waiting for the user's decision: `Query`
/// is refused while the decision is open, `Cancelled` withdraws only that
/// record, and the user's close stays.
pub(crate) fn a_cancelled_session_end_withdraws_only_its_own_record() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(10, 10));
    let seen = Seen::default();
    host.attach(&Guarded {
        hold: true,
        seen: seen.clone(),
    })
    .expect("root");
    let _ = host.pump(Duration::ZERO);
    let window = host.primary_window();
    let guard = seen.guard.borrow().clone().expect("guard acquired");
    seen.holds.borrow_mut().push(guard.require_decision());

    assert!(!host.request_close(window, CloseReason::User));
    assert_eq!(
        host.end_session(flui_platform::SessionEndPhase::Query),
        flui_platform::SessionEndAnswer::Block
    );
    let _ = host.end_session(flui_platform::SessionEndPhase::Cancelled);

    assert_eq!(
        guard.pending().map(|pending| pending.reason()),
        Some(CloseReason::User),
        "the user's close outlives the cancelled session end"
    );
}

#[test]
fn close_guard_hosting() {
    crate::run_table(
        "close_guard_hosting",
        &[
            (
                "a_held_close_is_carried_out_once_the_hold_is_released",
                a_held_close_is_carried_out_once_the_hold_is_released,
            ),
            (
                "a_decided_close_is_carried_out_past_the_hold",
                a_decided_close_is_carried_out_past_the_hold,
            ),
            (
                "each_window_of_one_runtime_holds_its_own_close",
                each_window_of_one_runtime_holds_its_own_close,
            ),
            (
                "a_guard_survives_a_root_remount",
                a_guard_survives_a_root_remount,
            ),
            (
                "a_cancelled_session_end_withdraws_only_its_own_record",
                a_cancelled_session_end_withdraws_only_its_own_record,
            ),
        ],
    );
}
