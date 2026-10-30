//! The platform calls the application's and a window's handlers with none of
//! its locks or borrows held, on every host: a handler may quit, open a window
//! or report to the platform from inside its call; a window's handler, whose
//! captures call the platform when dropped, is dropped after the platform let
//! go of its state; and a handler's panic stays the first failure, with what
//! was accepted behind it still delivered.
//!
//! A case that regresses deadlocks rather than fails, so each one runs on a
//! worker thread and is reported after [`TIMEOUT`]: the worker stays parked
//! for the rest of the process.

use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak as RcWeak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_native::{HeadlessPlatform, HostWindow, OwnerPlatform, Platform};
use flui_platform::{
    CloseDecision, DispatchEventResult, SessionEndAnswer, SessionEndPhase, SystemNotice,
    WindowCommand, WindowOptions,
};

use crate::app::{AppCall, AppLog, run_keeping_owner};

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
struct OnDrop(Option<Box<dyn FnOnce()>>);

impl OnDrop {
    fn new(action: impl FnOnce() + 'static) -> Self {
        Self(Some(Box::new(action)))
    }
}

impl Drop for OnDrop {
    fn drop(&mut self) {
        if let Some(action) = self.0.take() {
            action();
        }
    }
}

fn payload_text(payload: &(dyn Any + Send)) -> Option<&str> {
    flui_foundation::panic::payload_text(payload)
}

/// What a re-entering handler reaches: a platform before it runs, or the
/// owner capability of one that runs.
trait Reach {
    /// Open a window delivering to `handler`.
    fn open(
        &self,
        options: WindowOptions,
        handler: Rc<RecordingWindowHandler>,
    ) -> Rc<dyn HostWindow>;

    /// Quit the platform.
    fn quit(&self);

    /// A call that takes the platform's state.
    fn touch(&self);
}

impl Reach for Arc<dyn Platform> {
    fn open(
        &self,
        options: WindowOptions,
        handler: Rc<RecordingWindowHandler>,
    ) -> Rc<dyn HostWindow> {
        self.open_window(options, handler).expect("a window opens")
    }

    fn quit(&self) {
        Platform::quit(&**self);
    }

    fn touch(&self) {
        let _ = self.active_window();
    }
}

impl Reach for OwnerPlatform {
    fn open(
        &self,
        options: WindowOptions,
        handler: Rc<RecordingWindowHandler>,
    ) -> Rc<dyn HostWindow> {
        self.open_window(options, handler)
            .expect("a window opens")
            .try_ready()
            .expect("the window opens at once")
    }

    fn quit(&self) {
        OwnerPlatform::quit(self);
    }

    fn touch(&self) {
        let _ = self.active_window();
    }
}

/// The keyboard layout callback, the one application-level registration left:
/// a replaced callback whose captures call the platform when dropped reaches
/// the platform from its destructor.
fn a_replaced_keyboard_layout_callback_reaches_the_platform(platform: &Arc<dyn Platform>) {
    /// Reaches the platform when dropped.
    struct CallsBack(Weak<dyn Platform>, Arc<AtomicBool>);
    impl Drop for CallsBack {
        fn drop(&mut self) {
            if let Some(platform) = self.0.upgrade() {
                let _ = platform.active_window();
                self.1.store(true, Ordering::SeqCst);
            }
        }
    }
    let reached = Arc::new(AtomicBool::new(false));
    let capture = CallsBack(Arc::downgrade(platform), Arc::clone(&reached));
    platform.on_keyboard_layout_change(Box::new(move || {
        let _ = &capture;
    }));
    platform.on_keyboard_layout_change(Box::new(|| {}));
    assert!(
        reached.load(Ordering::SeqCst),
        "the replaced callback's destructor did not reach the platform"
    );
}

// --- window handlers ------------------------------------------------------

/// What a window handler's method does from inside its call: it opens another
/// window, changes its own and quits, each reaching the platform while the
/// method runs.
struct Reentry {
    platform: RcWeak<dyn Reach>,
    window: RefCell<RcWeak<dyn HostWindow>>,
    options: fn() -> WindowOptions,
    /// The windows opened from inside a method, kept open by the test.
    opened: RefCell<Vec<Rc<dyn HostWindow>>>,
    /// Whether the method re-entered yet: it re-enters once.
    done: Cell<bool>,
}

impl Reentry {
    fn new(platform: &Rc<dyn Reach>, options: fn() -> WindowOptions) -> Rc<Self> {
        Rc::new(Self {
            platform: Rc::downgrade(platform),
            window: RefCell::new(RcWeak::<flui_native::MockWindow>::new()),
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
        let window = platform.open((self.options)(), RecordingWindowHandler::new());
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
/// the platform: a window was opened and, where the application's handler
/// hears the quit (`quits`), the platform quit.
fn a_handler_method_reenters(
    platform: &Rc<dyn Reach>,
    quits: Option<&dyn Fn() -> usize>,
    options: fn() -> WindowOptions,
    method: Method,
    trigger: &Trigger,
) {
    let reentry = Reentry::new(platform, options);
    let handler = RecordingWindowHandler::new();
    reentry.install(&handler, method);
    let window = platform.open(options(), handler);
    *reentry.window.borrow_mut() = Rc::downgrade(&window);
    let before = quits.map(|quits| quits());
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
            quits(),
            before + 1,
            "{method:?}: the quit reached the application's handler"
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
    platform: &Rc<dyn Reach>,
    options: fn() -> WindowOptions,
    settle: &dyn Fn(&dyn Fn() -> bool) -> bool,
) {
    let reached = Rc::new(Cell::new(false));
    let observed = Rc::clone(&reached);
    let weak = Rc::downgrade(platform);
    let calls_back = OnDrop::new(move || {
        if let Some(platform) = weak.upgrade() {
            platform.touch();
            observed.set(true);
        }
    });
    let handler = RecordingWindowHandler::new();
    handler.react_to_frame(move |_| {
        let _ = &calls_back;
    });
    let window = platform.open(options(), handler);
    window.close_now();
    assert!(
        settle(&|| reached.get()),
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
    platform: &Rc<dyn Reach>,
    options: fn() -> WindowOptions,
    send_input: &Raise,
    raise_close: Rc<Raise>,
    settle: &dyn Fn(&dyn Fn() -> bool) -> bool,
) {
    let handler = RecordingWindowHandler::new();
    let own: Rc<RefCell<RcWeak<dyn HostWindow>>> =
        Rc::new(RefCell::new(RcWeak::<flui_native::MockWindow>::new()));
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
    let window = platform.open(options(), handler.clone());
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

/// A running headless platform, its handler's log and its owner capability.
fn running_headless() -> (Rc<AppLog>, Rc<OwnerPlatform>) {
    let log = AppLog::new();
    let owner = run_keeping_owner(Box::new(HeadlessPlatform::new()), &log);
    (log, Rc::new(owner))
}

fn mock(window: &Rc<dyn HostWindow>) -> flui_native::MockWindow {
    window
        .as_any()
        .downcast_ref::<flui_native::MockWindow>()
        .expect("a headless window")
        .clone()
}

/// Each method of a window's handler opens a window, changes its own and
/// quits from inside its call, on the headless host, which calls every one.
fn headless_handler_methods_reenter() {
    within("headless window handler", || {
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
            // A quit is heard once per run: each method gets its own.
            let (log, owner) = running_headless();
            let quits = move || log.count(AppCall::Quit);
            a_handler_method_reenters(
                &(owner as Rc<dyn Reach>),
                Some(&quits),
                WindowOptions::default,
                *method,
                trigger,
            );
        }
        let (_log, owner) = running_headless();
        a_handler_dropped_at_close_reaches_the_platform(
            &(owner as Rc<dyn Reach>),
            WindowOptions::default,
            &|done| done(),
        );
    });
}

/// A close request simulated from inside `input` on the headless window.
fn headless_close_request_inside_input_waits_for_it() {
    within("headless close request inside input", || {
        let (_log, owner) = running_headless();
        a_close_request_raised_inside_input_waits_for_it(
            &(owner as Rc<dyn Reach>),
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

/// Makes the platform call one method of the application's handler.
type AppTrigger = fn(&flui_native::HeadlessHostEvents, &OwnerPlatform);

/// Each method of the application's handler the headless host calls opens a
/// window, reports a notice and quits from inside its call: the window opens
/// at once, and the notice and the quit, which are reports, are heard after
/// the method returns.
fn headless_app_handler_methods_reenter() {
    within("headless application handler", || {
        let rows: [(AppCall, AppTrigger); 5] = [
            (AppCall::Quit, |_, owner| owner.quit()),
            (AppCall::Reopen, |events, _| events.reopen()),
            (AppCall::SessionEnd(SessionEndPhase::Query), |events, _| {
                assert_eq!(
                    events.session_end(SessionEndPhase::Query),
                    SessionEndAnswer::Proceed
                );
            }),
            (AppCall::Notice(SystemNotice::Sleep), |events, _| {
                events.notice(SystemNotice::Sleep);
            }),
            (AppCall::ExitPolicy, |_, owner| {
                owner
                    .open_window(WindowOptions::default(), RecordingWindowHandler::new())
                    .expect("a window")
                    .try_ready()
                    .expect("at once")
                    .close_now();
            }),
        ];
        for (method, trigger) in rows {
            let platform = HeadlessPlatform::new();
            let events = platform.host_events();
            let log = AppLog::new();
            let owner = Rc::new(run_keeping_owner(Box::new(platform), &log));
            let opened = Rc::new(RefCell::new(Vec::new()));
            let (inner_events, kept, done) = (
                events.clone(),
                Rc::clone(&opened),
                Rc::new(Cell::new(false)),
            );
            log.react(move |call, owner| {
                if call != method || done.replace(true) {
                    return;
                }
                // A platform that is quitting opens no window: the quit's own
                // method finds it so.
                let window = owner
                    .open_window(WindowOptions::default(), RecordingWindowHandler::new())
                    .map(|open| open.try_ready().expect("at once"));
                kept.borrow_mut().push(window.is_ok());
                inner_events.notice(SystemNotice::SessionLocked);
                owner.quit();
            });
            trigger(&events, &owner);

            assert_eq!(
                *opened.borrow(),
                [method != AppCall::Quit],
                "{method:?}: the window opened from inside the method, unless it quits"
            );
            let calls = log.calls();
            let at = calls
                .iter()
                .position(|call| *call == method)
                .expect("the method ran");
            let after = &calls[at + 1..];
            assert!(
                after.contains(&AppCall::Notice(SystemNotice::SessionLocked)),
                "{method:?}: the notice reported inside it was heard after it: {calls:?}"
            );
            assert_eq!(log.count(AppCall::Quit), 1, "{method:?}: one quit");
        }
    });
}

/// A method that reports, then panics: its caller gets the panic, the report
/// it made is still heard, and the next call is answered.
fn headless_failing_method_keeps_the_first_failure() {
    within("headless failing application handler", || {
        let platform = HeadlessPlatform::new();
        let events = platform.host_events();
        let log = AppLog::new();
        log.answer_session_end(SessionEndAnswer::Block);
        let _owner = run_keeping_owner(Box::new(platform), &log);
        let (inner, first) = (events.clone(), Cell::new(true));
        log.react(move |call, _| {
            if call == AppCall::SessionEnd(SessionEndPhase::Query) && first.replace(false) {
                inner.notice(SystemNotice::Sleep);
                panic!("first failure");
            }
        });

        let failure = catch_unwind(AssertUnwindSafe(|| {
            events.session_end(SessionEndPhase::Query)
        }))
        .expect_err("the method's panic reaches the caller");
        assert_eq!(payload_text(failure.as_ref()), Some("first failure"));
        assert_eq!(
            events.session_end(SessionEndPhase::Query),
            SessionEndAnswer::Block,
            "the handler answers the next phase"
        );
        assert_eq!(
            log.count(AppCall::Notice(SystemNotice::Sleep)),
            1,
            "the report made before the panic was heard"
        );
    });
}

/// The exit policy, asked when the last window closes, opens a window and
/// allows the exit: the window it opened keeps the platform running.
fn headless_exit_policy_that_opens_a_window_keeps_the_platform() {
    within("headless exit policy", || {
        let (log, owner) = running_headless();
        log.answer_exit(true);
        let opened = Rc::new(RefCell::new(None));
        let (kept, first) = (Rc::clone(&opened), Cell::new(true));
        log.react(move |call, owner| {
            if call == AppCall::ExitPolicy && first.replace(false) {
                let window = owner
                    .open_window(WindowOptions::default(), RecordingWindowHandler::new())
                    .expect("a window opens from inside the exit policy")
                    .try_ready()
                    .expect("at once");
                *kept.borrow_mut() = Some(window);
            }
        });

        owner
            .open_window(WindowOptions::default(), RecordingWindowHandler::new())
            .expect("a window")
            .try_ready()
            .expect("at once")
            .close_now();

        assert!(opened.borrow().is_some(), "the handler opened a window");
        assert_eq!(
            log.count(AppCall::Quit),
            0,
            "the window the handler opened keeps the platform running"
        );
        let window = opened.borrow_mut().take().expect("the handler's window");
        window.close_now();
        assert_eq!(
            log.count(AppCall::Quit),
            1,
            "closing it, with no window left, quits"
        );
    });
}

/// A platform run from inside another's handler binds a handler of its own:
/// each platform's reports reach its own handler, once.
fn headless_a_run_inside_a_handler_binds_its_own() {
    within("headless nested run", || {
        let outer_platform = HeadlessPlatform::new();
        let outer_events = outer_platform.host_events();
        let inner_platform = HeadlessPlatform::new();
        let inner_events = inner_platform.host_events();
        let (outer, inner) = (AppLog::new(), AppLog::new());
        // Each platform lives as long as the owner capability its bootstrap
        // kept.
        let owners = Rc::new(RefCell::new(Vec::new()));
        let kept = Rc::clone(&owners);
        inner.bootstrap(move |owner| {
            kept.borrow_mut().push(owner);
            Ok(())
        });
        let (nested, kept) = (Rc::clone(&inner), Rc::clone(&owners));
        let inner_platform = RefCell::new(Some(inner_platform));
        outer.bootstrap(move |owner| {
            kept.borrow_mut().push(owner);
            let platform = inner_platform.take().expect("one nested run");
            Box::new(platform)
                .run(nested as Rc<dyn flui_native::AppHandler>)
                .map_err(Into::into)
        });
        Box::new(outer_platform)
            .run(Rc::clone(&outer) as Rc<dyn flui_native::AppHandler>)
            .expect("the outer run");
        assert_eq!(owners.borrow().len(), 2, "both platforms ran");

        outer_events.notice(SystemNotice::Sleep);
        inner_events.notice(SystemNotice::Wake);
        assert_eq!(
            outer.calls(),
            [AppCall::Ready, AppCall::Notice(SystemNotice::Sleep)]
        );
        assert_eq!(
            inner.calls(),
            [AppCall::Ready, AppCall::Notice(SystemNotice::Wake)]
        );
    });
}

fn headless_replaced_keyboard_layout_callback_reaches_the_platform() {
    within("headless keyboard layout callback", || {
        let platform: Arc<dyn Platform> = Arc::new(HeadlessPlatform::new());
        a_replaced_keyboard_layout_callback_reaches_the_platform(&platform);
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
                "every application handler method opens a window, reports and quits",
                headless_app_handler_methods_reenter,
            ),
            (
                "a failing method keeps the first failure",
                headless_failing_method_keeps_the_first_failure,
            ),
            (
                "an exit policy that opens a window keeps the platform",
                headless_exit_policy_that_opens_a_window_keeps_the_platform,
            ),
            (
                "a run inside a handler binds its own",
                headless_a_run_inside_a_handler_binds_its_own,
            ),
            (
                "a replaced keyboard layout callback reaches the platform",
                headless_replaced_keyboard_layout_callback_reaches_the_platform,
            ),
        ],
    );
}

// --- winit ----------------------------------------------------------------

/// The winit platform's keyboard layout callback, before its loop runs: a
/// replaced callback's destructor reaches the platform.
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "linux")
))]
#[test]
fn replaced_winit_handlers_reach_the_platform() {
    within("winit keyboard layout callback", || {
        let platform: Arc<dyn Platform> = Arc::new(flui_native::WinitPlatform::new());
        a_replaced_keyboard_layout_callback_reaches_the_platform(&platform);
    });
}

// --- Win32 ----------------------------------------------------------------

#[cfg(target_os = "windows")]
mod win32 {
    #![expect(
        unsafe_code,
        reason = "pumps the owner thread's own Win32 message queue"
    )]

    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use flui_native::{AppHandler, Platform, WindowsPlatform};
    use flui_platform::{WindowCommand, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostMessageW, SendMessageW,
        TranslateMessage, WM_CLOSE, WM_LBUTTONDOWN, WM_QUIT,
    };

    use super::{
        Method, Reach, Trigger, a_close_request_raised_inside_input_waits_for_it,
        a_handler_dropped_at_close_reaches_the_platform, a_handler_method_reenters,
        a_replaced_keyboard_layout_callback_reaches_the_platform, within,
    };
    use crate::app::{AppCall, AppLog};

    fn platform() -> Rc<dyn Reach> {
        let platform: Arc<dyn Platform> =
            Arc::new(WindowsPlatform::try_new().expect("native Windows platform"));
        Rc::new(platform)
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
            // A quit stops the platform taking windows: each method gets its own.
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
            a_close_request_raised_inside_input_waits_for_it(
                &platform(),
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

    fn replaced_keyboard_layout_callback() {
        within("Win32 keyboard layout callback", || {
            let platform: Arc<dyn Platform> =
                Arc::new(WindowsPlatform::try_new().expect("native Windows platform"));
            a_replaced_keyboard_layout_callback_reaches_the_platform(&platform);
        });
    }

    /// An exit policy that asks to be asked again and then panics inside the
    /// window procedure: the panic is contained as a veto, the loop carries
    /// on, the handler decides the next request, and hears the quit once.
    fn failing_exit_policy_keeps_the_loop() {
        within("Win32 failing exit policy", || {
            let platform = WindowsPlatform::try_new().expect("native Windows platform");
            let log = AppLog::new();
            log.bootstrap(|owner| {
                owner.shared().request_exit_policy_reevaluation();
                Ok(())
            });
            let first = Cell::new(true);
            log.react(move |call, owner| {
                if call != AppCall::ExitPolicy {
                    return;
                }
                if first.replace(false) {
                    owner.shared().request_exit_policy_reevaluation();
                    panic!("first failure");
                }
                owner.proxy().request_quit().expect("the loop takes a quit");
            });
            Box::new(platform)
                .run(Rc::clone(&log) as Rc<dyn AppHandler>)
                .expect("the loop carries on past the contained panic");
            assert_eq!(log.count(AppCall::ExitPolicy), 2, "asked again");
            assert_eq!(log.count(AppCall::Quit), 1, "the quit, once");
        });
    }

    /// A callback whose captures panic when dropped is still installed when
    /// the loop ends: the teardown retires it with its panic contained, and
    /// `run` returns.
    fn a_handler_dropped_at_teardown_is_contained() {
        within("Win32 teardown", || {
            struct PanicsOnDrop;
            impl Drop for PanicsOnDrop {
                fn drop(&mut self) {
                    panic!("a capture's destructor");
                }
            }
            let platform = WindowsPlatform::try_new().expect("native Windows platform");
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Box::new(platform).run(crate::app::on_ready(|owner| {
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
                    "a replaced keyboard layout callback reaches the platform",
                    replaced_keyboard_layout_callback,
                ),
                (
                    "a failing exit policy keeps the loop",
                    failing_exit_policy_keeps_the_loop,
                ),
                (
                    "a handler dropped at teardown is contained",
                    a_handler_dropped_at_teardown_is_contained,
                ),
            ],
        );
    }
}
