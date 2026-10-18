//! Unit tests for `window_control.rs`, declared there as its child module
//! through `#[path]`, so `super::*` reaches the queue's private items.

#![allow(
    clippy::missing_const_for_thread_local,
    reason = "clippy asks for `const` on the Android target even though the initializer already is"
)]

use std::any::Any;
use std::sync::Mutex;

use flui_foundation::geometry::{Point, Size};
use flui_foundation::{PresentationId, UiRuntimeId};
use flui_platform::traits::WindowId;
use flui_platform::{HeadlessPlatform, Platform as _, WindowMode, WindowOptions};
use flui_platform_api::{CursorError, CursorIcon};
use flui_view::WindowHandle;

use super::*;

fn address(n: usize) -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(n),
        presentation_id: PresentationId::new(n),
    }
}

/// A window the headless platform opened, with the capabilities given.
fn headless_window(capabilities: WindowCapabilities) -> Arc<dyn PlatformWindow> {
    let window: Arc<dyn PlatformWindow> = HeadlessPlatform::new()
        .with_window_capabilities(capabilities)
        .open_window(WindowOptions::default())
        .expect("a headless window opens");
    window
}

struct Rig {
    queues: WindowCommandQueues,
    source: WindowSource,
    handle: WindowHandle,
    wakes: Rc<Cell<usize>>,
}

fn rig(window: &Arc<dyn PlatformWindow>, n: usize) -> Rig {
    let queues = WindowCommandQueues::default();
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
        Arc::new(CloseRequestRouter::new()),
    );
    let handle = source.handle();
    Rig {
        queues,
        source,
        handle,
        wakes,
    }
}

fn commands_wait_for_the_next_turn_and_apply_in_order() {
    let window = headless_window(WindowCapabilities::all());
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
    let none = headless_window(WindowCapabilities::none());
    let rig_none = rig(&none, 2);
    assert_eq!(
        rig_none.handle.set_title("x"),
        Err(WindowCommandError::Unsupported)
    );
    let all = headless_window(WindowCapabilities::all());
    let rig_all = rig(&all, 3);
    assert_eq!(
        rig_all.handle.set_size(Size::new(f64::NAN, 10.0)),
        Err(WindowCommandError::InvalidGeometry)
    );
    assert_eq!(
        rig_all.handle.set_position(Point::new(f64::INFINITY, 0.0)),
        Err(WindowCommandError::InvalidGeometry)
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

fn the_handles_of_two_windows_do_not_mix() {
    let first = headless_window(WindowCapabilities::all());
    let second = headless_window(WindowCapabilities::all());
    let queues = WindowCommandQueues::default();
    let source = |window: &Arc<dyn PlatformWindow>, n| {
        window_source(
            window,
            address(n),
            &queues,
            Rc::new(|| true),
            Arc::new(CloseRequestRouter::new()),
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
    let window = headless_window(WindowCapabilities::all());
    let rig = rig(&window, 6);
    let seen = Rc::new(Cell::new(0));
    let counted = Rc::clone(&seen);
    let (_, _subscription) = rig
        .handle
        .subscribe(move |_| counted.set(counted.get() + 1))
        .expect("open window accepts subscriptions");
    rig.source.close();
    assert_eq!(
        rig.handle.set_title("late"),
        Err(WindowCommandError::Closed)
    );
    assert_eq!(rig.handle.close(), Err(WindowCommandError::Closed));
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
    fn physical_size(&self) -> Size<i32> {
        Size::new(10, 10)
    }
    fn logical_size(&self) -> Size<f64> {
        Size::new(10.0, 10.0)
    }
    fn scale_factor(&self) -> f64 {
        1.0
    }
    fn request_redraw(&self) {}
    fn is_focused(&self) -> bool {
        true
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn set_cursor(&self, _: CursorIcon) -> Result<(), CursorError> {
        Ok(())
    }
    fn capabilities(&self) -> WindowCapabilities {
        WindowCapabilities::all()
    }
    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        if self.fail_titles && matches!(command, WindowCommand::SetTitle(_)) {
            return Err(WindowCommandError::Backend("the system said no".into()));
        }
        self.applied.lock().expect("log").push(command);
        if let Some(during) = self.during_apply {
            during();
        }
        Ok(())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn recording(fail_titles: bool, during_apply: Option<fn()>) -> Arc<RecordingWindow> {
    Arc::new(RecordingWindow {
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
    let as_platform: Arc<dyn PlatformWindow> = window.clone();
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
    let as_platform: Arc<dyn PlatformWindow> = window.clone();
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
    let as_platform: Arc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 9);
    rig.queues.applying.set(true);
    rig.handle.set_title("held").expect("accepted");
    rig.queues.apply_pending();
    assert!(applied(&window).is_empty(), "a nested call applies nothing");
    rig.queues.applying.set(false);
    rig.queues.apply_pending();
    assert_eq!(applied(&window).len(), 1);
}

fn subscribers_hear_a_new_state_in_order_and_what_they_ask_waits() {
    let window = headless_window(WindowCapabilities::all());
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

    let native = window
        .as_any()
        .downcast_ref::<flui_platform::MockWindow>()
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
    let as_platform: Arc<dyn PlatformWindow> = window.clone();
    let rig = rig(&as_platform, 11);
    rig.handle.set_resizable(false).expect("accepted");
    rig.handle.close().expect("accepted");
    assert_eq!(rig.wakes.get(), 2);
    rig.queues.apply_pending();
    assert_eq!(
        applied(&window),
        [WindowCommand::SetResizable(false)],
        "the close goes through the close-request path, not `apply`"
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
                "a_refused_command_is_never_queued",
                a_refused_command_is_never_queued as fn(),
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
                "subscribers_hear_a_new_state_in_order_and_what_they_ask_waits",
                subscribers_hear_a_new_state_in_order_and_what_they_ask_waits as fn(),
            ),
            (
                "closing_is_queued_with_the_commands_and_never_applied_inline",
                closing_is_queued_with_the_commands_and_never_applied_inline as fn(),
            ),
        ],
    );
}
