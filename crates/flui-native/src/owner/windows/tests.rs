//! The window class's dispatch rule, through a hook set of the tests' own
//! over the quiet host's sources.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, Weak};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, IsWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_GETTEXTLENGTH, WM_QUERYENDSESSION,
};
use windows::core::w;

use super::broadcast::Subscriber;
use super::control::{OwnerControl, OwnerMessages, OwnerPost, OwnerWindow};
use super::testing::{ClassHooks, NoDisplays, NoPreferences, NoWindows, QuietHost};
use super::{
    CreatedWindow, WindowGone, WindowHooks, WindowIdentity, WindowMessages, WindowsLoop,
    dispatch_window_message, window_created,
};
use crate::error::PlatformError;

thread_local! {
    /// What the hooks heard, in order.
    static HEARD: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

fn hear(what: &'static str) {
    HEARD.with(|heard| heard.borrow_mut().push(what));
}

/// The first hook answers it; the second would answer it too.
const BOTH_ANSWER: u32 = WM_APP + 0x51;
/// Only the second hook answers it.
const SECOND_ANSWERS: u32 = WM_APP + 0x52;

struct First;

impl WindowMessages for First {
    fn created(&self, _window: &CreatedWindow) {
        hear("first created");
    }

    fn answers(&self, msg: u32, _wparam: WPARAM) -> bool {
        msg == BOTH_ANSWER
    }

    unsafe fn message(
        &self,
        _hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        hear("first");
        (msg == BOTH_ANSWER).then_some(LRESULT(7))
    }
}

struct Second;

impl WindowMessages for Second {
    fn created(&self, _window: &CreatedWindow) {
        hear("second created");
    }

    fn answers(&self, msg: u32, _wparam: WPARAM) -> bool {
        matches!(msg, BOTH_ANSWER | SECOND_ANSWERS)
    }

    unsafe fn message(
        &self,
        _hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        hear("second");
        match msg {
            BOTH_ANSWER => Some(LRESULT(9)),
            SECOND_ANSWERS => Some(LRESULT(11)),
            _ => None,
        }
    }
}

/// The tests' two hooks, in order.
struct FirstThenSecond;

impl ClassHooks for FirstThenSecond {
    fn hooks() -> WindowHooks {
        test_hooks()
    }
}

/// A host of the tests' own: the quiet host with the two hooks.
type TestHost = QuietHost<FirstThenSecond>;

/// The test host's hooks, in order.
fn test_hooks() -> WindowHooks {
    Rc::new([Rc::new(First) as Rc<dyn WindowMessages>, Rc::new(Second)])
}

/// The class's procedure offers a message to the hooks in order: the first
/// that answers ends the dispatch, one that passes hands it on, and a message
/// no hook answers gets `DefWindowProcW`'s answer. A created window is
/// announced to every hook.
#[test]
fn a_window_message_reaches_the_hooks_in_order() {
    // SAFETY: a message-only window of a system class, created and destroyed
    // on this thread; the class name and title are static literals.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("flui"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
    }
    .expect("a message-only window");
    let dispatch = |msg| {
        HEARD.with(|heard| heard.borrow_mut().clear());
        // SAFETY: `hwnd` is this thread's live window, and the messages carry
        // no pointers; the default procedure answers `WM_GETTEXTLENGTH` from
        // the window's own title.
        let answer =
            unsafe { dispatch_window_message::<TestHost>(hwnd, msg, WPARAM(0), LPARAM(0)) };
        (answer.0, HEARD.with(RefCell::take))
    };

    assert_eq!(
        dispatch(BOTH_ANSWER),
        (7, vec!["first"]),
        "the first hook that answers ends the dispatch"
    );
    assert_eq!(
        dispatch(SECOND_ANSWERS),
        (11, vec!["first", "second"]),
        "a hook that passes hands the message on"
    );
    assert_eq!(
        dispatch(WM_GETTEXTLENGTH),
        ("flui".len() as isize, vec!["first", "second"]),
        "a message no hook answers gets the default procedure's answer"
    );

    assert_eq!(
        window_created(&test_hooks(), &created(hwnd), || true),
        Ok(()),
        "a window that lives through its hooks is announced to all of them"
    );
    assert_eq!(
        HEARD.with(RefCell::take),
        ["first created", "second created"],
        "every hook learns of a created window"
    );

    // SAFETY: the window this test created on this thread.
    unsafe { DestroyWindow(hwnd) }.expect("the test window is destroyed");
}

/// `hwnd` as the hooks are told of it, under an identity of the test's own.
fn created(hwnd: HWND) -> CreatedWindow {
    CreatedWindow {
        hwnd,
        identity: WindowIdentity::mint_from(&std::sync::atomic::AtomicU64::new(1)),
        client_size: (0, 0),
        scale: super::WindowScale::new(96),
        delivery: Arc::new(crate::owner::window_delivery::WindowDelivery::new(
            flui_testing::window_handler::RecordingWindowHandler::new(),
            flui_platform::input_capabilities::InputCapabilities::UNAVAILABLE,
        )),
        app_hooks: Rc::new(RefCell::new(super::AppHooks::new(Arc::new(
            crate::owner::app_delivery::AppDelivery::new(),
        )))),
        preferences: Rc::new(NoPreferences),
        hooks: test_hooks(),
        apartment: None,
        drag: Arc::new(super::testing::NoDrag),
    }
}

/// A hook whose `created` destroys the window, as a message one dispatches
/// can.
struct Destroys;

impl WindowMessages for Destroys {
    fn created(&self, window: &CreatedWindow) {
        hear("destroys created");
        // SAFETY: the test's own window, on the thread that created it.
        unsafe { DestroyWindow(window.hwnd) }.expect("the hook destroys the window");
    }
}

/// A window that a hook destroyed is announced to no later hook: none of them
/// runs on the dead handle, and the announcement answers `WindowGone`, which
/// fails the window's open.
#[test]
fn a_window_a_hook_destroyed_is_announced_to_no_later_hook() {
    let hooks: WindowHooks = Rc::new([
        Rc::new(First) as Rc<dyn WindowMessages>,
        Rc::new(Destroys),
        Rc::new(Second),
    ]);
    // SAFETY: a message-only window of a system class, created on this
    // thread; the class name and title are static literals.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("flui"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
    }
    .expect("a message-only window");
    HEARD.with(|heard| heard.borrow_mut().clear());

    // SAFETY: `IsWindow` takes any handle and dereferences nothing.
    let alive = || unsafe { IsWindow(Some(hwnd)) }.as_bool();
    assert_eq!(
        window_created(&hooks, &created(hwnd), alive),
        Err(WindowGone),
        "the announcement reports the window gone"
    );
    assert_eq!(
        HEARD.with(RefCell::take),
        ["first created", "destroys created"],
        "no hook after the one that destroyed the window learns of it"
    );
}

/// A broadcast subscriber that records what it hears and answers the messages
/// it is given.
struct Listener {
    name: &'static str,
    answers: &'static [(u32, isize)],
    after_panic: Option<(u32, isize)>,
    panics_on: Option<u32>,
}

impl Subscriber for Listener {
    fn answer_after_panic(&self, message: u32) -> Option<LRESULT> {
        self.after_panic
            .filter(|(answered, _)| *answered == message)
            .map(|(_, answer)| LRESULT(answer))
    }

    fn message(
        &self,
        _hwnd: HWND,
        message: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        hear(self.name);
        assert_ne!(self.panics_on, Some(message), "a subscriber that panics");
        self.answers
            .iter()
            .find(|(answered, _)| *answered == message)
            .map(|(_, answer)| LRESULT(*answer))
    }
}

/// The receiver tells its subscribers a message in the order its host named
/// them: the first that answers ends the dispatch, one that passes hands it
/// on. A subscriber that panics leaves the answer a subscriber gives for that
/// case, 0 when none gives one.
#[test]
fn broadcast_subscribers_hear_a_message_in_order() {
    let receiver = super::broadcast::Receiver::new([
        Rc::new(Listener {
            name: "first",
            answers: &[(BOTH_ANSWER, 7)],
            after_panic: None,
            panics_on: Some(WM_APP + 0x53),
        }) as Rc<dyn Subscriber>,
        Rc::new(Listener {
            name: "second",
            answers: &[(BOTH_ANSWER, 9), (SECOND_ANSWERS, 11)],
            after_panic: Some((WM_APP + 0x53, 1)),
            panics_on: None,
        }),
    ])
    .expect("receiver");
    let send = |message| {
        HEARD.with(|heard| heard.borrow_mut().clear());
        let answer = receiver.send_with_for_test(message, 0, 0);
        (answer, HEARD.with(RefCell::take))
    };

    assert_eq!(
        send(BOTH_ANSWER),
        (7, vec!["first"]),
        "the first subscriber that answers ends the dispatch"
    );
    assert_eq!(
        send(SECOND_ANSWERS),
        (11, vec!["first", "second"]),
        "a subscriber that passes hands the message on"
    );
    assert_eq!(
        send(WM_APP + 0x53),
        (1, vec!["first"]),
        "a panic answers what a subscriber gives for it"
    );
    assert_eq!(
        send(WM_GETTEXTLENGTH),
        (
            "FLUI system broadcasts".len() as isize,
            vec!["first", "second"]
        ),
        "a message nobody answers gets the default procedure's answer"
    );
}

/// A `tracing` subscriber that keeps the `entry` and `panic` fields of every
/// event that has a panic.
struct Logged(Arc<Mutex<Vec<(String, String)>>>);

impl tracing::Subscriber for Logged {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        #[derive(Default)]
        struct Fields {
            entry: String,
            panic: Option<String>,
        }
        impl tracing::field::Visit for Fields {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                match field.name() {
                    "entry" => value.clone_into(&mut self.entry),
                    "panic" => self.panic = Some(value.to_owned()),
                    _ => {}
                }
            }

            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        if let Some(panic) = fields.panic {
            self.0
                .lock()
                .expect("the log is not poisoned")
                .push((fields.entry, panic));
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A subscriber's panic is logged with its text under the receiver's name,
/// and a session-end query no subscriber answered for that case is granted:
/// refusing it would hold the user's logoff up. Red-check: contain the
/// subscribers in one generic containment again, or answer 0 after a panic.
#[test]
fn a_failing_subscriber_is_logged_and_a_session_end_still_granted() {
    let receiver = super::broadcast::Receiver::new([Rc::new(Listener {
        name: "failing",
        answers: &[],
        after_panic: None,
        panics_on: Some(WM_QUERYENDSESSION),
    }) as Rc<dyn Subscriber>])
    .expect("receiver");
    let log = Arc::new(Mutex::new(Vec::new()));
    let answer = tracing::subscriber::with_default(Logged(Arc::clone(&log)), || {
        receiver.send_with_for_test(WM_QUERYENDSESSION, 0, 0)
    });
    assert_eq!(answer, 1, "the session end is granted");
    let log = log.lock().expect("the log is not poisoned");
    assert!(
        log.iter().any(|(entry, panic)| {
            entry == "the broadcast receiver's procedure"
                && panic.contains("a subscriber that panics")
        }),
        "the subscriber's failure is logged with its text: {log:?}"
    );
}

/// An owner-window hook that hears the window go, and may panic as it does.
struct Ending {
    name: &'static str,
    panics: bool,
}

impl OwnerMessages for Ending {
    fn attach(&self, _owner: OwnerPost) {}

    fn message(&self, _owner: &OwnerWindow, _message: u32) -> Option<LRESULT> {
        None
    }

    fn destroyed(&self) {
        hear(self.name);
        assert!(!self.panics, "a hook that panics as the owner window goes");
    }
}

/// Every owner-window hook hears the window go, the one after a hook that
/// panics too.
#[test]
fn every_owner_hook_hears_the_window_go_after_one_panics() {
    let control = OwnerControl::new::<TestHost>(
        WindowIdentity::mint_from(WindowIdentity::source()),
        Weak::<NoWindows>::new(),
        vec![
            Arc::new(Ending {
                name: "first",
                panics: true,
            }),
            Arc::new(Ending {
                name: "second",
                panics: false,
            }),
        ],
        Arc::new(NoDisplays),
        Arc::new(crate::owner::notices::testing::NoNotices),
        Arc::new(crate::owner::app_delivery::AppDelivery::new()),
    )
    .expect("the owner control");
    HEARD.with(|heard| heard.borrow_mut().clear());
    drop(control);
    assert_eq!(HEARD.with(RefCell::take), ["first", "second"]);
}

/// Windows holds one window class of a name per process: once a loop of one
/// host registered it, a loop for another host is refused, not handed a class
/// that dispatches to the first host's hooks. The hosts are told apart by type:
/// the quiet host and the quiet host with the tests' hooks are two. The class
/// stays registered for the quiet host in this process, which nextest runs
/// this test alone in, as it runs every test.
#[test]
fn a_second_host_is_refused_the_window_class() {
    let _first = WindowsLoop::new::<QuietHost>(Arc::new(NoWindows))
        .expect("the first host registers the class");
    match WindowsLoop::new::<TestHost>(Arc::new(NoWindows)) {
        Err(PlatformError::Init { message }) => assert!(
            message.contains("another host"),
            "the refusal names the class's other host: {message}"
        ),
        Err(error) => panic!("refused for another reason: {error}"),
        Ok(_) => panic!("a second host was given the first host's window class"),
    }
}

/// A window opens at the DPI the system reports for it, and at the DPI of the
/// scale it was placed at when the system reports none: never at 100% by
/// default while the window itself sits at another scale. A hook reads the
/// window's one value, so it never lags behind a new DPI.
#[test]
fn a_window_opens_at_one_dpi_its_hooks_share() {
    for (reported, placed_at, dpi) in [
        (144, 1.0, 144),
        (0, 1.5, 144),
        (0, 1.25, 120),
        (0, 1.0, 96),
        (0, f64::NAN, 96),
        (0, 0.0, 96),
    ] {
        assert_eq!(
            super::window_table::opening_dpi(reported, placed_at),
            dpi,
            "reported {reported}, placed at {placed_at}"
        );
    }
    let scale = super::WindowScale::at_open(0, 1.5);
    let read_by_a_hook = scale.clone();
    assert_eq!(scale.rescale(192), 1.5);
    assert_eq!(
        read_by_a_hook.scale(),
        2.0,
        "a hook reads the window's new scale"
    );
}
