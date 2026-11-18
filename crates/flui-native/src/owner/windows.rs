//! The Windows host's owner thread: the Win32 message loop and its wake
//! deadlines, the owner's message-only window ([`control`]), the thread's COM
//! apartment ([`com_apartment`]), FLUI's window class with its raw window
//! procedure, and the hidden top-level window that hears the system's
//! broadcasts ([`broadcast`]).
//!
//! The window class, the broadcast receiver and the owner's message-only window
//! serve several features, so they name none of them: the procedure offers each
//! message to the [`WindowMessages`] hooks the host registers, the receiver
//! tells its [`broadcast::Subscriber`]s what the system broadcast, and the owner
//! window hands the messages a feature posts or sends it to that feature's
//! [`control::OwnerMessages`] hook (ADR-0151 §10). The drag-and-drop transport,
//! which the loop holds, the host makes ([`WindowsHost::drag`]); the loop names
//! it by what it asks of it ([`HostDrag`]).

// This module (and its submodules) is one of the workspace's sanctioned
// `unsafe` FFI islands — direct Win32 calls have no safe wrapper. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary; every `unsafe` block still carries its own `// SAFETY:` comment
// stating the invariant that makes it sound.
#![expect(unsafe_code)]

pub(crate) mod broadcast;
pub(crate) mod com_apartment;
pub(crate) mod control;
#[cfg(test)]
mod modal_frame_tests;
pub(crate) mod modal_loop;
#[cfg(feature = "single-instance")]
pub(crate) mod single_instance;
/// A build without the `single-instance` feature serves no pipe: the first
/// instance the host names has no value, and its owner window gets no hook.
#[cfg(not(feature = "single-instance"))]
pub(crate) mod single_instance {
    use std::sync::Arc;

    use super::control::OwnerMessages;

    /// No first instance: this build has none.
    #[derive(Debug)]
    pub(crate) enum FirstInstance {}

    impl FirstInstance {
        /// Unreachable: no value exists.
        pub(crate) fn into_hook(self) -> Arc<dyn OwnerMessages> {
            match self {}
        }
    }
}
#[cfg(test)]
mod tests;
// The quiet host the loop's tests run over, and another host's tests name.
#[cfg(test)]
pub(crate) mod testing;
mod window_table;

use std::{
    any::TypeId,
    cell::RefCell,
    rc::Rc,
    sync::{Arc, OnceLock},
};

use flui_platform::CursorError;
use flui_platform::data_transfer::DragTransport;
use flui_platform::drag::PlatformDragSource;
use flui_platform::text_store::TextStoreHost;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM},
        Graphics::Gdi::HBRUSH,
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::{
                CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                HCURSOR, HICON, HWND_MESSAGE, IDC_ARROW, LoadCursorW, MSG, MWMO_INPUTAVAILABLE,
                MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, PostQuitMessage, QS_ALLINPUT,
                RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_QUIT,
                WNDCLASSW,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::owner::app_delivery::AppDelivery;
use crate::owner::notices::{NoticeChannel, StaleOnWake};
use crate::owner::panic_boundary::{self, AfterPanic};
pub(crate) use crate::owner::wake_deadline::FrameCount;
use crate::owner::wake_deadline::{TurnDeadline, WakeDeadline};
use crate::{
    error::PlatformError, host, owner::OpenWindows, owner::OwnerHooks, owner::OwnerPlatform,
    owner::window_delivery::WindowDelivery, platform::AppHandler, platform::Platform,
};
use broadcast::Subscriber;
use com_apartment::ComApartment;
use flui_platform::{DisplayId, DisplaySnapshot};
pub(crate) use modal_loop::ModalLoops;
pub(crate) use window_table::{WindowDpi, WindowScale, WindowTable};

/// The name of FLUI's window class, which every window the host opens has.
pub(crate) const WINDOW_CLASS_NAME: PCWSTR = w!("FluiWindowClass");

/// The signed x coordinate a message packs in the low word of its `LPARAM`
/// (`GET_X_LPARAM`): a mouse message's, a move's.
#[inline]
pub(crate) fn get_x_lparam(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFF) as i16 as i32
}

/// The signed y coordinate a message packs in the high word of its `LPARAM`
/// (`GET_Y_LPARAM`).
#[inline]
pub(crate) fn get_y_lparam(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xFFFF) as i16 as i32
}

/// Windows' registration of FLUI's window class: the host whose hooks the
/// class's procedure dispatches to, or why the registration failed. Windows
/// holds the class, and the procedure it was registered with, once per
/// process; a second host is refused rather than silently given the first
/// one's hooks.
static WINDOW_CLASS: OnceLock<Result<TypeId, String>> = OnceLock::new();

/// The hooks of FLUI's window class, in the order a message reaches them.
pub(crate) type WindowHooks = Rc<[Rc<dyn WindowMessages>]>;

/// FLUI's window class on one owner thread, as the host's table makes it
/// ([`WindowsHost::window_class`]): the hooks its procedure offers each
/// message of a window to, and what those hooks tell the window's other
/// features of its input and its text input. A hook keeps what it needs of a
/// window as its own state, keyed by the window's identity, from the window's
/// announcement ([`WindowMessages::created`]) to its destruction.
#[derive(Clone)]
pub(crate) struct WindowClass {
    /// The hooks, in the order a message reaches them.
    pub(crate) hooks: WindowHooks,
    /// The input those hooks keep, as the window, drag and drop and haptics
    /// read it.
    pub(crate) input: Rc<dyn WindowInputs>,
    /// The text input those hooks keep, as the window hands it out.
    pub(crate) text: Rc<dyn WindowTexts>,
    /// The window's own hook, as the input reads whose caption buttons a
    /// window's are.
    pub(crate) chrome: Rc<dyn WindowChrome>,
    /// The modal loops running on the thread, which the class's procedure
    /// tells after each message ([`ModalLoops::ran`]); the host's table starts
    /// it empty.
    pub(crate) modal_loops: Rc<ModalLoops>,
}

thread_local! {
    /// FLUI's window class on this thread, by the host whose table made it:
    /// what the class's procedure, which Win32 calls with a window's handle and
    /// no pointer of FLUI's, reads to find the hooks of the window's owner
    /// (ADR-0097's Win32 trampoline). A host's class is made the first time an
    /// owner of that host opens on the thread and lives as long as the thread;
    /// its hooks keep a window's state only while the window exists.
    static CLASS_ON_THREAD: RefCell<Vec<(TypeId, WindowClass)>> = const { RefCell::new(Vec::new()) };
}

/// FLUI's window class on this thread for host `H`, made on first use; `None`
/// while the thread's storage is torn down, when no message is answered.
pub(crate) fn window_class<H: WindowsHost>() -> Option<WindowClass> {
    let host = TypeId::of::<H>();
    let kept = CLASS_ON_THREAD
        .try_with(|classes| {
            classes
                .borrow()
                .iter()
                .find(|(made_for, _)| *made_for == host)
                .map(|(_, class)| class.clone())
        })
        .ok()?;
    if let Some(class) = kept {
        return Some(class);
    }
    // Made with no borrow held: the table's constructors are its own code.
    let class = H::window_class();
    CLASS_ON_THREAD
        .try_with(|classes| classes.borrow_mut().push((host, class.clone())))
        .ok()?;
    Some(class)
}

/// What `part` reads of FLUI's window class on this thread for the host the
/// process registered the class for, read inside the class list's borrow so
/// only that part is cloned; `None` on a thread no owner of that host opened
/// on.
fn registered_class<T>(part: impl FnOnce(&WindowClass) -> T) -> Option<T> {
    let host = *WINDOW_CLASS.get()?.as_ref().ok()?;
    CLASS_ON_THREAD
        .try_with(|classes| {
            classes
                .borrow()
                .iter()
                .find(|(made_for, _)| *made_for == host)
                .map(|(_, class)| part(class))
        })
        .ok()
        .flatten()
}

/// The modal loops running on this thread, as FLUI's window class on it keeps
/// them for the host the process registered the class for; `None` on a thread
/// no owner of that host opened on.
pub(crate) fn thread_modal_loops() -> Option<Rc<ModalLoops>> {
    registered_class(|class| Rc::clone(&class.modal_loops))
}

/// What the input of FLUI's window class on this thread tells the other
/// features of a window, for the host the process registered the class for;
/// `None` on a thread no owner of that host opened on.
pub(crate) fn window_inputs() -> Option<Rc<dyn WindowInputs>> {
    registered_class(|class| Rc::clone(&class.input))
}

/// What the text input of FLUI's window class on this thread holds of a
/// window, for the host the process registered the class for; `None` on a
/// thread no owner of that host opened on.
pub(crate) fn window_texts() -> Option<Rc<dyn WindowTexts>> {
    registered_class(|class| Rc::clone(&class.text))
}

/// What the window's own hook of FLUI's window class on this thread tells the
/// class's other hooks of a window's frame, for the host the process
/// registered the class for; `None` on a thread no owner of that host opened
/// on.
pub(crate) fn window_chrome() -> Option<Rc<dyn WindowChrome>> {
    registered_class(|class| Rc::clone(&class.chrome))
}

/// What the window tells the class's other hooks of its frame, read-only.
/// Answers `None` for a window it does not hold: one already closed, or a
/// reference whose handle now names another window.
pub(crate) trait WindowChrome {
    /// Whether the chrome regions the application hands `window` answer the
    /// system's hit test (ADR-0286 §7): the system draws no title bar and the
    /// window is not fullscreen. Its caption buttons are then the
    /// application's; otherwise a caption button is the system's own, and so
    /// are the messages over it.
    fn regions_answer_the_hit_test(&self, window: WindowRef) -> Option<bool>;
}

/// What the text input of FLUI's window class holds of a window, as the
/// window hands it out. Answers `None` for a window it does not hold: one
/// whose text input did not start, one already closed, or a reference whose
/// handle now names another window.
pub(crate) trait WindowTexts {
    /// The text-store host the window's input methods read the focused
    /// field's store through.
    fn store_host(&self, window: WindowRef) -> Option<Rc<dyn TextStoreHost>>;
}

/// What the input of FLUI's window class tells the window's other features
/// (the window itself, drag and drop, haptics) of a window, read-only. Each
/// answers `None` for a window the input does not hold: one it was never told
/// of, one already destroyed, or a reference whose handle now names another
/// window.
pub(crate) trait WindowInputs {
    /// Whether the mouse is over the window's client area.
    fn hovered(&self, window: WindowRef) -> Option<bool>;

    /// The kind of the contact whose press began the sequence `press` in the
    /// window, while that sequence is in progress; `None` once it ended.
    fn press_kind(
        &self,
        window: WindowRef,
        press: flui_platform::pointer::PressId,
    ) -> Option<flui_platform::pointer::PointerKind>;

    /// The OS's id of a pen contact in range of the window (touching or
    /// hovering): `device`'s, or with `None` the pen read most recently.
    fn pen_in_range(
        &self,
        window: WindowRef,
        device: Option<flui_platform::pointer::DeviceId>,
    ) -> Option<u64>;
}

/// What a Windows host's table names for its owner thread (ADR-0151 §10): the
/// hooks of FLUI's window class and the subscribers of the broadcast receiver,
/// each in the order a message reaches them, and the displays, the preference
/// source, the notice channel, the lifecycle and the drag-and-drop transport
/// the loop holds. The owner dispatches through them and names no feature.
pub(crate) trait WindowsHost: 'static {
    /// The arguments the application was launched with, without its own
    /// name: what a registered scheme or file type hands it (`"%1"`,
    /// [`launch_arguments`](crate::owner::launch_arguments)).
    fn launch_arguments() -> Vec<std::ffi::OsString>;

    /// FLUI's window class on an owner thread: its hooks, whose procedure is
    /// `window_proc::<Self>`, and their input. Made once per thread an owner
    /// of this host opens on ([`window_class`]).
    fn window_class() -> WindowClass;

    /// The system preference source the owner keeps for the table's
    /// preference position.
    ///
    /// # Errors
    /// [`PlatformError::Preferences`] when the system's settings cannot be
    /// observed.
    fn preferences(
        owner: &control::OwnerSeeds<'_>,
    ) -> Result<Rc<dyn control::OwnerPreferences>, PlatformError>;

    /// The channel the owner's system notices take to the application.
    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel>;

    /// The lifecycle the owner keeps for the table's lifecycle position.
    fn lifecycle(owner: &control::OwnerSeeds<'_>) -> Rc<dyn control::OwnerLifecycle>;

    /// The broadcast receiver's subscribers, made from the owner's objects.
    fn broadcast_subscribers(owner: &control::OwnerObjects<'_>) -> Vec<Rc<dyn Subscriber>>;

    /// The displays, made once per loop: the loop holds them for the table's
    /// displays position and hands them to every window it opens.
    fn displays() -> Arc<dyn HostDisplays>;

    /// The drag-and-drop transport, made once per loop on the owner thread.
    /// The loop holds it and answers the table's drag position from it, the
    /// owner window hands it the transport's own messages, and every window
    /// the loop opens takes and starts drags through it.
    fn drag() -> Arc<dyn HostDrag>;
}

/// The host's monitors as the loop and the windows it opens reach them, which
/// the table makes ([`WindowsHost::displays`]). A wake marks them stale.
pub(crate) trait HostDisplays: StaleOnWake {
    /// The snapshot: the last sample, or a fresh one when none was taken or
    /// something invalidated it.
    ///
    /// # Errors
    /// [`PlatformError::Displays`] when the snapshot revision space is used up.
    fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

    /// Record that the monitors may have changed: the next
    /// [`snapshot`](Self::snapshot) samples again. Any thread, at any point.
    fn invalidate(&self);

    /// The last published snapshot; never samples. What a window reads its
    /// position against while it reports its state: the displays an
    /// application read, so a position it was given converts back exactly.
    fn published(&self) -> DisplaySnapshot;

    /// The display `hwnd` is on, from the last sample; never samples.
    fn display_of_window(&self, hwnd: HWND) -> Option<DisplayId>;

    /// The handle of the monitor nearest `hwnd`, `None` for none: what a window
    /// compares to learn that a move took it to another monitor.
    fn nearest_monitor(&self, hwnd: HWND) -> Option<std::num::NonZeroIsize>;

    /// How many times the monitors were read.
    #[cfg(test)]
    fn sample_count(&self) -> usize;
}

/// One window as an object that outlives its borrow of the window names it:
/// the handle and the identity of the context created with it. The OS
/// recycles handles, so a handle alone can name a later window; the window
/// feature's `with_context` reaches only the context this window was created
/// with. Plain values, so an `Arc`-shared object can hold one on any thread,
/// and the owner's, so a hook that is handed a window names no feature. The
/// window-level positions of the Windows table are made for one
/// ([`host::Window::Ref`]), so it is public where it
/// is defined, and nameable only inside the crate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowRef {
    hwnd: isize,
    identity: WindowIdentity,
}

impl WindowRef {
    pub(crate) fn new(hwnd: HWND, identity: WindowIdentity) -> Self {
        Self {
            hwnd: hwnd.0 as isize,
            identity,
        }
    }

    pub(crate) fn hwnd(self) -> HWND {
        HWND(self.hwnd as *mut core::ffi::c_void)
    }

    /// The identity of the context the window was created with.
    pub(crate) fn identity(self) -> WindowIdentity {
        self.identity
    }
}

/// Whether `hwnd` is a live window of FLUI's class, the only class whose
/// `GWLP_USERDATA` slot follows FLUI's protocol. Guards
/// against handle recycling: a dead handle value can be reissued for an
/// unrelated window.
pub(crate) fn is_flui_window(hwnd: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut name = [0_u16; 256];
    // SAFETY: `GetClassNameW` writes at most `name.len()` UTF-16 units into the
    // live stack buffer and returns the count copied without the terminator
    // (0 for a dead handle); `WINDOW_CLASS_NAME` is a NUL-terminated static
    // literal, whose length `as_wide` measures without the terminator too.
    let (copied, expected) =
        unsafe { (GetClassNameW(hwnd, &mut name), WINDOW_CLASS_NAME.as_wide()) };
    crate::owner::hwnd_affinity::class_name_matches(&name, copied, expected)
}

/// The host's drag-and-drop transport as the loop, its owner window and the
/// windows it opens reach it, which the table makes ([`WindowsHost::drag`]):
/// the platform's one data-transfer source, and its part of the owner window's
/// procedure.
pub(crate) trait HostDrag: control::OwnerMessages + DragTransport {
    /// The drag source `window`, created on this thread, hands out.
    fn drag_source(self: Arc<Self>, window: WindowRef) -> Arc<dyn PlatformDragSource>;

    /// Make `window`, which FLUI's class just created on this thread, a drop
    /// target, reporting at the window's scale as it changes. The caller keeps
    /// the registration for the window and revokes it as the window is
    /// destroyed; `None` when registration failed (or off the owner thread),
    /// and the window takes no drags.
    fn register_target(self: Arc<Self>, window: &CreatedWindow) -> Option<Box<dyn DropTarget>>;
}

/// One window's registration as an OLE drop target, which the window's
/// class keeps until the window is destroyed.
pub(crate) trait DropTarget {
    /// The window is destroyed, on its thread, while its callbacks still
    /// exist: a drag over it leaves it, and OLE no longer offers it one.
    fn revoke(&self);

    /// The target as OLE holds it.
    #[cfg(test)]
    fn interface(&self) -> windows::Win32::System::Ole::IDropTarget;

    /// The scale the target reports positions at.
    #[cfg(test)]
    fn scale(&self) -> flui_foundation::geometry::DevicePixelRatio;
}

/// The drag position: the transport the host's [`WindowsHost::drag`] made,
/// which the loop holds.
impl host::Drag<WindowsLoop> for WindowsLoop {
    fn data_transfer(owner: &WindowsLoop) -> Arc<dyn DragTransport> {
        Arc::clone(&owner.drag) as Arc<dyn DragTransport>
    }
}

/// The windows of FLUI's class, which the window feature opens and keeps
/// (ADR-0151 §10).
///
/// The loop holds them without naming them: the host builds the window
/// feature's object and hands it to [`WindowsLoop::try_new`]. The loop asks it
/// whether a window is left (the exit policy), and delivers to it what the main
/// loop and the modal loops pace: a wake deadline's frames and the paints a
/// modal drag-and-drop loop would otherwise hold back.
pub(crate) trait ClassWindows: OpenWindows + std::fmt::Debug {
    /// Asks every window for the frame a delivered wake deadline is due. A
    /// window's frame request runs user code, which may open or close
    /// windows; nothing of the set is held while it runs.
    fn redraw_for_deadline(&self);

    /// Paints every window with an invalid region now, as the main loop would
    /// while a modal loop stands in for it.
    fn paint_invalid(&self);

    /// Whether any window is shown: a launch another process forwarded that
    /// opens nothing asks an application that shows none to reopen.
    #[cfg(feature = "single-instance")]
    fn shows_any(&self) -> bool;

    /// Bring forward the shown window highest in the order of the windows on
    /// the screen, the one the user worked with last, restoring it when it is
    /// minimized: a launch another process forwarded with the right to the
    /// foreground. With no window shown, nothing comes forward: the window
    /// the application shows for the reopen it was asked comes forward with
    /// that right.
    #[cfg(feature = "single-instance")]
    fn activate_forwarded(&self);
}

/// One feature's part of the procedure of FLUI's window class.
///
/// The class and its raw procedure are the owner's: the procedure holds the
/// panic boundary and offers each message to the hooks in the order the host
/// names them ([`WindowClass::hooks`]); the first that answers ends the
/// message's dispatch, and a message no hook answers goes to `DefWindowProcW`.
/// A window of the class, once created, is announced to every hook
/// ([`Self::created`]).
///
/// Each hook lists the messages it answers ([`Self::answers`]) and answers no
/// other, so a host whose hooks list disjoint sets gets the same answer to a
/// message in any order. A hook may also hear a message it does not answer
/// and pass it on; the order then decides only that it hears the message
/// before the hook that answers it.
///
/// A feature that answers a message whose handling is a step other features
/// follow tells every hook of it ([`Self::step`]), and the window's own
/// procedure tells them that the window is destroyed ([`Self::destroyed`]). A
/// hook keeps what it needs of a window in itself, in a [`WindowTable`] keyed
/// by the window's identity: made when the window is announced, dropped when
/// it is destroyed (and at `WM_NCDESTROY`, the window's last message, for one
/// whose destruction was not announced), so no hook reaches another feature's
/// state and a message for a window it does not hold finds none. What every
/// hook reads of a window, its scale, the window holds once
/// ([`CreatedWindow::scale`]).
pub(crate) trait WindowMessages {
    /// `window` was created and its context installed, on its owner thread.
    fn created(&self, _window: &CreatedWindow) {}

    /// `window` took `step`, on its owner thread; told to every hook in order
    /// at the point of the handling where the step is complete
    /// ([`window_step`]).
    fn step(&self, _window: WindowRef, _step: WindowStep) {}

    /// `window` is destroyed (`WM_DESTROY`): its callbacks are still
    /// registered and its handle still valid, and what a hook ends for it
    /// reaches them. Told to every hook in order ([`window_destroyed`]).
    fn destroyed(&self, _window: WindowRef) {}

    /// Whether this hook answers `msg`, with `wparam` telling apart the
    /// messages that share one number (the timer a `WM_TIMER` names, the
    /// command a `WM_SYSCOMMAND` carries). [`Self::message`] answers only what
    /// this lists; the default lists nothing, for a hook that only hears
    /// [`Self::created`].
    fn answers(&self, _msg: u32, _wparam: WPARAM) -> bool {
        false
    }

    /// Answers `msg` for `hwnd`, or passes it on with `None`.
    ///
    /// # Safety
    ///
    /// The arguments are the ones Win32 delivered to the class's procedure for
    /// this message, which it calls on the thread that owns `hwnd`.
    unsafe fn message(
        &self,
        _hwnd: HWND,
        _msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        None
    }
}

/// What the loop shares on the owner thread with every
/// window it opens: the delivery to the application's handler, and the
/// keyboard layout callback of the host's input position
/// (`host::Input::on_keyboard_layout_change`).
pub(crate) struct AppHooks {
    /// The loop's delivery to the application's handler.
    pub(crate) app: Arc<AppDelivery>,
    /// The owner a due turn deadline gives its turn to.
    pub(crate) signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
    /// Called when the keyboard layout changes.
    pub(crate) keyboard_layout_changed: Rc<crate::owner::layout_callback::LayoutCallback>,
}

impl AppHooks {
    /// The hooks over `app` and the owner behind `signal`, with no keyboard
    /// layout callback.
    pub(crate) fn new(
        app: Arc<AppDelivery>,
        signal: std::sync::Weak<crate::owner::signal::OwnerSignal>,
    ) -> Self {
        Self {
            app,
            signal,
            keyboard_layout_changed: Rc::default(),
        }
    }
}

/// A window of FLUI's class as the hooks see it once it is created.
pub(crate) struct CreatedWindow {
    /// The window.
    pub(crate) hwnd: HWND,
    /// The identity of the context the window was created with: a hook names
    /// the window by it, never by the handle alone.
    pub(crate) identity: WindowIdentity,
    /// Its client size in device pixels.
    pub(crate) client_size: (i32, i32),
    /// Its DPI, which its procedure changes as the window moves to another
    /// (and then tells the step, [`WindowStep::Rescaled`]): a hook reads the
    /// window's scale here and keeps no copy of it.
    pub(crate) scale: WindowScale,
    /// Its callbacks, which a feature dispatches what it hears for the window
    /// to.
    pub(crate) delivery: Arc<WindowDelivery>,
    /// What the application gave the loop, shared with every window of it.
    pub(crate) app_hooks: Rc<RefCell<AppHooks>>,
    /// The system preference source the loop keeps, which measures the
    /// system's mouse geometry at the window's scale.
    pub(crate) preferences: Rc<dyn control::OwnerPreferences>,
    /// The hooks of its class, which a feature tells a step of its own
    /// ([`window_step`]).
    pub(crate) hooks: WindowHooks,
    /// The owner thread's COM apartment. A feature that keeps a COM object for
    /// the window holds its own `Rc` of it, so COM stays entered until the
    /// object is released, however long the window outlives the loop.
    pub(crate) apartment: Rc<ComApartment>,
    /// The loop's drag-and-drop transport, which makes the window a drop
    /// target.
    pub(crate) drag: Arc<dyn HostDrag>,
}

impl CreatedWindow {
    /// The window as an object that outlives this announcement names it.
    pub(crate) fn window(&self) -> WindowRef {
        WindowRef::new(self.hwnd, self.identity)
    }
}

/// The scale a window at `dpi` draws at: its DPI over the 96 of 100%.
pub(crate) fn dpi_scale(dpi: u32) -> f64 {
    f64::from(dpi) / 96.0
}

/// A step of a window of FLUI's class that one feature handles and others
/// follow, told to every hook ([`WindowMessages::step`]) where the step is
/// complete: in the window's own procedure for its size, focus, paint, scale,
/// settings and close, in the input's for a key the application let through,
/// in the lifecycle's for the session's lock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum WindowStep {
    /// Its client area is now `(width, height)` device pixels; it is not
    /// minimized.
    Resized((i32, i32)),
    /// It is minimized, and published so: what is held in it sees no release.
    Minimized,
    /// It lost the keyboard focus, and published so.
    FocusLost,
    /// It is about to paint a frame.
    Painting,
    /// The system's modal size-and-move loop starts for it.
    SizeMoveStarted,
    /// That loop ended.
    SizeMoveEnded,
    /// It is now at another DPI, which its [`WindowScale`] holds.
    Rescaled,
    /// The rectangle its new DPI asks for is applied too: what it shows sits
    /// where the new scale puts it on the screen.
    ScaleApplied,
    /// A system setting changed.
    SettingChanged,
    /// The user's session locked (`true`) or unlocked: a locked session
    /// hides every window, whatever its surface.
    SessionLocked(bool),
    /// It holds the pointer in place (`true`), registered for the mouse's raw
    /// input, or let it go: its input reads `WM_INPUT` as the locked
    /// pointer's motion while it holds it, and delivers no mouse moves.
    PointerLocked(bool),
    /// It is being destroyed and its close callbacks ran, so the presentation
    /// that showed it is closed; its handle is still valid.
    Closed,
}

/// The window a hook was told of is gone: a message the hook dispatched
/// destroyed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowGone;

/// Announces `window`, which FLUI's class just created, to `hooks` in order:
/// the host's, which the owner's state hands every window it opens. After each
/// hook, `alive` says whether the window is still the one created; once it is
/// not, no later hook hears of it, and the window is [`WindowGone`].
pub(crate) fn window_created(
    hooks: &[Rc<dyn WindowMessages>],
    window: &CreatedWindow,
    alive: impl Fn() -> bool,
) -> Result<(), WindowGone> {
    for hook in hooks {
        hook.created(window);
        if !alive() {
            return Err(WindowGone);
        }
    }
    Ok(())
}

/// Tells `hooks`, in order, that `window` took `step`. A hook's handling may
/// run user code that destroys the window: a later hook then holds no state
/// for it and does nothing.
pub(crate) fn window_step(hooks: &[Rc<dyn WindowMessages>], window: WindowRef, step: WindowStep) {
    for hook in hooks {
        hook.step(window, step);
    }
}

/// Tells `hooks`, in order, that `window` is destroyed.
pub(crate) fn window_destroyed(hooks: &[Rc<dyn WindowMessages>], window: WindowRef) {
    for hook in hooks {
        hook.destroyed(window);
    }
}

/// Offers one message of a window of FLUI's class to the hooks of `H`'s
/// class on this thread, in order; a message no hook answers goes to
/// `DefWindowProcW`.
///
/// # Safety
///
/// As [`WindowMessages::message`].
unsafe fn dispatch_window_message<H: WindowsHost>(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // A clone of the class: a hook's message may open or close windows, and
    // nothing of the thread's class stays borrowed while it runs.
    let class = window_class::<H>();
    let answered = class
        .iter()
        .flat_map(|class| class.hooks.iter())
        .find_map(|hook| {
            // SAFETY: the arguments as Win32 delivered them, per this function's
            // `# Safety` contract.
            let answer = unsafe { hook.message(hwnd, msg, wparam, lparam) };
            debug_assert!(
                answer.is_none() || hook.answers(msg, wparam),
                "BUG: a window hook answered message {msg:#x}, which its `answers` does not list"
            );
            answer
        });
    // SAFETY: forwards the message verbatim, as Win32 delivered it.
    let answer = answered.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) });
    // The message may have run the platform's code, which a modal loop
    // running on the thread asks the deadlines again after.
    if let Some(class) = &class {
        class.modal_loops.after_window_message(msg, wparam);
    }
    answer
}

/// Load a cursor by style
///
/// # Safety
///
/// `style` must be a value `LoadCursorW` accepts as its resource-name
/// argument: either one of the predefined `IDC_*` constants (an integer
/// resource ordinal packed into a pointer-sized value via
/// `MAKEINTRESOURCEW`, which must never be dereferenced as an actual
/// pointer) or a genuine NUL-terminated UTF-16 string pointer that stays
/// valid for the duration of this call. Every call site in this crate
/// passes a predefined `IDC_*` constant.
unsafe fn load_cursor_style(style: PCWSTR) -> Result<HCURSOR, CursorError> {
    // SAFETY: per the `# Safety` contract above; `None` for `hinstance` is
    // required and documented for loading a predefined `IDC_*` cursor.
    unsafe {
        LoadCursorW(None, style)
            .map_err(|e| CursorError::failed(format!("Failed to load cursor: {e}")))
    }
}

/// Identity of one native window's context, minted once per window the window
/// feature opens and held by both the context and every wrapper clone. Unlike
/// the context's address it is never reused, so a wrapper can tell its own
/// window from one the OS built behind a recycled handle. The owner's
/// message-only window has one too, drawn from the same source. Identities are
/// minted in increasing order, so they order windows by when they opened.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct WindowIdentity(std::num::NonZeroU64);

impl WindowIdentity {
    /// The process-wide identity source.
    pub(crate) fn source() -> &'static std::sync::atomic::AtomicU64 {
        // A monotonic id source, not shared state: nothing reads it back.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        &NEXT
    }

    /// The admission boundary `WindowsWindow::try_new` runs: reserve an
    /// identity from `source`, then run `acquire` for the native window it
    /// will own. Exhaustion refuses before `acquire` runs, so a refusal
    /// leaves nothing to release.
    pub(crate) fn admit<R>(
        source: &std::sync::atomic::AtomicU64,
        acquire: impl FnOnce() -> R,
    ) -> (Self, R) {
        let identity = Self::mint_from(source);
        (identity, acquire())
    }

    fn mint_from(next: &std::sync::atomic::AtomicU64) -> Self {
        use std::sync::atomic::Ordering;

        // Mint the final nonzero identity once, then leave the source at zero.
        // Refusal must not advance it: callers may catch a failed open and retry.
        let raw = next
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |raw| {
                (raw != 0).then(|| raw.checked_add(1).unwrap_or(0))
            })
            .expect("BUG: the window identity counter is exhausted");
        Self(std::num::NonZeroU64::new(raw).expect("BUG: an admitted window identity is nonzero"))
    }

    /// The identity as a posted message's two parameters, its low and high 32
    /// bits, whole on a 32-bit target too.
    pub(crate) fn halves(self) -> (usize, isize) {
        let raw = self.0.get();
        ((raw & 0xFFFF_FFFF) as usize, (raw >> 32) as isize)
    }
}

#[cfg(test)]
impl WindowsLoop {
    pub(crate) fn window_identity_exhaustion_permanently_refuses_retries() {
        use std::{
            cell::Cell,
            sync::atomic::{AtomicU64, Ordering},
        };

        // Exercise the actual allocator with local sources; never exhaust the
        // process-wide source used by other native window tests.
        for seed in [0, u64::MAX - 1] {
            let next = AtomicU64::new(seed);
            let acquisitions = Cell::new(0);
            // The admission `WindowsWindow::try_new` runs, with an acquisition
            // that records native creation instead of performing it.
            let admit =
                || WindowIdentity::admit(&next, || acquisitions.set(acquisitions.get() + 1)).0;
            if seed != 0 {
                let penultimate = admit();
                let final_identity = admit();
                assert_eq!(penultimate.0.get(), u64::MAX - 1);
                assert_eq!(final_identity.0.get(), u64::MAX);
                assert_ne!(penultimate, final_identity);
            }
            for attempt in 0..3 {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(admit)).is_err(),
                    "seed {seed}: exhausted allocation succeeded on retry {attempt}"
                );
            }
            assert_eq!(
                next.load(Ordering::Relaxed),
                0,
                "seed {seed}: refusal advanced the exhausted source"
            );
            assert_eq!(
                acquisitions.get(),
                if seed == 0 { 0 } else { 2 },
                "seed {seed}: exhausted admission acquired native resources"
            );
        }
    }
}

#[cfg(test)]
impl WindowsLoop {
    #[expect(unsafe_code, reason = "COM apartment and message-only window queries")]
    pub(crate) fn platform_identity_exhaustion_acquires_nothing() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use windows::Win32::{
            System::Com::{APTTYPE, APTTYPEQUALIFIER, CoGetApartmentType},
            UI::WindowsAndMessaging::FindWindowExW,
        };

        // This thread's COM apartment, or the query's refusal when none.
        let apartment = || {
            let (mut kind, mut qualifier) = (APTTYPE::default(), APTTYPEQUALIFIER::default());
            // SAFETY: both out-pointers are live, writable locals.
            unsafe { CoGetApartmentType(&raw mut kind, &raw mut qualifier) }
                .map(|()| (kind, qualifier))
                .map_err(|error| error.code())
        };
        // The platform message-only windows that exist in this process.
        let message_windows = || {
            let mut count = 0;
            let mut after = None;
            // SAFETY: enumerates message-only windows by a static class name;
            // every handle is only passed back as the next search start.
            while let Ok(hwnd) = unsafe {
                FindWindowExW(Some(HWND_MESSAGE), after, WINDOW_CLASS_NAME, PCWSTR::null())
            } {
                count += 1;
                after = Some(hwnd);
            }
            count
        };

        let (apartment_before, windows_before) = (apartment(), message_windows());
        let source = AtomicU64::new(0);
        for attempt in 0..3 {
            let refused = std::panic::catch_unwind(|| {
                Self::admit::<testing::QuietHost>(
                    Arc::new(testing::NoWindows),
                    &source,
                    <testing::QuietHost>::drag,
                    None,
                )
            });
            assert!(
                refused.is_err(),
                "exhausted platform admission succeeded on retry {attempt}"
            );
        }
        assert_eq!(
            source.load(Ordering::Relaxed),
            0,
            "refusal advanced the exhausted source"
        );
        assert_eq!(
            apartment(),
            apartment_before,
            "refused admission initialized COM"
        );
        assert_eq!(
            message_windows(),
            windows_before,
            "refused admission created a message window"
        );
    }
}

/// The Windows host's loop: the owner thread's message-only window, the
/// windows it tracks and the state every position of its table reads.
/// [`WindowsPlatform`](crate::WindowsPlatform) is its platform.
pub struct WindowsLoop {
    pub(crate) owner_control: control::OwnerControl,
    /// What the loop delivers to the application's handler, which `run`
    /// binds; the owner context and every window share it.
    pub(crate) app: Arc<AppDelivery>,
    /// The drag-and-drop transport, as the table made it: the platform's one
    /// data-transfer source, which every window it opens shares.
    pub(crate) drag: Arc<dyn HostDrag>,
    /// Message-only window for platform messages
    message_window: HWND,

    /// The windows of FLUI's class: the window feature's, which opens and
    /// keeps them.
    pub(crate) windows: Arc<dyn ClassWindows>,

    // The platform-level handlers (callbacks from platform to framework)
    // are not a field: they live in the owner control context, reachable
    // only on the owner thread through `owner_control.shares`.
    /// The monitors this platform last sampled and the registry of their ids,
    /// as the table made them.
    pub(crate) displays: Arc<dyn HostDisplays>,

    /// The owner thread's COM apartment. Declared last, so it is released
    /// after everything above; each window's text services and touchpad
    /// viewport hold their own `Rc`, so COM outlives the platform while any of
    /// them is alive.
    pub(crate) apartment: Rc<ComApartment>,
}

// Owner-local (ADR-0327 §5): the loop, its window set, its message-only
// window and its COM apartment belong to the thread that built them, so the
// thread-affine Win32 calls it makes (`PostQuitMessage`, `DestroyWindow`,
// `OleUninitialize`) run there with no thread check of their own.
static_assertions::assert_not_impl_any!(WindowsLoop: Send, Sync);

impl std::fmt::Debug for WindowsLoop {
    // Hand-written: the remaining fields are raw platform handles and callback
    // payloads with no useful Debug form.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsLoop")
            .field("windows", &self.windows)
            .finish_non_exhaustive()
    }
}
impl WindowsLoop {
    /// A loop whose windows are `windows`, the window feature's, and that
    /// dispatches through the hooks and subscribers `H` names; with
    /// `instance`, the application's one instance in the session, which
    /// takes the launches later processes hand it.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Init`] when the process's window class is already
    /// registered for another host, and whatever the native setup refuses.
    pub(crate) fn try_new<H: WindowsHost>(
        windows: Arc<dyn ClassWindows>,
        instance: Option<single_instance::FirstInstance>,
    ) -> Result<Self, PlatformError> {
        Self::admit::<H>(windows, WindowIdentity::source(), H::drag, instance)
    }

    /// [`Self::try_new`] holding `drag` in place of the transport `H` makes, for
    /// a test that drives the transport through a handle of its own.
    #[cfg(test)]
    pub(crate) fn with_drag<H: WindowsHost>(
        windows: Arc<dyn ClassWindows>,
        drag: Arc<dyn HostDrag>,
    ) -> Result<Self, PlatformError> {
        Self::admit::<H>(windows, WindowIdentity::source(), move || drag, None)
    }

    /// [`Self::try_new`] with the identity drawn from `source` and the transport
    /// `drag` makes. The owner control's identity is reserved first:
    /// exhaustion refuses before COM is initialized or any HWND exists, so a
    /// refusal (and every caught retry) leaves nothing to release.
    fn admit<H: WindowsHost>(
        windows: Arc<dyn ClassWindows>,
        source: &std::sync::atomic::AtomicU64,
        drag: impl FnOnce() -> Arc<dyn HostDrag>,
        instance: Option<single_instance::FirstInstance>,
    ) -> Result<Self, PlatformError> {
        let owner_identity = WindowIdentity::mint_from(source);

        // Enter this thread's STA for drag-and-drop, clipboard and TSF. The
        // apartment is a value: an early return below releases it, and once
        // built it lives until the platform and every window's text services
        // are gone (see `com_apartment`).
        let apartment = ComApartment::enter().map_err(|hr| PlatformError::Init {
            message: format!("Failed to initialize COM: {hr:?}"),
        })?;

        // SAFETY: `SetProcessDpiAwarenessContext` takes a constant by value,
        // no pointer arguments; failure (already set, or an OS predating
        // per-monitor-v2 awareness) is expected and non-fatal, hence the
        // discarded result.
        //
        // Set DPI awareness to per-monitor v2 (best quality)
        // Ignore errors - this can fail if already set or on older Windows
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            // This native process setting is idempotent for the same value.
            // A host that already fixed it to false keeps the legacy mouse route.
            if let Err(error) = windows::Win32::UI::Input::Pointer::EnableMouseInPointer(true) {
                tracing::debug!(%error, "mouse pointer messages unavailable; retaining mouse messages");
            }
        }

        // Register window class
        Self::register_window_class::<H>()?;

        // Build owner resources first so any later native creation failure
        // retires its preference subscriptions and receiver through the guard.
        let drag = drag();
        let displays = H::displays();
        let app = Arc::new(AppDelivery::new());
        // What launched the application waits for its `ready`.
        app.opened(crate::owner::launch_arguments::launched_by(
            H::launch_arguments(),
        ))
        .or_contain((), "the launch's arguments");
        let notices = H::notices(&app);
        notices.mark_held_stale_on_wake(Arc::downgrade(&displays) as _);
        let owner_control = control::OwnerControl::try_new::<H>(
            owner_identity,
            Arc::downgrade(&windows),
            std::iter::once(Arc::clone(&drag) as Arc<dyn control::OwnerMessages>)
                .chain(instance.map(single_instance::FirstInstance::into_hook))
                .collect::<Vec<Arc<dyn control::OwnerMessages>>>(),
            Arc::clone(&displays),
            notices,
            Arc::clone(&app),
        )?;

        // SAFETY: `GetModuleHandleW(None)` queries the current process
        // image, no pointer arguments. `CreateWindowExW` here creates the
        // message-only window with `HWND_MESSAGE` as parent and constant
        // Win32 arguments (no caller-supplied buffers); its class was just
        // registered above on this same thread, satisfying the ordering
        // Win32 requires (register-before-create).
        //
        // Create message-only window for platform messages
        let message_window = unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|e| PlatformError::Init {
                message: format!("Failed to get module handle: {e:?}"),
            })?;

            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                WINDOW_CLASS_NAME,
                w!("Flui Platform Message Window"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE), // Message-only window
                None,
                Some(hinstance.into()),
                None,
            )
            .map_err(|e| PlatformError::Init {
                message: format!("Failed to create message window: {e:?}"),
            })?
        };

        tracing::info!("Windows platform initialized");

        // The message-only window above was just created on this thread, so
        // its queue belongs here: the owner is decided at construction, and
        // the loop, being neither `Send` nor `Sync`, never leaves it.
        Ok(Self {
            owner_control,
            app,
            drag,
            message_window,
            windows,
            displays,
            apartment,
        })
    }

    /// Register FLUI's window class with `window_proc::<H>` as its procedure,
    /// once per process: Windows holds one class of a name, so a second host
    /// is refused rather than silently given the first one's hooks, and a
    /// failed registration is the answer of every later call too.
    fn register_window_class<H: WindowsHost>() -> Result<(), PlatformError> {
        let registered = WINDOW_CLASS.get_or_init(|| {
            // SAFETY: `window_proc::<H>`'s signature is exactly `WNDPROC`'s
            // `extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT`, so
            // every dispatch to a window of the class is an ABI-matched call.
            // `GetModuleHandleW(None)` takes no pointer arguments.
            // `wc.lpszClassName`/`wc.hCursor` reference process-lifetime
            // statics (`WINDOW_CLASS_NAME`, the loaded cursor resource);
            // `&raw const wc` gives `RegisterClassW` a valid pointer to the
            // fully-initialized, stack-local `WNDCLASSW`. `get_or_init` runs
            // this body at most once, so there is no concurrent registration
            // to race.
            unsafe {
                let hinstance = GetModuleHandleW(None)
                    .map_err(|e| format!("Failed to get module handle: {e}"))?;

                let wc = WNDCLASSW {
                    // No `CS_HREDRAW`/`CS_VREDRAW`: a flip-model surface is
                    // presented whole, and `WM_SIZE` asks for the frame itself.
                    style: CS_OWNDC,
                    lpfnWndProc: Some(Self::window_proc::<H>),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance.into(),
                    hIcon: HICON::default(),
                    hCursor: load_cursor_style(IDC_ARROW)
                        .map_err(|e| format!("Failed to load the default cursor: {e}"))?,
                    hbrBackground: HBRUSH(std::ptr::null_mut()),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: WINDOW_CLASS_NAME,
                };

                let atom = RegisterClassW(&raw const wc);
                if atom == 0 {
                    return Err(format!(
                        "Failed to register the window class: {}",
                        windows::core::Error::from_thread()
                    ));
                }

                tracing::info!("Registered Windows window class");
                Ok(TypeId::of::<H>())
            }
        });
        match registered {
            Ok(host) if *host == TypeId::of::<H>() => Ok(()),
            Ok(_) => Err(PlatformError::Init {
                message: "FLUI's window class is already registered for another host's hooks; \
                          a process has one"
                    .into(),
            }),
            Err(message) => Err(PlatformError::Init {
                message: message.clone(),
            }),
        }
    }

    /// Main window procedure for all FLUI windows: a panic boundary around
    /// [`dispatch_window_message`], which offers the message to `H`'s hooks,
    /// which hold the actual message handling.
    ///
    /// The hooks dispatch framework-supplied callbacks (a window's callbacks,
    /// the platform's window-event handler) that can panic, and a panic must
    /// not unwind across this `extern "system"` frame. The boundary logs it
    /// and ends the process: deliberately NOT swallowed-and-continued
    /// (`DefWindowProcW` as a fallback answer), since a panic mid-`WM_DESTROY`
    /// or mid-dispatch leaves framework state torn, and pumping further
    /// messages over torn state converts one crash into arbitrary later
    /// misbehavior — per `docs/PANIC-POLICY.md`, a panic is a bug report,
    /// never control flow.
    ///
    /// # Safety
    ///
    /// Called by Win32 as a `WNDPROC` for windows of `WINDOW_CLASS_NAME`,
    /// registered as this exact function in `register_window_class`; that
    /// provenance is exactly what [`dispatch_window_message`]'s own `# Safety`
    /// contract requires, so forwarding the arguments verbatim discharges it.
    unsafe extern "system" fn window_proc<H: WindowsHost>(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        panic_boundary::os_entry(
            "FLUI's window procedure",
            (msg, hwnd),
            AfterPanic::Abort,
            || {
                // SAFETY: arguments forwarded verbatim from Win32's dispatch of
                // this registered `WNDPROC` — see `# Safety` above.
                unsafe { dispatch_window_message::<H>(hwnd, msg, wparam, lparam) }
            },
        )
    }

    /// Run the Windows message loop (internal implementation)
    fn run_message_loop(&self) -> Result<(), PlatformError> {
        tracing::info!("Starting Windows message loop");

        let mut deadline = WakeDeadline::default();
        let mut turn_deadline = TurnDeadline::default();
        // Shared with every window this platform opens; without an owner
        // there are no windows, so the count never moves.
        let shares = self.owner_control.shares("run message loop").ok();
        let frames = shares.as_ref().map(|shares| Rc::clone(&shares.frames));
        let preferences = shares.map(|shares| shares.preferences);
        let fallback = Arc::clone(&self.owner_control.wake);
        let wait_handles = [fallback.handle()];
        let frame_count = || frames.as_ref().map_or(0, |frames| frames.get());

        // SAFETY: `msg` is a stack-local, default-initialized `MSG` that
        // outlives every call here. `&raw mut msg` is the only pointer
        // `PeekMessageW` writes through, and `TranslateMessage`/
        // `DispatchMessageW` read `&raw const msg` only after the peek
        // reported a filled message. No pointer into `msg` is held across a
        // call, so the re-entrancy below cannot alias it.
        // `wait_handles` and its strong OwnedHandle owner outlive every native
        // wait, including reentrant shutdown; no handle closes while waiting.
        // The loop runs on the thread that created this
        // platform's windows, so every peek, wait and dispatch addresses that
        // thread's queue, and `DispatchMessageW` (like `PeekMessageW`, which
        // delivers sent messages) re-enters `window_proc`
        // on it. The deadline hook is safe code, called with no borrow of
        // `msg` and no platform lock held, so it may re-enter the platform
        // (close windows, replace itself, request quit) without violating
        // either invariant.
        unsafe {
            let mut msg = MSG::default();

            loop {
                if let Some(source) = &preferences {
                    source.retry_if_due();
                }
                // An admitted due wake is delivery debt. Actuate it before
                // querying user code, which may abandon a late deadline.
                // A platform that quit asks for no frame: the deadline was the
                // run's handler's, and the run is ending (its next answer is
                // none, which forgets the delivery).
                if deadline.take_due(web_time::Instant::now(), frame_count())
                    && self.owner_control.signal.accepting()
                    && self.app.is_bound_here()
                {
                    self.windows.redraw_for_deadline();
                }
                // A due turn deadline is the owner's turn, window or not,
                // and asks no window for a frame.
                if turn_deadline.take_due(web_time::Instant::now())
                    && self.owner_control.signal.accepting()
                {
                    let _ = self.owner_control.signal.wake();
                }

                // Drain native input/paint/quit before another idle query.
                // Peek also services sent messages, so never follow a timed
                // wake with an unconditional GetMessage that might re-park.
                if PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        break;
                    }
                    let _ = TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                    continue;
                }

                // The handler is asked with no borrow of `msg` and no lock
                // held: it may close windows or quit before it answers.
                let armed = deadline.arm(self.query_wake_deadline(), frame_count());
                let turn_armed = turn_deadline.arm(self.query_turn_deadline());

                let next_deadline = armed
                    .into_iter()
                    .chain(turn_armed)
                    .chain(
                        preferences
                            .as_ref()
                            .and_then(|source| source.retry_deadline()),
                    )
                    .min();
                let timeout = next_deadline.map_or(u32::MAX, |deadline| {
                    let remaining = deadline.saturating_duration_since(web_time::Instant::now());
                    // Round upwards so a submillisecond deadline cannot spin;
                    // reserve Win32's INFINITE value for an actual idle wait.
                    let milliseconds = remaining.as_nanos().saturating_add(999_999) / 1_000_000;
                    u32::try_from(milliseconds.min(u128::from(u32::MAX - 1)))
                        .expect("BUG: a bounded native timeout fits u32")
                });
                let outcome = MsgWaitForMultipleObjectsEx(
                    Some(&wait_handles),
                    timeout,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                if outcome == WAIT_FAILED {
                    return Err(PlatformError::EventLoop {
                        message: windows::core::Error::from_thread().to_string(),
                    });
                }
                if outcome == WAIT_OBJECT_0 {
                    self.owner_control.drive_fallback();
                } else if outcome != WAIT_TIMEOUT && outcome.0 != WAIT_OBJECT_0.0 + 1 {
                    return Err(PlatformError::EventLoop {
                        message: format!("unexpected Win32 message wait result: {}", outcome.0),
                    });
                }
            }

            tracing::info!(code = msg.wParam.0, "the message loop exited");
        }
        Ok(())
    }

    /// Ask the application's handler for its wake deadline. `None` when it
    /// asks for none, when no handler is bound, or once the platform no
    /// longer accepts work. Its panic unwinds out of the loop.
    fn query_wake_deadline(&self) -> Option<web_time::Instant> {
        if !self.owner_control.signal.accepting() {
            return None;
        }
        Self::ask_wake_deadline(&self.app)
    }

    /// The wake deadline `app`'s handler asks for, through the call point.
    fn ask_wake_deadline(app: &AppDelivery) -> Option<web_time::Instant> {
        app.wake_deadline().or_resume(None)
    }

    /// Ask the application's handler for its turn deadline, as
    /// [`query_wake_deadline`](Self::query_wake_deadline) asks for its wake
    /// deadline.
    fn query_turn_deadline(&self) -> Option<web_time::Instant> {
        if !self.owner_control.signal.accepting() {
            return None;
        }
        Self::ask_turn_deadline(&self.app)
    }

    /// The turn deadline `app`'s handler asks for, through the call point.
    fn ask_turn_deadline(app: &AppDelivery) -> Option<web_time::Instant> {
        app.turn_deadline().or_resume(None)
    }

    /// Registers the keyboard layout callback in the owner-thread hooks.
    ///
    /// Once the platform has shut down, the registration is refused and
    /// `callback` is dropped here. A replaced callback is dropped through the
    /// call point after the `RefCell` borrow ends, since its destructor may
    /// re-enter the platform, in a boundary of its own: a capture that panics
    /// in `Drop` must not unwind into the host.
    pub(crate) fn register_layout_callback(&self, op: &'static str, callback: Box<dyn FnMut()>) {
        match self.owner_control.shares(op) {
            Ok(shares) => {
                let slot = Rc::clone(&shares.app_hooks.borrow().keyboard_layout_changed);
                slot.register(callback);
            }
            Err(reason) => {
                tracing::debug!(
                    op,
                    ?reason,
                    "platform callback registration refused: the platform has shut down"
                );
                drop(callback);
            }
        }
    }
}

impl host::Loop for WindowsLoop {
    fn run(
        self: Rc<Self>,
        platform: Rc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError> {
        tracing::info!("Running Windows platform");

        struct RunGuard(Rc<WindowsLoop>);
        impl Drop for RunGuard {
            fn drop(&mut self) {
                // Close (which frees the owner context on this thread), and
                // only then tell the handler the platform quits: it still
                // observes a closed platform. The delivery hears the quit once
                // and contains its panic: this is a destructor, possibly of an
                // unwind already under way. The handler is released last,
                // contained, or kept while an unwind is under way.
                self.0.owner_control.close();
                self.0
                    .app
                    .quit_requested()
                    .or_contain((), "the application's quit");
                self.0.app.release();
            }
        }
        // The delivery holds the platform and the hooks weakly: they live in
        // this frame until the loop ends, declared before the guard so they
        // outlive it and the handler can still hear the quit.
        let hooks: Rc<dyn OwnerHooks> = Rc::new(control::WindowsOwnerHooks::new(
            Rc::clone(&platform),
            &self.owner_control,
        ));
        let guard = RunGuard(Rc::clone(&self));
        self.app.bind(handler, &platform, &hooks);
        self.app.resume_through_signal(&self.owner_control.signal);
        self.app
            .ready(OwnerPlatform::new(Rc::clone(&platform), Rc::clone(&hooks)))
            .map_err(PlatformError::bootstrap)?;
        self.owner_control
            .signal
            .start()
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            })?;
        // A panic of the loop (a wake deadline's) leaves it here, and the
        // teardown runs; then the exit policy's parked panic, asked inside the
        // window procedure where no unwind may go, comes out of `run` ahead of
        // it as the run's first failure (`AppDelivery::resume_after_run`).
        let mut failure = flui_foundation::panic::FirstPanic::new();
        let ran = failure.contain(|| self.run_message_loop());
        drop(guard);
        self.app.resume_after_run(failure);
        ran.expect("BUG: a loop that raised no panic returned its result")
    }

    fn quit(&self) {
        self.owner_control.close_signal();
        tracing::info!("Quitting Windows platform");
        // SAFETY: `PostQuitMessage` takes a plain `i32` exit code, no
        // pointer arguments. It posts to the calling thread's queue, which is
        // the loop's: the loop is neither `Send` nor `Sync`, so `quit` runs
        // on the thread that built it (ADR-0327 §5).
        unsafe {
            PostQuitMessage(0);
        }
    }

    fn request_exit_policy_reevaluation(&self) {
        self.owner_control.exit_policy_request().post();
    }
}

impl host::Launch for WindowsLoop {
    /// Only a page has an origin.
    fn page_origin(&self) -> Option<String> {
        None
    }

    fn launch_value(&self, key: flui_platform::LaunchKey) -> Option<String> {
        host::environment_launch_value(key)
    }
}

/// The displays position is the loop's: it answers from the displays the table
/// made ([`WindowsHost::displays`]), so the position and the hook name one
/// object.
impl host::Displays<WindowsLoop> for WindowsLoop {
    fn snapshot(owner: &WindowsLoop) -> Result<DisplaySnapshot, PlatformError> {
        owner.displays.snapshot()
    }
}

/// The preference position is the loop's: it reads the source the table made
/// ([`WindowsHost::preferences`]) on the owner thread.
impl host::Preferences<WindowsLoop> for WindowsLoop {
    fn sample(owner: &WindowsLoop) -> Result<flui_platform::SystemPreferences, PlatformError> {
        owner
            .owner_control
            .shares("read system preferences")
            .map_err(|reason| PlatformError::Preferences {
                message: format!("platform owner is unavailable: {reason:?}"),
            })?
            .preferences
            .sample()
    }
}

/// The lifecycle position is the loop's: the broadcast receiver tells the
/// lifecycle the table made ([`WindowsHost::lifecycle`]) of the session, power,
/// lock and memory messages, and the lifecycle asks the application's handler.
impl host::Lifecycle<WindowsLoop> for WindowsLoop {
    fn release_session_end(owner: &WindowsLoop) {
        if let Ok(shares) = owner.owner_control.shares("release_session_end") {
            shares.lifecycle.release_session_end();
        }
    }

    /// `WM_QUERYENDSESSION` answered `FALSE` with a `ShutdownBlockReason`
    /// holds the session until the reason is destroyed.
    fn session_end_support(_: &WindowsLoop) -> flui_platform::SessionEndSupport {
        flui_platform::SessionEndSupport::Hold
    }

    /// Windows has no application to bring forward, only its windows: a
    /// window comes forward through `WindowCommand::Activate`, under the
    /// foreground rules. Nothing here touches the foreground, least of all
    /// another application's window.
    fn activate(_: &WindowsLoop, _ignoring_other_apps: bool) {}
}

impl Drop for WindowsLoop {
    fn drop(&mut self) {
        tracing::debug!("Dropping WindowsLoop");

        // Destroy message window
        if !self.message_window.is_invalid() {
            // SAFETY: `DestroyWindow` takes `self.message_window` by value; the
            // `is_invalid()` guard skips the call for a handle that was never
            // successfully created. This window carries no `GWLP_USERDATA`
            // context (it is message-only and never routed through
            // `WindowsWindow::try_new`), so there is no allocation to reclaim
            // here.
            unsafe {
                if let Err(error) = DestroyWindow(self.message_window) {
                    tracing::warn!(
                        hwnd = ?self.message_window,
                        ?error,
                        "DestroyWindow failed for the message-only window"
                    );
                }
            }
        }

        // COM is left by the `apartment` field, after this body, or later by
        // the last window's text services if any still live.
    }
}
