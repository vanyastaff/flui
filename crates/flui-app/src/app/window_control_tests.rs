//! Unit tests for `window_control.rs`, declared there as its child module
//! through `#[path]`, so `super::*` reaches the queue's private items.

#![allow(
    clippy::missing_const_for_thread_local,
    reason = "clippy asks for `const` on the Android target even though the initializer already is"
)]

use crate::app::runner::window_routes::OpenRouted as _;
use std::sync::{Arc, Mutex};

use flui_foundation::geometry::{Point, Size};
use flui_foundation::{PresentationId, UiRuntimeId};
use flui_native::HeadlessPlatform;
use flui_platform::WindowId;
use flui_platform::{CursorError, CursorIcon};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_platform::{WindowCapabilities, WindowMode, WindowOptions};
use flui_view::{WindowClosed, WindowHandle};

use super::*;

fn address(n: usize) -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(n),
        presentation_id: PresentationId::new(n),
    }
}

/// A window the headless platform opened, with the capabilities given.
fn headless_window(capabilities: WindowCapabilities) -> Rc<dyn PlatformWindow> {
    headless_host_window(capabilities)
}

/// The same, as the host hands it out, so a test can reach the headless
/// window's own controls.
fn headless_host_window(capabilities: WindowCapabilities) -> Rc<dyn flui_native::HostWindow> {
    HeadlessPlatform::new()
        .with_window_capabilities(capabilities)
        .open_routed(WindowOptions::default())
        .expect("a headless window opens")
}

struct Rig {
    queues: WindowCommandQueues,
    source: WindowSource,
    handle: WindowHandle,
    wakes: Rc<Cell<usize>>,
}

fn rig(window: &Rc<dyn PlatformWindow>, n: usize) -> Rig {
    rig_with(
        window,
        n,
        WindowCommandQueues::default(),
        CloseRequestRouter::new(),
    )
}

fn rig_with(
    window: &Rc<dyn PlatformWindow>,
    n: usize,
    queues: WindowCommandQueues,
    router: CloseRequestRouter,
) -> Rig {
    let wakes = Rc::new(Cell::new(0));
    let counted = Rc::clone(&wakes);
    let source = window_source(
        window,
        address(n),
        &queues,
        Rc::new(move || {
            counted.set(counted.get() + 1);
            true
        }),
        Rc::new(router),
    );
    let handle = source.handle();
    Rig {
        queues,
        source,
        handle,
        wakes,
    }
}

/// A router that knows the window at `address(n)`, answering with `handler`.
fn router_for(
    window: &Rc<dyn PlatformWindow>,
    n: usize,
    handler: Option<crate::app::CloseRequestHandler>,
) -> CloseRequestRouter {
    let router = CloseRequestRouter::new();
    let prepared =
        crate::app::close_request::PreparedCloseRequest::new(address(n), window, handler);
    router
        .publication(&prepared)
        .expect("a fresh router")
        .publish(prepared);
    router
}

fn commands_wait_for_the_next_turn_and_apply_in_order() {
    let window = headless_window(WindowCapabilities::ALL);
    let rig = rig(&window, 1);
    let before = window.state();
    rig.handle.set_title("A").expect("accepted");
    rig.handle
        .set_size(Size::new(640.0, 480.0))
        .expect("accepted");
    rig.handle.set_title("B").expect("accepted");
    assert_eq!(window.state(), before, "nothing is applied inline");
    assert_eq!(rig.wakes.get(), 3, "each command asks the owner for a turn");

    rig.queues.apply_pending();
    let after = window.state();
    assert_eq!(after.title.as_deref(), Some("B"), "the later title wins");
    assert_eq!(after.content_size, Size::new(640.0, 480.0));
    assert_eq!(
        after.revision.get(),
        before.revision.get() + 3,
        "three commands, three snapshots, in submission order"
    );
}

fn a_refused_command_is_never_queued() {
    let none = headless_window(WindowCapabilities::NONE);
    let rig_none = rig(&none, 2);
    assert_eq!(
        rig_none.handle.set_title("x"),
        Err(SubmitError::Admission(AdmissionError::Unsupported(
            Unsupported::new(UnsupportedReason::NotOnThisPlatform)
        )))
    );
    let all = headless_window(WindowCapabilities::ALL);
    let rig_all = rig(&all, 3);
    assert_eq!(
        rig_all.handle.set_size(Size::new(f64::NAN, 10.0)),
        Err(SubmitError::Admission(AdmissionError::InvalidGeometry))
    );
    assert_eq!(
        rig_all.handle.set_position(Point::new(f64::INFINITY, 0.0)),
        Err(SubmitError::Admission(AdmissionError::InvalidGeometry))
    );
    assert_eq!(
        rig_none.wakes.get() + rig_all.wakes.get(),
        0,
        "no turn asked for"
    );
    let before = all.state();
    rig_all.queues.apply_pending();
    assert_eq!(all.state(), before);
}

/// A material is a command like the others: a window that draws one takes it
/// through the queue at the owner's next turn, and one that draws none
/// refuses it at the submit.
fn a_material_goes_through_the_window_queue() {
    let host = headless_host_window(WindowCapabilities::ALL);
    let window: Rc<dyn PlatformWindow> = host.clone();
    let rig_all = rig(&window, 21);
    let mock = host
        .as_any()
        .downcast_ref::<flui_native::MockWindow>()
        .expect("headless window");
    rig_all
        .handle
        .set_material(flui_platform::WindowMaterial::System)
        .expect("accepted");
    assert_eq!(mock.materials(), [], "nothing is applied inline");
    rig_all.queues.apply_pending();
    assert_eq!(mock.materials(), [flui_platform::WindowMaterial::System]);

    let mut without = WindowCapabilities::ALL;
    without.material = flui_platform::Availability::Unavailable;
    let none = headless_window(without);
    let rig_none = rig(&none, 22);
    assert_eq!(
        rig_none
            .handle
            .set_material(flui_platform::WindowMaterial::System),
        Err(SubmitError::Admission(AdmissionError::Unsupported(
            Unsupported::new(UnsupportedReason::NotOnThisPlatform)
        )))
    );
}

/// The window system revises what the window honours between a submit and
/// the turn that applies it: the queue, the command's caller, admits each
/// queued command again against the snapshot then. What the window still
/// honours applies, in submission order; what it lost is refused and changes
/// nothing; neither is applied under the capabilities it was checked against.
fn a_command_superseded_in_the_queue_is_admitted_again() {
    let host = headless_host_window(WindowCapabilities::ALL);
    let window: Rc<dyn PlatformWindow> = host.clone();
    let rig = rig(&window, 20);
    rig.handle.set_title("kept").expect("accepted");
    rig.handle
        .set_mode(WindowMode::Maximized)
        .expect("accepted while the window maximizes");
    rig.handle
        .set_size(Size::new(300.0, 200.0))
        .expect("accepted");

    let mut revised = WindowCapabilities::ALL;
    revised.maximizes = flui_platform::Availability::Unavailable;
    host.as_any()
        .downcast_ref::<flui_native::MockWindow>()
        .expect("headless window")
        .simulate_capabilities(revised);
    let before = window.state();
    assert_ne!(
        before.capabilities.revision(),
        WindowCapabilities::ALL.revision(),
        "the change is published at a new capabilities revision"
    );

    rig.queues.apply_pending();
    let after = window.state();
    assert_eq!(after.title.as_deref(), Some("kept"));
    assert_eq!(
        after.mode,
        WindowMode::Normal,
        "the maximize the window lost"
    );
    assert_eq!(after.content_size, Size::new(300.0, 200.0));
    assert_eq!(
        after.revision.get(),
        before.revision.get() + 2,
        "the title and the size, nothing for the refused maximize"
    );
}

/// A close the window refuses (its window system lets the application close
/// none of its windows) leaves the window open, so what was queued behind it
/// still applies; only a close that goes ahead, or finds the window closed,
/// ends the batch.
fn a_refused_close_keeps_the_work_queued_behind_it() {
    let mut cannot_close = WindowCapabilities::ALL;
    cannot_close.closes = flui_platform::Availability::Unavailable;
    let window = headless_window(cannot_close);
    let rig = rig_with(
        &window,
        21,
        WindowCommandQueues::default(),
        router_for(&window, 21, None),
    );
    rig.handle.request_close().expect("accepted");
    rig.handle
        .set_title("after the refused close")
        .expect("accepted");
    rig.queues.apply_pending();
    assert_eq!(
        window.state().title.as_deref(),
        Some("after the refused close"),
        "the window stayed, and so did its queue"
    );
}

fn the_handles_of_two_windows_do_not_mix() {
    let first = headless_window(WindowCapabilities::ALL);
    let second = headless_window(WindowCapabilities::ALL);
    let queues = WindowCommandQueues::default();
    let source = |window: &Rc<dyn PlatformWindow>, n| {
        window_source(
            window,
            address(n),
            &queues,
            Rc::new(|| true),
            Rc::new(CloseRequestRouter::new()),
        )
    };
    let (a, b) = (source(&first, 4), source(&second, 5));
    assert_eq!(
        first.state().title,
        second.state().title,
        "two windows with the same title"
    );
    a.handle().set_title("first").expect("accepted");
    queues.apply_pending();
    assert_eq!(first.state().title.as_deref(), Some("first"));
    assert_eq!(
        second.state().title.as_deref(),
        Some(WindowOptions::default().title.as_str()),
        "the other window is untouched"
    );
    assert!(queues.handle_for(address(4)).is_some());
    assert!(queues.handle_for(address(6)).is_none());
    drop(b);
    assert!(
        queues.handle_for(address(5)).is_none(),
        "closed with its source"
    );
}

fn a_closed_window_answers_closed() {
    let window = headless_window(WindowCapabilities::ALL);
    let rig = rig(&window, 6);
    let seen = Rc::new(Cell::new(0));
    let counted = Rc::clone(&seen);
    let (_, _subscription) = rig
        .handle
        .subscribe(move |_| counted.set(counted.get() + 1))
        .expect("open window accepts subscriptions");
    rig.source.close();
    assert_eq!(rig.handle.set_title("late"), Err(SubmitError::Closed));
    assert_eq!(rig.handle.request_close(), Err(WindowClosed));
    assert!(rig.handle.state().is_err());
    assert!(rig.handle.subscribe(|_| {}).is_err());
    rig.source.publish_current();
    assert_eq!(seen.get(), 0, "a closed window delivers nothing");
}

// The handle is not `Send`, so the window's `Send` hook reaches it through the
// test thread's own slot.
thread_local! {
    static REENTRANT: RefCell<Option<WindowHandle>> = const { RefCell::new(None) };
}

/// A window that records what it was asked and can fail or ask for more.
struct RecordingWindow {
    applied: Mutex<Vec<WindowCommand>>,
    fail_titles: bool,
    during_apply: Option<fn()>,
}

impl PlatformWindow for RecordingWindow {
    fn id(&self) -> WindowId {
        WindowId(900)
    }
    fn request_redraw(&self) {}
    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(|| {})
    }
    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        Arc::new(flui_platform::testing::NoNativeSurface)
    }
    flui_platform::window_takes_no_frame_verbs!();

    fn set_cursor(&self, _: CursorIcon) -> Result<(), CursorError> {
        Ok(())
    }
    fn state(&self) -> WindowState {
        WindowState::new(
            flui_platform::WindowGeometry::new(None, Size::new(10.0, 10.0)),
            Size::new(10.0, 10.0),
        )
        .with_visible(true)
        .with_focused(true)
        .with_capabilities(WindowCapabilities::ALL)
    }
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        let command = command.against(&self.state().capabilities)?;
        if self.fail_titles && matches!(command, WindowCommand::SetTitle(_)) {
            return Err(ApplyError::failed("the system said no".into()));
        }
        self.applied.lock().expect("log").push(command);
        if let Some(during) = self.during_apply {
            during();
        }
        Ok(())
    }
    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }
    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }
    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }
}

fn recording(fail_titles: bool, during_apply: Option<fn()>) -> Rc<RecordingWindow> {
    Rc::new(RecordingWindow {
        applied: Mutex::new(Vec::new()),
        fail_titles,
        during_apply,
    })
}

fn applied(window: &RecordingWindow) -> Vec<WindowCommand> {
    window.applied.lock().expect("log").clone()
}

fn a_backend_failure_does_not_stop_the_rest_of_the_queue() {
    let window = recording(true, None);
    let as_platform: Rc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 7);
    rig.handle.set_title("fails").expect("accepted at submit");
    rig.handle.set_resizable(false).expect("accepted");
    rig.handle.set_title("fails again").expect("accepted");
    rig.handle
        .set_mode(WindowMode::Maximized)
        .expect("accepted");
    rig.queues.apply_pending();
    assert_eq!(
        applied(&window),
        [
            WindowCommand::SetResizable(false),
            WindowCommand::SetMode(WindowMode::Maximized)
        ],
        "the commands after a failure still apply, in order"
    );
}

fn a_command_submitted_while_applying_runs_on_the_following_turn() {
    fn ask_again() {
        REENTRANT.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.set_resizable(true).expect("accepted");
            }
        });
    }
    let window = recording(false, Some(ask_again));
    let as_platform: Rc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 8);
    REENTRANT.with(|slot| *slot.borrow_mut() = Some(rig.handle.clone()));
    rig.handle.set_decorated(false).expect("accepted");

    rig.queues.apply_pending();
    assert_eq!(
        applied(&window),
        [WindowCommand::SetDecorated(false)],
        "the command made during the walk waits"
    );
    rig.queues.apply_pending();
    assert_eq!(
        applied(&window),
        [
            WindowCommand::SetDecorated(false),
            WindowCommand::SetResizable(true)
        ]
    );
}

fn a_nested_turn_leaves_the_queue_to_the_walk_in_progress() {
    let window = recording(false, None);
    let as_platform: Rc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 9);
    rig.queues.applying.set(true);
    rig.handle.set_title("held").expect("accepted");
    rig.queues.apply_pending();
    assert!(applied(&window).is_empty(), "a nested call applies nothing");
    rig.queues.applying.set(false);
    rig.queues.apply_pending();
    assert_eq!(applied(&window).len(), 1);
}

fn a_panic_while_applying_neither_wedges_the_walk_nor_loses_the_rest() {
    thread_local! {
        static ARMED: Cell<bool> = const { Cell::new(true) };
    }
    fn explode_once() {
        assert!(!ARMED.replace(false), "a command panics");
    }
    ARMED.set(true);
    let window = recording(false, Some(explode_once));
    let as_platform: Rc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 12);
    rig.handle.set_resizable(false).expect("accepted");
    rig.handle.set_decorated(false).expect("accepted");
    rig.handle.set_title("last").expect("accepted");

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rig.queues.apply_pending();
    }));
    assert!(outcome.is_err(), "the first command panics");
    assert!(
        !rig.queues.applying.get(),
        "the walk is not left marked as in progress"
    );
    assert_eq!(applied(&window), [WindowCommand::SetResizable(false)]);

    rig.queues.apply_pending();
    assert_eq!(
        applied(&window),
        [
            WindowCommand::SetResizable(false),
            WindowCommand::SetDecorated(false),
            WindowCommand::SetTitle("last".into()),
        ],
        "what the panic cut off still applies, in order, on the next turn"
    );
}

fn a_window_closed_between_the_request_and_the_lookup_has_no_handle() {
    let window = headless_window(WindowCapabilities::ALL);
    let rig = rig(&window, 14);
    assert!(
        rig.queues.handle_for(address(14)).is_some(),
        "an open window has one"
    );

    rig.source.close();
    assert!(
        rig.queues.handle_for(address(14)).is_none(),
        "closed, though its source has not been dropped yet"
    );
}

fn a_dead_window_takes_its_queued_commands_with_it() {
    let window = recording(false, None);
    let as_platform: Rc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 13);
    rig.handle.set_title("never applied").expect("accepted");
    rig.handle.request_close().expect("accepted");
    drop((window, as_platform));

    rig.queues.apply_pending();
    let queue = rig.queues.queues.borrow()[0]
        .0
        .upgrade()
        .expect("the source still holds its queue");
    assert!(
        queue.pending.borrow().is_empty(),
        "the batch of a destroyed window is not queued again for every later turn"
    );
}

fn subscribers_hear_a_new_state_in_order_and_what_they_ask_waits() {
    let host = headless_host_window(WindowCapabilities::ALL);
    let window: Rc<dyn PlatformWindow> = host.clone();
    let rig = rig(&window, 10);
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&log);
    let reply = rig.handle.clone();
    let (initial, _subscription) = rig
        .handle
        .subscribe(move |state| {
            seen.borrow_mut().push(state.revision.get());
            reply.set_title("from a subscriber").expect("accepted");
        })
        .expect("open window accepts subscriptions");
    assert!(log.borrow().is_empty(), "subscribing calls nothing");

    let native = host
        .as_any()
        .downcast_ref::<flui_native::MockWindow>()
        .expect("headless window");
    native.simulate_move(Point::new(5.0, 6.0));
    rig.source.publish_current();
    assert_eq!(*log.borrow(), [initial.revision.get() + 1]);
    assert_ne!(
        window.state().title.as_deref(),
        Some("from a subscriber"),
        "the request made inside the callback waits for the next turn"
    );
    rig.source.publish_current();
    assert_eq!(
        log.borrow().len(),
        1,
        "the same revision is not delivered twice"
    );
    rig.queues.apply_pending();
    assert_eq!(window.state().title.as_deref(), Some("from a subscriber"));
}

fn closing_is_queued_with_the_commands_and_never_applied_inline() {
    let window = recording(false, None);
    let as_platform: Rc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 11);
    rig.handle.set_resizable(false).expect("accepted");
    rig.handle.request_close().expect("accepted");
    assert_eq!(rig.wakes.get(), 2);
    rig.queues.apply_pending();
    assert_eq!(
        applied(&window),
        [WindowCommand::SetResizable(false)],
        "the close goes through the close-request path, not `apply`"
    );
}

fn a_panic_in_one_window_still_applies_the_others_and_asks_for_a_turn() {
    thread_local! {
        static ARMED: Cell<bool> = const { Cell::new(true) };
    }
    fn explode_once() {
        assert!(!ARMED.replace(false), "a command panics");
    }
    ARMED.set(true);
    let failing = recording(false, Some(explode_once));
    let healthy = recording(false, None);
    let queues = WindowCommandQueues::default();
    let wakes = Rc::new(Cell::new(0));
    let source = |window: &Rc<dyn PlatformWindow>, n| {
        let counted = Rc::clone(&wakes);
        window_source(
            window,
            address(n),
            &queues,
            Rc::new(move || {
                counted.set(counted.get() + 1);
                true
            }),
            Rc::new(CloseRequestRouter::new()),
        )
    };
    let (failing_platform, healthy_platform): (Rc<dyn PlatformWindow>, Rc<dyn PlatformWindow>) =
        (failing.clone(), healthy.clone());
    let (a, b) = (source(&failing_platform, 15), source(&healthy_platform, 16));
    a.handle().set_resizable(false).expect("accepted");
    a.handle().set_title("cut off").expect("accepted");
    b.handle().set_decorated(false).expect("accepted");
    b.handle().set_title("other").expect("accepted");
    let submitted = wakes.get();

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        queues.apply_pending();
    }));
    assert!(
        outcome.is_err(),
        "the panic is raised once the walk is done"
    );
    assert_eq!(
        applied(&healthy),
        [
            WindowCommand::SetDecorated(false),
            WindowCommand::SetTitle("other".into())
        ],
        "the window after the one that panicked still applies its batch"
    );
    assert_eq!(
        wakes.get(),
        submitted + 1,
        "the window left with work asks for the turn that applies it"
    );

    queues.apply_pending();
    assert_eq!(
        applied(&failing),
        [
            WindowCommand::SetResizable(false),
            WindowCommand::SetTitle("cut off".into())
        ]
    );
}

fn commands_after_a_close_that_goes_ahead_are_dropped_and_after_a_veto_apply() {
    let closing = recording(false, None);
    let as_platform: Rc<dyn PlatformWindow> = closing.clone();
    let rig = rig_with(
        &as_platform,
        18,
        WindowCommandQueues::default(),
        router_for(&as_platform, 18, None),
    );
    rig.handle.set_title("before").expect("accepted");
    rig.handle.request_close().expect("accepted");
    rig.handle.set_resizable(false).expect("accepted");
    rig.queues.apply_pending();
    assert_eq!(
        applied(&closing),
        [
            WindowCommand::SetTitle("before".into()),
            WindowCommand::Close
        ],
        "nothing is applied to a window after its close"
    );
    rig.queues.apply_pending();
    assert_eq!(
        applied(&closing).len(),
        2,
        "nor given back for a later turn"
    );

    let kept = recording(false, None);
    let as_platform: Rc<dyn PlatformWindow> = kept.clone();
    let veto = crate::app::CloseRequestHandler::new(|_| CloseResponse::KeepOpen);
    let rig = rig_with(
        &as_platform,
        19,
        WindowCommandQueues::default(),
        router_for(&as_platform, 19, Some(veto)),
    );
    rig.handle.request_close().expect("accepted");
    rig.handle.set_resizable(false).expect("accepted");
    rig.queues.apply_pending();
    assert_eq!(
        applied(&kept),
        [WindowCommand::SetResizable(false)],
        "a vetoed close leaves the window and the rest of the batch"
    );
}

#[test]
fn window_handle_contract() {
    crate::table_test::run_table(
        "window_handle_contract",
        &[
            (
                "commands_wait_for_the_next_turn_and_apply_in_order",
                commands_wait_for_the_next_turn_and_apply_in_order as fn(),
            ),
            (
                "a_material_goes_through_the_window_queue",
                a_material_goes_through_the_window_queue as fn(),
            ),
            (
                "a_refused_command_is_never_queued",
                a_refused_command_is_never_queued as fn(),
            ),
            (
                "a_command_superseded_in_the_queue_is_admitted_again",
                a_command_superseded_in_the_queue_is_admitted_again as fn(),
            ),
            (
                "a_refused_close_keeps_the_work_queued_behind_it",
                a_refused_close_keeps_the_work_queued_behind_it as fn(),
            ),
            (
                "the_handles_of_two_windows_do_not_mix",
                the_handles_of_two_windows_do_not_mix as fn(),
            ),
            (
                "a_closed_window_answers_closed",
                a_closed_window_answers_closed as fn(),
            ),
            (
                "a_backend_failure_does_not_stop_the_rest_of_the_queue",
                a_backend_failure_does_not_stop_the_rest_of_the_queue as fn(),
            ),
            (
                "a_command_submitted_while_applying_runs_on_the_following_turn",
                a_command_submitted_while_applying_runs_on_the_following_turn as fn(),
            ),
            (
                "a_nested_turn_leaves_the_queue_to_the_walk_in_progress",
                a_nested_turn_leaves_the_queue_to_the_walk_in_progress as fn(),
            ),
            (
                "a_panic_while_applying_neither_wedges_the_walk_nor_loses_the_rest",
                a_panic_while_applying_neither_wedges_the_walk_nor_loses_the_rest as fn(),
            ),
            (
                "a_window_closed_between_the_request_and_the_lookup_has_no_handle",
                a_window_closed_between_the_request_and_the_lookup_has_no_handle as fn(),
            ),
            (
                "a_dead_window_takes_its_queued_commands_with_it",
                a_dead_window_takes_its_queued_commands_with_it as fn(),
            ),
            (
                "subscribers_hear_a_new_state_in_order_and_what_they_ask_waits",
                subscribers_hear_a_new_state_in_order_and_what_they_ask_waits as fn(),
            ),
            (
                "closing_is_queued_with_the_commands_and_never_applied_inline",
                closing_is_queued_with_the_commands_and_never_applied_inline as fn(),
            ),
            (
                "a_panic_in_one_window_still_applies_the_others_and_asks_for_a_turn",
                a_panic_in_one_window_still_applies_the_others_and_asks_for_a_turn as fn(),
            ),
            (
                "commands_after_a_close_that_goes_ahead_are_dropped_and_after_a_veto_apply",
                commands_after_a_close_that_goes_ahead_are_dropped_and_after_a_veto_apply as fn(),
            ),
        ],
    );
}
