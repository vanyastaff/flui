//! The window class's dispatch rule, through a hook set of the tests' own.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, IsWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_GETTEXTLENGTH,
};
use windows::core::w;

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, NullDataTransferSource,
    RepresentationIndex, TransferLimits, TransferRequest,
};
use flui_platform::drag::{DragError, OsDragRequest, OsDragStart, PlatformDragSource};

use super::broadcast::Subscriber;
use super::control::{
    OwnerControl, OwnerLifecycle, OwnerMessages, OwnerObjects, OwnerPost, OwnerPreferences,
    OwnerSeeds, OwnerWindow,
};
use super::{
    ClassWindows, CreatedWindow, HostDisplays, HostDrag, WindowGone, WindowHooks, WindowIdentity,
    WindowMessages, WindowRef, WindowsHost, WindowsLoop, dispatch_window_message, window_created,
};
use crate::display::windows::WindowsDisplays;
use crate::error::PlatformError;
use crate::owner::handlers::OpenWindows;

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

/// A host of the tests' own: two hooks, the Windows host's sources, no
/// broadcast subscriber and a drag position that takes no drags.
pub(super) struct TestHost;

impl WindowsHost for TestHost {
    const WINDOW_HOOKS: WindowHooks = &[&First, &Second];

    fn preferences(owner: &OwnerSeeds<'_>) -> Result<Rc<dyn OwnerPreferences>, PlatformError> {
        <crate::hosts::windows::Windows as WindowsHost>::preferences(owner)
    }

    fn notices() -> std::sync::Arc<dyn crate::owner::notices::NoticeChannel> {
        <crate::hosts::windows::Windows as WindowsHost>::notices()
    }

    fn lifecycle(owner: &OwnerSeeds<'_>) -> Rc<dyn OwnerLifecycle> {
        <crate::hosts::windows::Windows as WindowsHost>::lifecycle(owner)
    }

    fn broadcast_subscribers(_: &OwnerObjects<'_>) -> Vec<Rc<dyn Subscriber>> {
        Vec::new()
    }

    fn displays() -> Arc<dyn HostDisplays> {
        Arc::new(WindowsDisplays::new())
    }

    fn drag() -> Arc<dyn HostDrag> {
        Arc::new(NoDrag)
    }
}

/// A transport that takes and starts no drags, and leaves the owner window's
/// messages to the rest.
struct NoDrag;

impl OwnerMessages for NoDrag {
    fn attach(&self, _owner: OwnerPost) {}

    fn message(&self, _owner: &OwnerWindow, _message: u32) -> Option<LRESULT> {
        None
    }

    fn destroyed(&self) {}
}

impl DataTransferSource for NoDrag {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        NullDataTransferSource.request(id, representation, limits)
    }

    fn update_drop_feedback(&self, _id: DataTransferId, _feedback: DropFeedback) {}

    fn conclude_drop(&self, _id: DataTransferId) {}
}

impl PlatformDragSource for NoDrag {
    fn start_timing(&self) -> OsDragStart {
        OsDragStart::OnLeavingWindow
    }

    fn start(&self, _request: OsDragRequest) -> Result<(), DragError> {
        Err(DragError::Unavailable)
    }
}

impl HostDrag for NoDrag {
    fn drag_source(self: Arc<Self>, _window: WindowRef) -> Arc<dyn PlatformDragSource> {
        self
    }

    fn register_target(self: Arc<Self>, _window: WindowRef) {}
}

/// No window, for a loop the tests build only to see it refused.
#[derive(Debug)]
pub(super) struct NoWindows;

impl OpenWindows for NoWindows {
    fn is_empty(&self) -> bool {
        true
    }
}

impl ClassWindows for NoWindows {
    fn redraw_for_deadline(&self) {}

    fn paint_invalid(&self) {}
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
        window_created(TestHost::WINDOW_HOOKS, &created(hwnd), || true),
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
        apartment: None,
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
    const HOOKS: WindowHooks = &[&First, &Destroys, &Second];
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
        window_created(HOOKS, &created(hwnd), alive),
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
        Arc::new(WindowsDisplays::new()),
        TestHost::notices(),
    )
    .expect("the owner control");
    HEARD.with(|heard| heard.borrow_mut().clear());
    drop(control);
    assert_eq!(HEARD.with(RefCell::take), ["first", "second"]);
}

/// Windows holds one window class of a name per process: a loop for a second
/// host is refused, not handed a class that dispatches to the first host's
/// hooks.
#[test]
fn a_second_host_is_refused_the_window_class() {
    let _platform = crate::WindowsPlatform::new().expect("native platform");
    match WindowsLoop::new::<TestHost>(Arc::new(NoWindows)) {
        Err(PlatformError::Init { message }) => assert!(
            message.contains("another host"),
            "the refusal names the class's other host: {message}"
        ),
        Err(error) => panic!("refused for another reason: {error}"),
        Ok(_) => panic!("a second host was given the first host's window class"),
    }
}
