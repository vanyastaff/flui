//! The platform calls the application's and a window's handlers with none of
//! its locks or borrows held, on every host: a handler may install another,
//! quit or open a window from inside its call; a replaced handler's captures,
//! whose destructor calls the platform, are dropped after the platform let go
//! of its state; and a handler's panic stays the first failure, its displaced
//! captures retained rather than dropped into a second one.
//!
//! A case that regresses deadlocks rather than fails, so each one runs on a
//! worker thread and is reported after [`TIMEOUT`]: the worker stays parked
//! for the rest of the process.

use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_native::{HeadlessPlatform, HostWindow, Platform};
use flui_platform::{
    CloseDecision, DispatchEventResult, SessionEndAnswer, SessionEndPhase, SystemNotice,
    WindowCommand, WindowOptions,
};
use parking_lot::Mutex;

/// How long a case may take before it counts as deadlocked.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Run `case` on a worker thread: its panic is resumed here, and a case that
/// does not finish within [`TIMEOUT`] fails as deadlocked.
fn within(case: &'static str, body: impl FnOnce() + Send + 'static) {
    let (done, finished) = channel();
    let worker = std::thread::spawn(move || {
        body();
        let _ = done.send(());
    });
    match finished.recv_timeout(TIMEOUT) {
        Ok(()) => worker.join().expect("the case finished"),
        Err(RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(worker.join().expect_err("the case panicked"));
        }
        Err(RecvTimeoutError::Timeout) => panic!(
            "{case}: deadlocked — a handler ran, or a replaced one was dropped, under a lock the \
             platform holds"
        ),
    }
}

/// Runs its action when dropped: a capture whose destructor calls back into
/// the platform, as a UI runtime's state does.
struct OnDrop(Option<Box<dyn FnOnce() + Send + Sync>>);

impl OnDrop {
    fn new(action: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self(Some(Box::new(action)))
    }

    fn inert() -> Self {
        Self(None)
    }
}

impl Drop for OnDrop {
    fn drop(&mut self) {
        if let Some(action) = self.0.take() {
            action();
        }
    }
}

/// Records whether the capture it sits in was dropped.
struct Witness(Arc<AtomicBool>);

impl Drop for Witness {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn payload_text(payload: &(dyn Any + Send)) -> Option<&str> {
    flui_foundation::panic::payload_text(payload)
}

// --- application handlers -------------------------------------------------

/// Installs an application handler of one kind, its closure holding `capture`.
type InstallHandler = fn(&dyn Platform, OnDrop);

/// Every application handler a platform takes.
const APPLICATION_HANDLERS: &[(&str, InstallHandler)] = &[
    ("on_quit", |platform, capture| {
        platform.on_quit(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("set_exit_policy_hook", |platform, capture| {
        platform.set_exit_policy_hook(Box::new(move || {
            let _ = &capture;
            false
        }));
    }),
    ("set_wake_deadline_hook", |platform, capture| {
        platform.set_wake_deadline_hook(Box::new(move || {
            let _ = &capture;
            None
        }));
    }),
    ("on_session_end", |platform, capture| {
        platform.on_session_end(Box::new(move |_| {
            let _ = &capture;
            SessionEndAnswer::Proceed
        }));
    }),
    ("on_keyboard_layout_change", |platform, capture| {
        platform.on_keyboard_layout_change(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_reopen", |platform, capture| {
        platform.on_reopen(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_host_notice", |platform, capture| {
        platform.on_host_notice(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
];

/// For each kind of application handler: a handler whose captures call the
/// platform when dropped is replaced, and its destructor reaches the platform.
fn replaced_application_handlers_reach_the_platform(platform: &Arc<dyn Platform>) {
    for &(name, install) in APPLICATION_HANDLERS {
        let reached = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reached);
        let weak: Weak<dyn Platform> = Arc::downgrade(platform);
        install(
            platform.as_ref(),
            OnDrop::new(move || {
                if let Some(platform) = weak.upgrade() {
                    let _ = platform.active_window();
                    observed.store(true, Ordering::SeqCst);
                }
            }),
        );
        install(platform.as_ref(), OnDrop::inert());
        assert!(
            reached.load(Ordering::SeqCst),
            "{name}: the replaced handler's destructor did not reach the platform"
        );
    }
}

// --- window handlers ------------------------------------------------------

/// What a window handler's method does from inside its call: it opens another
/// window, changes its own and quits, each reaching the platform while the
/// method runs.
struct Reentry {
    platform: Weak<dyn Platform>,
    window: RefCell<std::rc::Weak<dyn HostWindow>>,
    options: fn() -> WindowOptions,
    /// The windows opened from inside a method, kept open by the test.
    opened: RefCell<Vec<Rc<dyn HostWindow>>>,
    /// Whether the method re-entered yet: it re-enters once.
    done: Cell<bool>,
}

impl Reentry {
    fn new(platform: &Arc<dyn Platform>, options: fn() -> WindowOptions) -> Rc<Self> {
        Rc::new(Self {
            platform: Arc::downgrade(platform),
            window: RefCell::new(std::rc::Weak::<flui_native::MockWindow>::new()),
            options,
            opened: RefCell::new(Vec::new()),
            done: Cell::new(false),
        })
    }

    /// Re-enter, the first time it is called.
    fn run(&self) {
        if self.done.replace(true) {
            return;
        }
        let platform = self.platform.upgrade().expect("the platform");
        let window = platform
            .open_window((self.options)(), RecordingWindowHandler::new())
            .expect("a window opens from inside a handler method");
        self.opened.borrow_mut().push(window);
        let own = self.window.borrow().upgrade();
        if let Some(own) = own {
            // A closed window answers `Closed`; the call reached it either way.
            let _ = own.admit_and_apply(WindowCommand::SetTitle("re-entered".to_owned()));
        }
        platform.quit();
    }

    /// Make `method` re-enter; for `input`, the first input after the
    /// capability snapshot.
    fn install(self: &Rc<Self>, handler: &RecordingWindowHandler, method: Method) {
        let reentry = Rc::clone(self);
        match method {
            Method::Input => handler.react_to_input(move |input| {
                if !matches!(input, flui_platform::PlatformInput::Capabilities(_)) {
                    reentry.run();
                }
                DispatchEventResult::default()
            }),
            Method::StateChanged => handler.react_to_state_change(move || reentry.run()),
            Method::FrameRequested => handler.react_to_frame(move |_| reentry.run()),
            Method::CloseRequested => handler.react_to_close_request(move || {
                reentry.run();
                CloseDecision::KeepOpen
            }),
            Method::Closed => handler.react_to_close(move || reentry.run()),
            Method::SurfaceChanged => handler.react_to_surface_change(move |_| reentry.run()),
        }
    }
}

/// The methods of a window handler.
#[derive(Debug, Clone, Copy)]
enum Method {
    Input,
    StateChanged,
    FrameRequested,
    CloseRequested,
    Closed,
    SurfaceChanged,
}

/// Makes a window call a handler method; `reentered` answers whether the
/// method ran, for a host that calls it on a later message. `false` when the
/// window never called it.
type Trigger = dyn Fn(&Rc<dyn HostWindow>, &dyn Fn() -> bool) -> bool;

/// Opens a window delivering to a handler whose `method` re-enters, makes the
/// window call that method with `trigger`, and checks every re-entry reached
/// the platform: a window was opened and, where the host reports its quit to
/// `quits`, the platform quit.
fn a_handler_method_reenters(
    platform: &Arc<dyn Platform>,
    quits: Option<&AtomicUsize>,
    options: fn() -> WindowOptions,
    method: Method,
    trigger: &Trigger,
) {
    let reentry = Reentry::new(platform, options);
    let handler = RecordingWindowHandler::new();
    reentry.install(&handler, method);
    let window = platform
        .open_window(options(), handler)
        .expect("a window opens");
    *reentry.window.borrow_mut() = Rc::downgrade(&window);
    let before = quits.map(|quits| quits.load(Ordering::SeqCst));
    let probe = Rc::clone(&reentry);
    assert!(
        trigger(&window, &move || probe.done.get()),
        "{method:?}: the window never called it"
    );
    assert_eq!(
        reentry.opened.borrow().len(),
        1,
        "{method:?}: the window opened from inside the method"
    );
    if let (Some(quits), Some(before)) = (quits, before) {
        assert_eq!(
            quits.load(Ordering::SeqCst),
            before + 1,
            "{method:?}: the quit reached the platform"
        );
    }
    for opened in reentry.opened.take() {
        opened.close_for_cleanup();
    }
    window.close_for_cleanup();
}

/// A window lets go of its handler when it closes; the handler's captures,
/// whose destructor calls the platform, are dropped with none of the
/// platform's locks held.
fn a_handler_dropped_at_close_reaches_the_platform(
    platform: &Arc<dyn Platform>,
    options: fn() -> WindowOptions,
    settle: &dyn Fn(&dyn Fn() -> bool) -> bool,
) {
    let reached = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&reached);
    let weak = Arc::downgrade(platform);
    let calls_back = OnDrop::new(move || {
        if let Some(platform) = weak.upgrade() {
            let _ = platform.active_window();
            observed.store(true, Ordering::SeqCst);
        }
    });
    let handler = RecordingWindowHandler::new();
    handler.react_to_frame(move |_| {
        let _ = &calls_back;
    });
    let window = platform
        .open_window(options(), handler)
        .expect("a window opens");
    window.close_now();
    assert!(
        settle(&|| reached.load(Ordering::SeqCst)),
        "the handler's destructor did not reach the platform"
    );
}

/// Raises something on a window from inside a handler method.
type Raise = dyn Fn(&Rc<dyn HostWindow>);

/// A close request the window system raises synchronously from inside the
/// handler's `input` (a `WM_CLOSE` sent by the input's own handling) is not
/// asked inside `input`: the window stays open until `input` returns, then
/// the handler is asked once, and its `Close` closes the window.
fn a_close_request_raised_inside_input_waits_for_it(
    platform: &Arc<dyn Platform>,
    options: fn() -> WindowOptions,
    send_input: &Raise,
    raise_close: Rc<Raise>,
    settle: &dyn Fn(&dyn Fn() -> bool) -> bool,
) {
    let handler = RecordingWindowHandler::new();
    let own: Rc<RefCell<std::rc::Weak<dyn HostWindow>>> =
        Rc::new(RefCell::new(std::rc::Weak::<flui_native::MockWindow>::new()));
    let in_input = Rc::new(Cell::new(false));
    let asked_inside = Rc::new(Cell::new(false));
    let closed = Rc::new(Cell::new(false));
    let raised = Rc::new(Cell::new(false));
    {
        let (own, in_input, raised) = (Rc::clone(&own), Rc::clone(&in_input), Rc::clone(&raised));
        handler.react_to_input(move |input| {
            if !matches!(input, flui_platform::PlatformInput::Capabilities(_))
                && !raised.replace(true)
                && let Some(window) = own.borrow().upgrade()
            {
                in_input.set(true);
                raise_close(&window);
                in_input.set(false);
            }
            DispatchEventResult::default()
        });
    }
    {
        let (in_input, asked_inside) = (Rc::clone(&in_input), Rc::clone(&asked_inside));
        handler.react_to_close_request(move || {
            asked_inside.set(asked_inside.get() || in_input.get());
            CloseDecision::Close
        });
    }
    {
        let closed = Rc::clone(&closed);
        handler.react_to_close(move || closed.set(true));
    }
    let window = platform
        .open_window(options(), handler.clone())
        .expect("a window opens");
    *own.borrow_mut() = Rc::downgrade(&window);
    send_input(&window);
    assert!(
        settle(&|| closed.get()),
        "the granted request closed the window"
    );
    assert!(raised.get(), "the input never ran");
    assert!(
        !asked_inside.get(),
        "the close request was asked inside `input`"
    );
    assert_eq!(handler.count(&HandlerCall::CloseRequested), 1);
}

// --- headless -------------------------------------------------------------

fn headless() -> Arc<HeadlessPlatform> {
    Arc::new(HeadlessPlatform::new())
}

/// Each method of a window's handler opens a window, changes its own and
/// quits from inside its call, on the headless host, which calls every one.
fn headless_handler_methods_reenter() {
    within("headless window handler", || {
        // A quit is heard once per platform: each method gets its own.
        let platform = || {
            let headless = headless();
            let quits = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&quits);
            headless.on_quit(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }));
            (headless as Arc<dyn Platform>, quits)
        };
        let mock = |window: &Rc<dyn HostWindow>| {
            window
                .as_any()
                .downcast_ref::<flui_native::MockWindow>()
                .expect("a headless window")
                .clone()
        };
        let rows: [(Method, Box<Trigger>); 6] = [
            (
                Method::Input,
                Box::new(move |window, _| {
                    mock(window).inject_event(flui_platform::PlatformInput::Ime(
                        flui_platform::ImeEvent::Enabled,
                    ));
                    true
                }),
            ),
            (
                Method::StateChanged,
                Box::new(move |window, _| {
                    mock(window).simulate_resize(320.0, 200.0);
                    true
                }),
            ),
            (
                Method::FrameRequested,
                Box::new(|window, _| {
                    window.request_redraw();
                    true
                }),
            ),
            (
                Method::CloseRequested,
                Box::new(move |window, _| !mock(window).simulate_close()),
            ),
            (
                Method::Closed,
                Box::new(|window, _| {
                    window.close_now();
                    true
                }),
            ),
            (
                Method::SurfaceChanged,
                Box::new(move |window, _| {
                    mock(window).simulate_surface_change(flui_platform::SurfaceChange::Release);
                    true
                }),
            ),
        ];
        for (method, trigger) in &rows {
            let (platform, quits) = platform();
            a_handler_method_reenters(
                &platform,
                Some(&quits),
                WindowOptions::default,
                *method,
                trigger,
            );
        }
        let (platform, _) = platform();
        a_handler_dropped_at_close_reaches_the_platform(
            &platform,
            WindowOptions::default,
            &|done| done(),
        );
    });
}

/// A close request simulated from inside `input` on the headless window.
fn headless_close_request_inside_input_waits_for_it() {
    within("headless close request inside input", || {
        let platform: Arc<dyn Platform> = headless();
        let mock = |window: &Rc<dyn HostWindow>| {
            window
                .as_any()
                .downcast_ref::<flui_native::MockWindow>()
                .expect("a headless window")
                .clone()
        };
        a_close_request_raised_inside_input_waits_for_it(
            &platform,
            WindowOptions::default,
            &move |window| {
                mock(window).inject_event(flui_platform::PlatformInput::Ime(
                    flui_platform::ImeEvent::Enabled,
                ));
            },
            Rc::new(move |window| {
                assert!(
                    !mock(window).simulate_close(),
                    "a request raised inside `input` keeps the window for now"
                );
            }),
            &|done| done(),
        );
    });
}

/// A session-end hook that installs its successor from inside its call, its
/// captures calling the platform when dropped.
fn headless_session_end_hook_replaces_itself() {
    within("headless session-end hook", || {
        let platform = headless();
        let reached = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reached);
        let weak = Arc::downgrade(&platform);
        let calls_back = OnDrop::new(move || {
            if let Some(platform) = weak.upgrade() {
                let _ = platform.active_window();
                observed.store(true, Ordering::SeqCst);
            }
        });
        let weak = Arc::downgrade(&platform);
        platform.on_session_end(Box::new(move |_| {
            let _ = &calls_back;
            let platform = weak.upgrade().expect("the platform");
            platform.on_session_end(Box::new(|_| SessionEndAnswer::Block));
            platform.quit();
            SessionEndAnswer::Proceed
        }));

        assert_eq!(
            platform.simulate_session_end(SessionEndPhase::Query),
            SessionEndAnswer::Proceed
        );
        assert!(
            reached.load(Ordering::SeqCst),
            "the replaced hook's destructor reached the platform"
        );
        assert_eq!(
            platform.simulate_session_end(SessionEndPhase::Query),
            SessionEndAnswer::Block,
            "the successor answers the next phase"
        );
    });
}

/// A system-notice hook that installs its successor from inside its call, its
/// captures reporting another notice when dropped: that notice is heard after
/// the one being delivered, by the successor.
fn headless_notice_hook_replaces_itself() {
    within("headless system-notice hook", || {
        let platform = headless();
        let reached = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reached);
        let weak = Arc::downgrade(&platform);
        let calls_back = OnDrop::new(move || {
            if let Some(platform) = weak.upgrade() {
                platform.simulate_host_notice(SystemNotice::SessionLocked);
                observed.store(true, Ordering::SeqCst);
            }
        });
        let successor = Arc::new(Mutex::new(Vec::new()));
        let (weak, heard) = (Arc::downgrade(&platform), Arc::clone(&successor));
        platform.on_host_notice(Box::new(move |_| {
            let _ = &calls_back;
            let heard = Arc::clone(&heard);
            weak.upgrade()
                .expect("the platform")
                .on_host_notice(Box::new(move |notice| heard.lock().push(notice)));
        }));

        platform.simulate_host_notice(SystemNotice::Sleep);
        assert!(
            reached.load(Ordering::SeqCst),
            "the replaced hook's destructor reached the platform"
        );
        platform.simulate_host_notice(SystemNotice::Wake);
        assert_eq!(
            *successor.lock(),
            [SystemNotice::SessionLocked, SystemNotice::Wake]
        );
    });
}

fn headless_replaced_application_handlers_reach_the_platform() {
    within("headless application handlers", || {
        let platform: Arc<dyn Platform> = headless();
        replaced_application_handlers_reach_the_platform(&platform);
    });
}

/// A session-end hook that installs its successor and then panics: the
/// caller gets that panic, the hook's captures are retained rather than
/// dropped after the failure, and the successor answers the next phase.
fn headless_failing_hook_keeps_the_first_failure() {
    within("headless failing session-end hook", || {
        let platform = headless();
        let dropped = Arc::new(AtomicBool::new(false));
        let witness = Witness(Arc::clone(&dropped));
        let weak = Arc::downgrade(&platform);
        platform.on_session_end(Box::new(move |_| {
            let _ = &witness;
            weak.upgrade()
                .expect("the platform")
                .on_session_end(Box::new(|_| SessionEndAnswer::Block));
            panic!("first failure");
        }));

        let failure = catch_unwind(AssertUnwindSafe(|| {
            platform.simulate_session_end(SessionEndPhase::Query)
        }))
        .expect_err("the hook's panic reaches the caller");
        assert_eq!(payload_text(failure.as_ref()), Some("first failure"));
        assert!(
            !dropped.load(Ordering::SeqCst),
            "the failed hook's captures are retained, not dropped"
        );
        assert_eq!(
            platform.simulate_session_end(SessionEndPhase::Query),
            SessionEndAnswer::Block
        );
    });
}

/// The exit-policy hook, asked when the last window closes, opens a window and
/// allows the exit: the window it opened keeps the platform running.
fn headless_exit_policy_hook_that_opens_a_window_keeps_the_platform() {
    within("headless exit-policy hook", || {
        let platform = headless();
        let quits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&quits);
        platform.on_quit(Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }));
        let opened = Arc::new(Mutex::new(None));
        let (weak, kept) = (Arc::downgrade(&platform), Arc::clone(&opened));
        let first = AtomicBool::new(true);
        platform.set_exit_policy_hook(Box::new(move || {
            if first.swap(false, Ordering::SeqCst) {
                let window = weak
                    .upgrade()
                    .expect("the platform")
                    .open_window(
                        WindowOptions::default(),
                        flui_testing::window_handler::RecordingWindowHandler::new(),
                    )
                    .expect("a window opens from inside the hook");
                let window = window
                    .as_any()
                    .downcast_ref::<flui_native::MockWindow>()
                    .expect("a headless platform opens mock windows")
                    .clone();
                *kept.lock() = Some(window);
            }
            true
        }));

        let window = platform
            .open_window(
                WindowOptions::default(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("a window");
        window.close_now();

        assert!(opened.lock().is_some(), "the hook ran and opened a window");
        assert_eq!(
            quits.load(Ordering::SeqCst),
            0,
            "the window the hook opened keeps the platform running"
        );
        let window = opened.lock().take().expect("the hook's window");
        window.close_now();
        assert_eq!(
            quits.load(Ordering::SeqCst),
            1,
            "closing it, with no window left, quits"
        );
    });
}

#[test]
fn handlers_run_outside_every_lock_of_the_headless_platform() {
    crate::run_table(
        "handlers_run_outside_every_lock_of_the_headless_platform",
        &[
            (
                "every window handler method opens a window and quits",
                headless_handler_methods_reenter,
            ),
            (
                "a close request raised inside input waits for it",
                headless_close_request_inside_input_waits_for_it,
            ),
            (
                "a session-end hook replaces itself",
                headless_session_end_hook_replaces_itself,
            ),
            (
                "a system-notice hook replaces itself",
                headless_notice_hook_replaces_itself,
            ),
            (
                "replaced application handlers reach the platform",
                headless_replaced_application_handlers_reach_the_platform,
            ),
            (
                "a failing hook keeps the first failure",
                headless_failing_hook_keeps_the_first_failure,
            ),
            (
                "an exit-policy hook that opens a window keeps the platform",
                headless_exit_policy_hook_that_opens_a_window_keeps_the_platform,
            ),
        ],
    );
}

// --- winit ----------------------------------------------------------------

/// The winit platform's registrations, before its loop runs: a replaced
/// handler's destructor reaches the platform.
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "linux")
))]
#[test]
fn replaced_winit_handlers_reach_the_platform() {
    within("winit application handlers", || {
        let platform: Arc<dyn Platform> = Arc::new(flui_native::WinitPlatform::new());
        replaced_application_handlers_reach_the_platform(&platform);
    });
}

/// Where winit reports no lifecycle (Windows, and macOS alike) the host's
/// lifecycle position is absent: each registration is dropped as it is made,
/// captures and all, instead of kept for a call that never comes.
#[cfg(all(feature = "winit-backend", target_os = "windows"))]
#[test]
fn winit_lifecycle_registrations_drop_their_captures_without_a_source() {
    /// Records that it was dropped.
    struct Capture(Arc<AtomicBool>);

    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn capture() -> (Arc<AtomicBool>, Capture) {
        let dropped = Arc::new(AtomicBool::new(false));
        (Arc::clone(&dropped), Capture(dropped))
    }

    let platform = flui_native::WinitPlatform::new();
    let (session_end, held) = capture();
    platform.on_session_end(Box::new(move |_| {
        let _ = &held;
        SessionEndAnswer::Proceed
    }));
    let (notice, held) = capture();
    platform.on_host_notice(Box::new(move |_| {
        let _ = &held;
    }));
    let (reopen, held) = capture();
    platform.on_reopen(Box::new(move || {
        let _ = &held;
    }));
    for (registration, dropped) in [
        ("session end", session_end),
        ("host notice", notice),
        ("reopen", reopen),
    ] {
        assert!(
            dropped.load(Ordering::SeqCst),
            "the {registration} registration was dropped at registration"
        );
    }
}

// --- Win32 ----------------------------------------------------------------

#[cfg(target_os = "windows")]
mod win32 {
    #![expect(
        unsafe_code,
        reason = "pumps the owner thread's own Win32 message queue"
    )]

    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use flui_native::{Platform, WindowsPlatform};
    use flui_platform::{WindowCommand, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostMessageW, SendMessageW,
        TranslateMessage, WM_CLOSE, WM_LBUTTONDOWN, WM_QUIT,
    };

    use super::{
        Method, Trigger, Witness, a_close_request_raised_inside_input_waits_for_it,
        a_handler_dropped_at_close_reaches_the_platform, a_handler_method_reenters,
        replaced_application_handlers_reach_the_platform, within,
    };

    fn platform() -> Arc<WindowsPlatform> {
        Arc::new(WindowsPlatform::new().expect("native Windows platform"))
    }

    fn hidden() -> WindowOptions {
        WindowOptions::default()
            .with_title("handler reentry")
            .with_visible(false)
    }

    /// Dispatch this thread's messages until `done`, for at most two seconds;
    /// a posted quit is consumed, not acted on.
    fn pump_until(mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let mut message = MSG::default();
            // SAFETY: `message` is a stack-local out-parameter; the messages
            // are this thread's own.
            unsafe {
                while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    if message.message == WM_QUIT {
                        continue;
                    }
                    let _ = TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
            }
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    /// Each method a Win32 window calls opens a window, changes its own and
    /// quits from inside its call, inside the window procedure that called
    /// it. A Win32 window never releases its surface, and the quit is a
    /// posted `WM_QUIT` the pump consumes.
    fn handler_methods_reenter() {
        within("Win32 window handler", || {
            // A quit stops the platform taking windows: each method gets its own.
            let platform = || -> Arc<dyn Platform> { platform() };
            let rows: [(Method, Box<Trigger>); 5] = [
                (
                    Method::Input,
                    Box::new(|window, reentered| {
                        let hwnd = crate::win32_handle(window.as_ref());
                        // SAFETY: the window is alive (the trigger runs before
                        // anything closes it) and owned by this thread, which
                        // opened it, so the send runs its window procedure
                        // here; the message carries plain integers, a primary
                        // press at client (10, 10).
                        unsafe {
                            SendMessageW(
                                hwnd,
                                WM_LBUTTONDOWN,
                                Some(WPARAM(0x0001)),
                                Some(LPARAM((0x000a << 16) | 0x000a)),
                            );
                        }
                        pump_until(reentered)
                    }),
                ),
                (
                    Method::StateChanged,
                    Box::new(|window, reentered| {
                        window
                            .admit_and_apply(WindowCommand::SetSize(
                                flui_foundation::geometry::Size::new(320.0, 200.0),
                            ))
                            .expect("a resize");
                        pump_until(reentered)
                    }),
                ),
                (
                    Method::FrameRequested,
                    Box::new(|window, reentered| {
                        window.request_redraw();
                        pump_until(reentered)
                    }),
                ),
                (
                    Method::CloseRequested,
                    Box::new(|window, reentered| {
                        let hwnd = crate::win32_handle(window.as_ref());
                        // SAFETY: the window is alive and owned by this thread,
                        // whose pump delivers the posted message; `WM_CLOSE`
                        // carries no pointers.
                        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                            .expect("a posted close");
                        pump_until(reentered)
                    }),
                ),
                (
                    Method::Closed,
                    Box::new(|window, reentered| {
                        window.close_now();
                        pump_until(reentered)
                    }),
                ),
            ];
            for (method, trigger) in &rows {
                a_handler_method_reenters(&platform(), None, hidden, *method, trigger);
            }
            a_handler_dropped_at_close_reaches_the_platform(&platform(), hidden, &|done| {
                pump_until(done)
            });
        });
    }

    /// A `WM_CLOSE` sent from inside the handler's `input`, inside the window
    /// procedure that delivers the press: the window procedure keeps the
    /// window, the handler is asked after `input` returns, and the close it
    /// grants destroys the window by the posted close.
    fn close_request_inside_input_waits_for_it() {
        within("Win32 close request inside input", || {
            let platform: Arc<dyn Platform> = platform();
            a_close_request_raised_inside_input_waits_for_it(
                &platform,
                hidden,
                &|window| {
                    let hwnd = crate::win32_handle(window.as_ref());
                    // SAFETY: the test opened this window on this thread, which
                    // owns it, and it is alive (nothing closed it yet); the
                    // message carries plain integers, a primary press at
                    // client (10, 10).
                    unsafe {
                        SendMessageW(
                            hwnd,
                            WM_LBUTTONDOWN,
                            Some(WPARAM(0x0001)),
                            Some(LPARAM((0x000a << 16) | 0x000a)),
                        );
                    }
                },
                Rc::new(|window| {
                    let hwnd = crate::win32_handle(window.as_ref());
                    // SAFETY: the window is alive and owned by this thread (the
                    // send runs inside its own window procedure), and
                    // `WM_CLOSE` carries no pointers.
                    unsafe {
                        SendMessageW(hwnd, WM_CLOSE, None, None);
                    }
                    assert!(
                        raw_window_handle::HasWindowHandle::window_handle(
                            &*window.surface_target()
                        )
                        .is_ok(),
                        "the window was destroyed inside `input`"
                    );
                }),
                &|done| pump_until(done),
            );
        });
    }

    fn replaced_application_handlers() {
        within("Win32 application handlers", || {
            let platform: Arc<dyn Platform> = platform();
            replaced_application_handlers_reach_the_platform(&platform);
        });
    }

    /// An exit-policy hook that installs its successor and then panics inside
    /// the window procedure: the panic is contained as a veto, the hook's
    /// captures are retained rather than dropped after the failure, and the
    /// successor decides the next request.
    fn failing_exit_policy_keeps_the_first_failure() {
        within("Win32 failing exit-policy hook", || {
            let platform = platform();
            let ran = Arc::new(AtomicBool::new(false));
            let dropped = Arc::new(AtomicBool::new(false));
            let successor_calls = Arc::new(AtomicUsize::new(0));
            let witness = Witness(Arc::clone(&dropped));
            let (weak, failed, successor) = (
                Arc::downgrade(&platform),
                Arc::clone(&ran),
                Arc::clone(&successor_calls),
            );
            platform.set_exit_policy_hook(Box::new(move || {
                let _ = &witness;
                failed.store(true, Ordering::SeqCst);
                let successor = Arc::clone(&successor);
                weak.upgrade()
                    .expect("the platform")
                    .set_exit_policy_hook(Box::new(move || {
                        successor.fetch_add(1, Ordering::SeqCst);
                        false
                    }));
                panic!("first failure");
            }));

            platform.request_exit_policy_reevaluation();
            assert!(pump_until(|| ran.load(Ordering::SeqCst)), "the hook ran");
            assert!(
                !dropped.load(Ordering::SeqCst),
                "the failed hook's captures are retained, not dropped"
            );
            platform.request_exit_policy_reevaluation();
            assert!(
                pump_until(|| successor_calls.load(Ordering::SeqCst) == 1),
                "the successor decides the next request"
            );
        });
    }

    /// A handler whose captures panic when dropped is still installed when the
    /// loop ends: the teardown retires it with its panic contained, and `run`
    /// returns.
    fn a_handler_dropped_at_teardown_is_contained() {
        within("Win32 teardown", || {
            struct PanicsOnDrop;
            impl Drop for PanicsOnDrop {
                fn drop(&mut self) {
                    panic!("a capture's destructor");
                }
            }
            let platform = WindowsPlatform::new().expect("native Windows platform");
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Box::new(platform).run(Box::new(|owner| {
                    let capture = PanicsOnDrop;
                    owner.shared().on_keyboard_layout_change(Box::new(move || {
                        let _ = &capture;
                    }));
                    owner.proxy().request_quit().expect("the loop takes a quit");
                    Ok(())
                }))
            }));
            assert!(
                matches!(ran, Ok(Ok(()))),
                "a handler's destructor escaped the teardown: {:?}",
                ran.map(|result| result.is_ok())
            );
        });
    }

    #[test]
    fn handlers_run_outside_every_borrow_of_the_win32_platform() {
        crate::run_table(
            "handlers_run_outside_every_borrow_of_the_win32_platform",
            &[
                (
                    "every window handler method opens a window and quits",
                    handler_methods_reenter,
                ),
                (
                    "a close request raised inside input waits for it",
                    close_request_inside_input_waits_for_it,
                ),
                (
                    "replaced application handlers reach the platform",
                    replaced_application_handlers,
                ),
                (
                    "a failing exit-policy hook keeps the first failure",
                    failing_exit_policy_keeps_the_first_failure,
                ),
                (
                    "a handler dropped at teardown is contained",
                    a_handler_dropped_at_teardown_is_contained,
                ),
            ],
        );
    }
}
