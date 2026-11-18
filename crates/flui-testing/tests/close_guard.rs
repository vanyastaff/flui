//! A presentation's close guard as its host consults it: a held close is
//! recorded instead of carried out, and the pump after the last hold is
//! released carries it out once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_runtime::owner_turn::{CloseRequestHandler, CloseResponse};
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        GuardedState {
            view: self.clone(),
            observation: None,
        }
    }
}

impl View for Guarded {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<Guarded> for GuardedState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let seen = self.view.seen.clone();
        let guard = ctx
            .close_guard()
            .expect("a hosted presentation has a guard");
        if self.view.hold {
            seen.holds.borrow_mut().push(guard.hold());
        }
        seen.guard.replace(Some(guard));
        let detached = Rc::clone(&seen.detached);
        let (_, observation) = ctx
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

    fn build(&self, _view: &Guarded, _ctx: &dyn BuildContext) -> impl IntoView {
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

/// Panics as its window detaches.
#[derive(Clone)]
struct FailsAsItDetaches;

/// Holds the subscription for its drop alone.
struct FailsAsItDetachesState {
    _observation: flui_view::LifecycleSubscription,
}

impl StatefulView for FailsAsItDetaches {
    type State = FailsAsItDetachesState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let (_, observation) = ctx
            .lifecycle_handle()
            .expect("a hosted presentation has a lifecycle")
            .subscribe(|state| {
                assert!(
                    state != AppLifecycleState::Detached,
                    "an observer fails as its window detaches"
                );
            })
            .expect("the presentation is open");
        FailsAsItDetachesState {
            _observation: observation,
        }
    }
}

impl View for FailsAsItDetaches {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<FailsAsItDetaches> for FailsAsItDetachesState {
    fn build(&self, _view: &FailsAsItDetaches, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// A host with a plain primary window and two windows that hold their close,
/// the first of them failing as it detaches.
fn two_held_windows() -> (
    HeadlessHost,
    flui_testing::HeadlessWindowId,
    flui_testing::HeadlessWindowId,
    Seen,
    Seen,
) {
    let mut host = HeadlessHost::new(HeadlessWindow::new(10, 10));
    let (failing, other) = (Seen::default(), Seen::default());
    let first = host.open_window(HeadlessWindow::new(10, 10));
    let second = host.open_window(HeadlessWindow::new(10, 10));
    host.attach_to(
        first,
        &flui_widgets::Column::new((
            Guarded {
                hold: true,
                seen: failing.clone(),
            },
            FailsAsItDetaches,
        )),
    )
    .expect("first root");
    host.attach_to(
        second,
        &Guarded {
            hold: true,
            seen: other.clone(),
        },
    )
    .expect("second root");
    let _ = host.pump(Duration::ZERO);
    (host, first, second, failing, other)
}

/// Two closes released in one turn are both carried out though the first
/// one's observer panics: the panic reaches the test once both closed.
///
/// Fails against a turn that carries the closes out uncontained: the second
/// window, its close already taken out of its guard, stays open.
pub(crate) fn every_due_close_is_carried_out_past_one_that_panics() {
    let (mut host, first, second, failing, other) = two_held_windows();
    assert!(!host.request_close(first, CloseReason::User));
    assert!(!host.request_close(second, CloseReason::User));
    failing.holds.borrow_mut().clear();
    other.holds.borrow_mut().clear();

    let turn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.pump(Duration::ZERO)));
    assert!(turn.is_err(), "the observer's panic reaches the test");
    assert!(host.is_closed(first));
    assert!(
        host.is_closed(second),
        "the second close was carried out past the first one's panic"
    );
}

/// A close the program asked for that a hold refused is, once the hold is
/// released, the window's consent to a quit, as a runner treats it: no quit
/// is asked, so the window stays open.
///
/// Fails against a host that carries every due close out whatever its reason.
pub(crate) fn a_held_program_close_is_consent_to_a_quit_and_closes_nothing() {
    let (mut host, first, _second, failing, _other) = two_held_windows();
    assert!(!host.request_close(first, CloseReason::Program));
    failing.holds.borrow_mut().clear();
    let _ = host.pump(Duration::ZERO);
    assert!(
        !host.is_closed(first),
        "the program's close closes nothing alone"
    );
    // The first window's observer fails as the host's drop detaches it.
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(host)));
    assert!(dropped.is_err());
}

/// A quit no window holds closes every window at the next turn and retires
/// the host.
pub(crate) fn a_quit_nothing_holds_closes_every_window() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(10, 10));
    let second = host.open_window(HeadlessWindow::new(10, 10));
    let _ = host.pump(Duration::ZERO);
    host.request_quit();
    assert!(!host.is_retired(), "the next turn asks the windows");
    let _ = host.pump(Duration::ZERO);
    assert!(host.is_closed(second));
    assert!(host.is_retired(), "the quit closed every window");
}

/// A quit a window's hold refuses waits for the hold: the windows stay as
/// they were, and the turn after the release closes them all.
///
/// Fails against a quit that closes past every hold.
pub(crate) fn a_quit_a_hold_refuses_waits_for_its_release() {
    let (mut host, first, second, failing, other) = two_held_windows();
    host.request_quit();
    let _ = host.pump(Duration::ZERO);
    assert!(!host.is_closed(first) && !host.is_closed(second), "held");
    failing.holds.borrow_mut().clear();
    let _ = host.pump(Duration::ZERO);
    assert!(!host.is_retired(), "the second window still holds it");
    other.holds.borrow_mut().clear();
    let quit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.pump(Duration::ZERO)));
    assert!(
        quit.is_err(),
        "the first window's observer fails as it detaches"
    );
    assert!(host.is_retired(), "the released quit closed every window");
}

/// A quit a window decides to stay open for is cancelled: every window stays
/// as it was.
pub(crate) fn a_quit_a_window_stays_open_for_is_cancelled() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(10, 10));
    let seen = Seen::default();
    host.attach(&Guarded {
        hold: false,
        seen: seen.clone(),
    })
    .expect("root");
    let _ = host.pump(Duration::ZERO);
    let guard = seen.guard.borrow().clone().expect("guard acquired");
    let decision = guard.require_decision();
    host.request_quit();
    let _ = host.pump(Duration::ZERO);
    guard
        .pending()
        .expect("the quit's close is recorded")
        .stay_open()
        .expect("the recording is current");
    let _ = host.pump(Duration::ZERO);
    drop(decision);
    let _ = host.pump(Duration::ZERO);
    assert!(
        !host.is_retired(),
        "the window stayed open: the quit is over"
    );
    assert_eq!(seen.detached.get(), 0);
}

/// The session's end follows the platform the window names, as a runner
/// does: a desktop closes every window past its hold and retires; a phone
/// keeps every window open for the user's return.
///
/// Fails against a host that keeps every window open on every platform.
pub(crate) fn a_session_end_follows_the_platform_the_window_names() {
    use flui_platform::{SessionEndAnswer, SessionEndPhase, TargetPlatform};

    for (platform, ends_application) in [
        (TargetPlatform::Windows, true),
        (TargetPlatform::MacOS, true),
        (TargetPlatform::Linux, true),
        (TargetPlatform::Android, false),
        (TargetPlatform::iOS, false),
    ] {
        let mut host = HeadlessHost::new(HeadlessWindow::new(10, 10).with_platform(platform));
        let seen = Seen::default();
        host.attach(&Guarded {
            hold: true,
            seen: seen.clone(),
        })
        .expect("root");
        let _ = host.pump(Duration::ZERO);
        assert_eq!(
            host.end_session(SessionEndPhase::Ending),
            SessionEndAnswer::Proceed,
            "{platform:?}: nothing is in flight"
        );
        assert_eq!(
            host.is_retired(),
            ends_application,
            "{platform:?}: the session's end ends the application: {ends_application}"
        );
        assert_eq!(seen.detached.get(), usize::from(ends_application));
    }
}

/// A close handler that records the reason of every close it is asked and
/// keeps the window open.
fn keeping_open(asked: &Rc<RefCell<Vec<CloseReason>>>) -> CloseRequestHandler {
    let asked = Rc::clone(asked);
    CloseRequestHandler::new(move |request| {
        asked.borrow_mut().push(request.reason());
        CloseResponse::KeepOpen
    })
}

/// The application's close handler is asked as a runner asks it: after the
/// window's guard for the user's close, so a hold answers without it; first
/// at the session's end, so its refusal holds the session though the guard
/// holds only work; and the window that refused releases the platform once
/// it carries its close out.
///
/// Fails against a host that asks only the guard: the handler is never asked
/// and the session end proceeds past it.
pub(crate) fn a_close_handler_is_asked_after_the_guard_and_first_at_a_session_end() {
    use flui_platform::{SessionEndAnswer, SessionEndPhase, TargetPlatform};

    let asked = Rc::new(RefCell::new(Vec::new()));
    let mut host = HeadlessHost::new(
        HeadlessWindow::new(10, 10)
            .with_platform(TargetPlatform::Windows)
            .with_close_request_handler(keeping_open(&asked)),
    );
    let seen = Seen::default();
    host.attach(&Guarded {
        hold: true,
        seen: seen.clone(),
    })
    .expect("root");
    let _ = host.pump(Duration::ZERO);
    let window = host.primary_window();

    assert!(!host.request_close(window, CloseReason::User));
    assert!(
        asked.borrow().is_empty(),
        "the guard's hold answers the user's close before the handler"
    );
    assert_eq!(
        host.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block,
        "the handler refuses the session end a hold for work does not"
    );
    assert_eq!(*asked.borrow(), [CloseReason::SessionEnd]);
    assert_eq!(
        host.session_end_releases(),
        0,
        "the refusal holds the session"
    );

    seen.holds.borrow_mut().clear();
    let _ = host.pump(Duration::ZERO);
    assert!(host.is_closed(window), "the released close is carried out");
    assert_eq!(
        host.session_end_releases(),
        1,
        "the window that refused carried its close out and released the platform"
    );
}

/// Records the notices its window hears and keeps a block on every session
/// end it is told of.
#[derive(Clone, Default)]
struct HoldsSessionEnds {
    heard: Rc<RefCell<Vec<&'static str>>>,
    blocks: Rc<RefCell<Vec<flui_platform::SessionEndBlock>>>,
}

struct HoldsSessionEndsState {
    _notices: flui_view::HostNoticeSubscription,
}

impl StatefulView for HoldsSessionEnds {
    type State = HoldsSessionEndsState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let view = self.clone();
        let notices = ctx
            .lifecycle_handle()
            .expect("a hosted presentation has a lifecycle")
            .subscribe_notices(move |notice| match notice {
                flui_platform::HostNotice::SessionEnding(request) => {
                    view.heard.borrow_mut().push("ending");
                    view.blocks.borrow_mut().push(
                        request
                            .block("a draft is being saved")
                            .expect("the query is not answered yet"),
                    );
                }
                flui_platform::HostNotice::SessionEndCancelled => {
                    view.heard.borrow_mut().push("cancelled");
                }
                _ => {}
            })
            .expect("the presentation is open");
        HoldsSessionEndsState { _notices: notices }
    }
}

impl View for HoldsSessionEnds {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl ViewState<HoldsSessionEnds> for HoldsSessionEndsState {
    fn build(&self, _view: &HoldsSessionEnds, _ctx: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::new(10.0, 10.0)
    }
}

/// A session-end query reaches the window's subscribers as a runner's does,
/// a block one of them takes holds the session until it is released, which
/// lets the platform go on, and a cancel is heard too.
///
/// Fails against a host that asks only the windows' guards: no subscriber
/// hears the query, and the session end proceeds.
pub(crate) fn a_session_end_query_reaches_the_subscribers_and_their_block_holds_it() {
    use flui_platform::{SessionEndAnswer, SessionEndPhase, TargetPlatform};

    let mut host =
        HeadlessHost::new(HeadlessWindow::new(10, 10).with_platform(TargetPlatform::MacOS));
    let view = HoldsSessionEnds::default();
    host.attach(&view).expect("root");
    let _ = host.pump(Duration::ZERO);

    assert_eq!(
        host.end_session(SessionEndPhase::Query),
        SessionEndAnswer::Block
    );
    assert_eq!(host.session_end_releases(), 0);
    view.blocks.borrow_mut().clear();
    assert_eq!(
        host.session_end_releases(),
        1,
        "the last block's release lets the platform go on"
    );
    let _ = host.end_session(SessionEndPhase::Cancelled);
    assert_eq!(*view.heard.borrow(), ["ending", "cancelled"]);
}

/// A quit asks every window's handler, not only up to the first refusal; it
/// waits for each window whose refusal recorded its close, and goes ahead
/// once both records are carried out.
///
/// Fails against a host that asks only the windows' guards: no handler is
/// asked, and the quit closes both windows at once.
pub(crate) fn a_quit_asks_every_windows_handler_and_waits_for_each_record() {
    let (first_asked, second_asked) = (
        Rc::new(RefCell::new(Vec::new())),
        Rc::new(RefCell::new(Vec::new())),
    );
    let mut host = HeadlessHost::new(
        HeadlessWindow::new(10, 10).with_close_request_handler(keeping_open(&first_asked)),
    );
    let second = host.open_window(
        HeadlessWindow::new(10, 10).with_close_request_handler(keeping_open(&second_asked)),
    );
    let (first_seen, second_seen) = (Seen::default(), Seen::default());
    host.attach(&Guarded {
        hold: false,
        seen: first_seen.clone(),
    })
    .expect("first root");
    host.attach_to(
        second,
        &Guarded {
            hold: false,
            seen: second_seen.clone(),
        },
    )
    .expect("second root");
    let _ = host.pump(Duration::ZERO);

    host.request_quit();
    let _ = host.pump(Duration::ZERO);
    assert_eq!(*first_asked.borrow(), [CloseReason::Program]);
    assert_eq!(*second_asked.borrow(), [CloseReason::Program]);
    assert!(!host.is_retired(), "both windows refused");

    for seen in [&first_seen, &second_seen] {
        assert!(!host.is_retired(), "a record is still waited on");
        seen.guard
            .borrow()
            .clone()
            .expect("guard acquired")
            .pending()
            .expect("the refusal recorded the quit's close")
            .discard_and_close()
            .expect("the recording is current");
        let _ = host.pump(Duration::ZERO);
    }
    assert!(
        host.is_retired(),
        "both records carried out: the quit went ahead"
    );
}

#[test]
fn close_guard_hosting() {
    crate::run_table(
        "close_guard_hosting",
        &[
            (
                "a_close_handler_is_asked_after_the_guard_and_first_at_a_session_end",
                a_close_handler_is_asked_after_the_guard_and_first_at_a_session_end,
            ),
            (
                "a_session_end_query_reaches_the_subscribers_and_their_block_holds_it",
                a_session_end_query_reaches_the_subscribers_and_their_block_holds_it,
            ),
            (
                "a_quit_asks_every_windows_handler_and_waits_for_each_record",
                a_quit_asks_every_windows_handler_and_waits_for_each_record,
            ),
            (
                "every_due_close_is_carried_out_past_one_that_panics",
                every_due_close_is_carried_out_past_one_that_panics,
            ),
            (
                "a_held_program_close_is_consent_to_a_quit_and_closes_nothing",
                a_held_program_close_is_consent_to_a_quit_and_closes_nothing,
            ),
            (
                "a_quit_nothing_holds_closes_every_window",
                a_quit_nothing_holds_closes_every_window,
            ),
            (
                "a_quit_a_hold_refuses_waits_for_its_release",
                a_quit_a_hold_refuses_waits_for_its_release,
            ),
            (
                "a_quit_a_window_stays_open_for_is_cancelled",
                a_quit_a_window_stays_open_for_is_cancelled,
            ),
            (
                "a_session_end_follows_the_platform_the_window_names",
                a_session_end_follows_the_platform_the_window_names,
            ),
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
