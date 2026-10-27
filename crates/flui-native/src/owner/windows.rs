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
#[cfg(test)]
mod tests;
// The quiet host the loop's tests run over, and another host's tests name.
#[cfg(test)]
pub(crate) mod testing;
mod wake_deadline;
mod window_table;

use std::{
    any::TypeId,
    cell::RefCell,
    rc::Rc,
    sync::{Arc, OnceLock, Weak},
};

use flui_platform::CursorError;
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::drag::PlatformDragSource;
use flui_platform::text_store::TextStoreHost;
use windows::{
    Win32::{
        Foundation::{
            ERROR_CANCELLED, HWND, LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
            WPARAM,
        },
        Graphics::Gdi::HBRUSH,
        System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::{
                CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                HCURSOR, HICON, HWND_MESSAGE, IDC_ARROW, LoadCursorW, MSG, MWMO_INPUTAVAILABLE,
                MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, PostQuitMessage, QS_ALLINPUT,
                RegisterClassW, SW_SHOWNORMAL, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_QUIT, WNDCLASSW,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::owner::call_point::{Field, Once, SharedField, call, install, retire};
use crate::owner::notices::{NoticeChannel, StaleOnWake};
use crate::owner::panic_boundary::{self, AfterPanic};
use crate::{
    error::PlatformError,
    executor::BackgroundExecutor,
    host,
    owner::OwnerHooks,
    owner::OwnerPlatform,
    owner::handlers::{OpenWindows, PlatformHandlers},
    owner::hwnd_affinity::UserDataRefusal,
    owner::window_delivery::WindowDelivery,
    platform::Platform,
    platform::PlatformExecutor,
    platform::PlatformReadyCallback,
};
use broadcast::Subscriber;
use com_apartment::ComApartment;
use flui_platform::{DisplayId, DisplaySnapshot};
pub(crate) use wake_deadline::FrameCount;
use wake_deadline::ModalDelivery;
use wake_deadline::{DeadlineArm, DeadlineDelivery, WakeDeadlineHook};
pub(crate) use window_table::{WindowScale, WindowTable};

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

/// FLUI's window class on this thread for the host the process registered
/// the class for; `None` on a thread no owner of that host opened on.
fn registered_class() -> Option<WindowClass> {
    let host = *WINDOW_CLASS.get()?.as_ref().ok()?;
    CLASS_ON_THREAD
        .try_with(|classes| {
            classes
                .borrow()
                .iter()
                .find(|(made_for, _)| *made_for == host)
                .map(|(_, class)| class.clone())
        })
        .ok()
        .flatten()
}

/// What the input of FLUI's window class on this thread tells the other
/// features of a window, for the host the process registered the class for;
/// `None` on a thread no owner of that host opened on.
pub(crate) fn window_inputs() -> Option<Rc<dyn WindowInputs>> {
    registered_class().map(|class| class.input)
}

/// What the text input of FLUI's window class on this thread holds of a
/// window, for the host the process registered the class for; `None` on a
/// thread no owner of that host opened on.
pub(crate) fn window_texts() -> Option<Rc<dyn WindowTexts>> {
    registered_class().map(|class| class.text)
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

    /// The kind of the contact whose press began `pointer`'s sequence in the
    /// window.
    fn press_kind(
        &self,
        window: WindowRef,
        pointer: flui_platform::PointerId,
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
    fn notices() -> Arc<dyn NoticeChannel>;

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
pub(crate) trait HostDrag: control::OwnerMessages + DataTransferSource {
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
    fn scale(&self) -> f64;
}

/// The drag position: the transport the host's [`WindowsHost::drag`] made,
/// which the loop holds.
impl host::Drag<WindowsLoop> for WindowsLoop {
    fn data_transfer(owner: &WindowsLoop) -> Arc<dyn DataTransferSource> {
        Arc::clone(&owner.drag) as Arc<dyn DataTransferSource>
    }
}

/// The windows of FLUI's class, which the window feature opens and keeps
/// (ADR-0151 §10).
///
/// The loop holds them without naming them: the host builds the window
/// feature's object and hands it to [`WindowsLoop::new`]. The loop asks it
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
    /// The application's handlers, shared with every window of the loop.
    pub(crate) handlers: Rc<RefCell<PlatformHandlers>>,
    /// The system preference source the loop keeps, which measures the
    /// system's mouse geometry at the window's scale.
    pub(crate) preferences: Rc<dyn control::OwnerPreferences>,
    /// The hooks of its class, which a feature tells a step of its own
    /// ([`window_step`]).
    pub(crate) hooks: WindowHooks,
    /// The owner thread's COM apartment, when the window was opened with it.
    pub(crate) apartment: Option<Rc<ComApartment>>,
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
    /// It is being destroyed and its close callbacks ran, so the presentation
    /// that showed it is closed; its handle is still valid.
    Closed,
    /// The application let a keydown through (did not prevent its default):
    /// the window's own keys follow it. `repeat` when the key auto-repeats.
    KeyLetThrough {
        /// Its virtual-key code.
        virtual_key: u16,
        /// Whether it auto-repeats.
        repeat: bool,
    },
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
    // A clone of the hooks: a hook's message may open or close windows, and
    // nothing of the thread's class stays borrowed while it runs.
    let hooks = window_class::<H>().map(|class| class.hooks);
    let answered = hooks
        .iter()
        .flat_map(|hooks| hooks.iter())
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
    answered.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) })
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
            .map_err(|e| CursorError::Backend(format!("Failed to load cursor: {e}")))
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

    /// The admission boundary `WindowsWindow::new` runs: reserve an
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
            // The admission `WindowsWindow::new` runs, with an acquisition
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
    /// Background executor for async tasks
    background_executor: Arc<BackgroundExecutor>,

    /// The monitors this platform last sampled and the registry of their ids,
    /// as the table made them.
    pub(crate) displays: Arc<dyn HostDisplays>,

    /// Records the owner thread so the thread-affine Win32 operations below
    /// can `debug_assert` their caller (ADR-0039). Bound at construction —
    /// the message-only window's queue belongs to the constructing thread —
    /// and re-asserted by `run`. Pre-run window creation on that same
    /// thread (the Win32 examples) stays legal.
    pub(crate) affinity: flui_foundation::OwnerAffinity,

    /// The owner thread's COM apartment. Declared last, so it is released
    /// after everything above; each window's text services hold their own
    /// clone, so COM outlives the platform while any of them is alive.
    pub(crate) apartment: com_apartment::ApartmentHold,
}

// SAFETY, per field: `windows` and `displays` are `Send + Sync` by their
// traits' bounds, `owner_control` holds only an `Arc`-shared signal and a
// locked address (its owner-thread state, callbacks included, sits behind the
// owner window's `GWLP_USERDATA` and is reached only through the owner-thread
// gate `OwnerControl::shares`), and the executors are `Arc`-shared and
// internally synchronized. The only non-`Sync` member is
// `message_window: HWND`, a bare address that is never dereferenced here.
//
// NOT claimed — an earlier version of this comment claimed both, wrongly: that
// an HWND is "thread-safe by design" (it is thread-AFFINE; its message queue
// belongs to the creating thread, and `DestroyWindow` must run there), and that
// the struct is itself just a handle. Sending the struct is sound because the
// address alone aliases nothing; any Win32 call made through it still owes the
// thread-affinity obligation. The soundness-relevant slice of that obligation
// — dereferencing per-window `GWLP_USERDATA` contexts, and `DestroyWindow` —
// is discharged by the window feature's `with_window_context`/`teardown_route`
// gates over `hwnd_affinity` (see `WindowsWindow`'s `Send`/`Sync` docs); the
// remaining affine calls here
// (`quit`'s `PostQuitMessage`, `open_window`'s queue binding) are logic-level
// rather than memory-safety and stay guarded by `affinity.debug_assert_owner`
// — see ADR-0039 (event-loop affinity). `apartment` is `Send + Sync` by its
// own owner-thread gate, and `drag` is `Send + Sync` by its trait's bound
// (the OLE transport keeps its COM objects in an owner slot).
unsafe impl Send for WindowsLoop {}
// SAFETY: as for `Send` — `&WindowsLoop` grants no more than shared access
// to already-synchronized members plus a never-dereferenced address.
unsafe impl Sync for WindowsLoop {}

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
    /// dispatches through the hooks and subscribers `H` names.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Init`] when the process's window class is already
    /// registered for another host, and whatever the native setup refuses.
    pub(crate) fn new<H: WindowsHost>(
        windows: Arc<dyn ClassWindows>,
    ) -> Result<Self, PlatformError> {
        Self::admit::<H>(windows, WindowIdentity::source(), H::drag)
    }

    /// [`Self::new`] holding `drag` in place of the transport `H` makes, for
    /// a test that drives the transport through a handle of its own.
    #[cfg(test)]
    pub(crate) fn with_drag<H: WindowsHost>(
        windows: Arc<dyn ClassWindows>,
        drag: Arc<dyn HostDrag>,
    ) -> Result<Self, PlatformError> {
        Self::admit::<H>(windows, WindowIdentity::source(), move || drag)
    }

    /// [`Self::new`] with the identity drawn from `source` and the transport
    /// `drag` makes. The owner control's identity is reserved first:
    /// exhaustion refuses before COM is initialized or any HWND exists, so a
    /// refusal (and every caught retry) leaves nothing to release.
    fn admit<H: WindowsHost>(
        windows: Arc<dyn ClassWindows>,
        source: &std::sync::atomic::AtomicU64,
        drag: impl FnOnce() -> Arc<dyn HostDrag>,
    ) -> Result<Self, PlatformError> {
        let owner_identity = WindowIdentity::mint_from(source);

        // Enter this thread's STA for drag-and-drop, clipboard and TSF. The
        // apartment is a value: an early return below releases it, and once
        // built it lives until the platform and every window's text services
        // are gone (see `com_apartment`). This thread is also the one the
        // platform's `affinity` binds to below.
        let apartment = com_apartment::ComApartment::enter().map_err(|hr| PlatformError::Init {
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
        let notices = H::notices();
        notices.mark_held_stale_on_wake(Arc::downgrade(&displays) as _);
        let owner_control = control::OwnerControl::new::<H>(
            owner_identity,
            Arc::downgrade(&windows),
            vec![Arc::clone(&drag) as Arc<dyn control::OwnerMessages>],
            Arc::clone(&displays),
            notices,
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

        // Create executors
        let background_executor = Arc::new(BackgroundExecutor::new());

        tracing::info!("Windows platform initialized with Tokio executors");

        let platform = Self {
            owner_control,
            drag,
            message_window,
            windows,
            background_executor,
            displays,
            affinity: flui_foundation::OwnerAffinity::new(),
            apartment: com_apartment::ApartmentHold::new(apartment),
        };
        // The message-only window above was just created on THIS thread, so
        // its message queue already belongs here — the owner is decided at
        // construction, not at `run`. A later `run` on another thread trips
        // the foreign re-bind assertion instead of silently accepting a
        // thread that cannot service the HWND.
        platform.affinity.bind_current();
        Ok(platform)
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

        let mut armed: Option<DeadlineArm> = None;
        let mut delivered: Option<DeadlineDelivery> = None;
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
                if armed
                    .as_ref()
                    .is_some_and(|arm| arm.deadline <= web_time::Instant::now())
                {
                    let arm = armed.take().expect("BUG: a due deadline is armed");
                    if self
                        .deadline_hook()
                        .is_some_and(|hook| Weak::ptr_eq(&arm.hook, &Arc::downgrade(&hook)))
                    {
                        let before = frame_count();
                        self.windows.redraw_for_deadline();
                        delivered = Some(DeadlineDelivery {
                            arm,
                            frames: before,
                        });
                    }
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

                // A replaced hook's final captures may themselves replace
                // registration or quit: the query retires its clone before
                // the answer is validated below.
                let next = self.query_wake_deadline();
                armed = match next {
                    Some((deadline, identity))
                        if self.deadline_hook().is_some_and(|hook| {
                            Weak::ptr_eq(&identity, &Arc::downgrade(&hook))
                        }) =>
                    {
                        match deadline {
                            Some(deadline)
                                if delivered.as_ref().is_none_or(|last| {
                                    last.admits(deadline, &identity, frame_count())
                                }) =>
                            {
                                Some(DeadlineArm {
                                    deadline,
                                    hook: identity,
                                })
                            }
                            Some(_) => None,
                            None => {
                                delivered = None;
                                None
                            }
                        }
                    }
                    _ => {
                        delivered = None;
                        None
                    }
                };

                let next_deadline = armed
                    .as_ref()
                    .map(|arm| arm.deadline)
                    .into_iter()
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

            tracing::info!("Message loop exited with code: {}", msg.wParam.0);
        }
        Ok(())
    }

    fn deadline_hook(&self) -> Option<Arc<WakeDeadlineHook>> {
        if !self.owner_control.signal.accepting() {
            return None;
        }
        let shares = self.owner_control.shares("query wake deadline").ok()?;
        shares.handlers.borrow().wake_deadline.clone()
    }

    /// Ask the wake-deadline hook through the call point: its answer, and
    /// the identity of the hook that gave it, so an answer is never credited
    /// to a replacement. `None` when no hook is installed or the platform no
    /// longer accepts work. Its panic unwinds out of the loop.
    fn query_wake_deadline(&self) -> Option<(Option<web_time::Instant>, Weak<WakeDeadlineHook>)> {
        if !self.owner_control.signal.accepting() {
            return None;
        }
        let shares = self.owner_control.shares("query wake deadline").ok()?;
        Self::ask_wake_deadline(&shares.handlers)
    }

    /// [`Self::query_wake_deadline`] over a handler set.
    fn ask_wake_deadline(
        handlers: &RefCell<PlatformHandlers>,
    ) -> Option<(Option<web_time::Instant>, Weak<WakeDeadlineHook>)> {
        call(
            &SharedField::new(handlers, |handlers| &mut handlers.wake_deadline),
            |hook| Some((hook(), Arc::downgrade(hook))),
        )
        .or_resume(None)
    }

    /// Installs a platform-level handler into the owner-thread handler set.
    ///
    /// Off the owner thread, or once the platform has shut down, the
    /// registration is refused and `callback` is dropped here, on the
    /// calling thread. A replaced handler is retired through the call point
    /// after the `RefCell` borrow ends, since its destructor may re-enter the
    /// platform, in a boundary of its own: a capture that panics in `Drop`
    /// must not unwind into the host.
    pub(crate) fn register_handler<H>(
        &self,
        op: &'static str,
        callback: H,
        project: fn(&mut PlatformHandlers) -> &mut Option<H>,
    ) {
        match self.owner_control.shares(op) {
            Ok(shares) => install(&Field::new(&*shares.handlers, project), callback),
            Err(UserDataRefusal::ForeignThread) => {
                tracing::error!(
                    op,
                    "platform callback registration refused: platform delivery are registered \
                     on the owner thread; the callback was dropped on the calling thread"
                );
                drop(callback);
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
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        on_ready: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        tracing::info!("Running Windows platform");

        // Idempotent from the constructing thread; a `run` migrated to a
        // different thread is a bug the re-bind assertion surfaces (the
        // message-only window's queue is bound to the constructing thread).
        self.affinity.bind_current();

        struct RunGuard(Arc<WindowsLoop>);
        impl Drop for RunGuard {
            fn drop(&mut self) {
                // Hold the handler set while the owner context still exists,
                // close (which frees that context on this thread), and only
                // then run the quit callback: it still observes a closed
                // platform. It runs once, and its panic is contained: this
                // is a destructor, possibly of an unwind already under way.
                let handlers = self
                    .0
                    .owner_control
                    .shares("run teardown")
                    .ok()
                    .map(|shares| Rc::clone(&shares.handlers));
                self.0.owner_control.close();
                if let Some(handlers) = handlers {
                    call(
                        &Once(Field::new(&*handlers, |handlers| &mut handlers.quit)),
                        |quit| quit(),
                    )
                    .or_contain((), "the quit callback");
                    // The last handle to the handler set when the owner context
                    // is gone: its handlers' destructors are user code, dropped
                    // contained, or kept while an unwind is under way.
                    retire(handlers);
                }
            }
        }
        let _guard = RunGuard(Arc::clone(&self));
        let hooks: Arc<dyn OwnerHooks> = Arc::new(control::WindowsOwnerHooks::new(
            Arc::clone(&platform),
            &self.owner_control,
        ));
        on_ready(OwnerPlatform::new(platform, hooks)).map_err(PlatformError::bootstrap)?;
        self.owner_control
            .signal
            .start()
            .map_err(|error| PlatformError::EventLoop {
                message: error.to_string(),
            })?;
        self.run_message_loop()
    }

    fn quit(&self) {
        self.owner_control.close_signal();
        // PostQuitMessage posts to the CALLING thread's message queue — off
        // the owner thread it silently quits nothing (ADR-0039).
        self.affinity.debug_assert_owner("WindowsLoop::quit");
        tracing::info!("Quitting Windows platform");
        // SAFETY: `PostQuitMessage` takes a plain `i32` exit code, no
        // pointer arguments; the `debug_assert_owner` above documents (in
        // debug builds) that this runs on the thread whose queue it posts
        // to — posting from elsewhere is a logic bug (silently posts to the
        // wrong queue), not memory-unsafety.
        unsafe {
            PostQuitMessage(0);
        }
    }

    fn set_wake_deadline_hook(&self, hook: Box<WakeDeadlineHook>) {
        if !self.owner_control.signal.accepting() {
            drop(hook);
            return;
        }
        let hook: Arc<WakeDeadlineHook> = Arc::from(hook);
        self.register_handler("set_wake_deadline_hook", hook, |handlers| {
            &mut handlers.wake_deadline
        });
        // Re-query after replacement even when the old query re-entered this
        // setter. The existing owner signal coalesces these notifications.
        if self.owner_control.signal.accepting()
            && let Err(error) = self.owner_control.signal.wake()
        {
            tracing::warn!(%error, "failed to wake Win32 after deadline hook registration");
        }
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.register_handler("on_quit", callback, |handlers| &mut handlers.quit);
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        self.register_handler("set_exit_policy_hook", hook, |handlers| {
            &mut handlers.exit_policy
        });
    }

    fn request_exit_policy_reevaluation(&self) {
        self.owner_control.exit_policy_request().post();
    }
}

impl host::Services for WindowsLoop {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        Arc::clone(&self.background_executor) as Arc<dyn PlatformExecutor>
    }

    fn app_path(&self) -> Result<std::path::PathBuf, PlatformError> {
        // SAFETY: `buffer` is a stack-local `[u16; MAX_PATH]`; `&mut buffer`
        // gives `GetModuleFileNameW` a valid, correctly-sized out-parameter,
        // and only the first `len` code units it reports writing are read
        // below — a `len == 0` failure returns before any read.
        unsafe {
            let mut buffer = [0u16; 260]; // MAX_PATH, stack-allocated
            let len = GetModuleFileNameW(None, &mut buffer);
            if len == 0 {
                return Err(PlatformError::AppPath {
                    message: windows::core::Error::from_thread().to_string(),
                });
            }
            Ok(std::path::PathBuf::from(String::from_utf16_lossy(
                &buffer[..len as usize],
            )))
        }
    }

    fn open_url(&self, url: &str) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        let wide_url: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide_url` is explicitly NUL-terminated
        // (`.chain(std::iter::once(0))`), matching what `PCWSTR` requires,
        // and stays alive for the duration of this call (it is not dropped
        // or reallocated between `.as_ptr()` and the call).
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide_url.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn reveal_path(&self, path: &std::path::Path) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        // Use "explorer /select,<path>" to reveal in Explorer
        let path_str = path.to_string_lossy();
        let arg = format!("/select,{path_str}");
        let wide_arg: Vec<u16> = arg.encode_utf16().chain(std::iter::once(0)).collect();
        let explorer: Vec<u16> = "explorer\0".encode_utf16().collect();
        // SAFETY: see `open_url` above — both `wide_arg` and `explorer` are
        // explicitly NUL-terminated and kept alive across the call.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(explorer.as_ptr()),
                PCWSTR(wide_arg.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn open_path(&self, path: &std::path::Path) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        let wide_path: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: see `open_url` above — `wide_path` is explicitly
        // NUL-terminated and kept alive across the call.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide_path.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    fn prompt_for_paths(
        &self,
        options: crate::platform::PathPromptOptions,
    ) -> crate::task::Task<Result<Option<Vec<std::path::PathBuf>>, PlatformError>> {
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            // COM file dialogs must run on an STA thread. The closure keeps
            // the raw `windows::core::Result` so the COM `?`s inside stay
            // untouched; the typed mapping happens once, at the join below.
            let result = std::thread::spawn(
                move || -> windows::core::Result<Option<Vec<std::path::PathBuf>>> {
                    // SAFETY: this whole body runs on the freshly `std::thread::spawn`ed
                    // thread, which owns no other window state — `CoInitializeEx`
                    // makes it an STA for the `IFileOpenDialog` COM object, as
                    // that interface requires. Every `windows-rs` COM call
                    // (`CoCreateInstance`, `dialog.Show`/`SetOptions`, `results.*`,
                    // `item.GetDisplayName`) is a checked, typed FFI wrapper with
                    // no raw pointer of ours to validate. `CoTaskMemFree` below
                    // frees the `PWSTR` `name` owns from COM — it is only used
                    // (via `name.to_string()`, which copies the string out) before
                    // this free, and each loop iteration gets its own `name` from
                    // `GetDisplayName`, so there is no double-free or use-after-free
                    // across iterations.
                    unsafe {
                        use windows::Win32::{
                            System::Com::{
                                CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance,
                                CoInitializeEx, CoTaskMemFree,
                            },
                            UI::Shell::{
                                FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                                FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
                                SIGDN_FILESYSPATH,
                            },
                        };

                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                        let dialog: IFileOpenDialog =
                            CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL)?;

                        let mut flags = FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
                        if options.multiple {
                            flags |= FOS_ALLOWMULTISELECT;
                        }
                        if options.directories {
                            flags |= FOS_PICKFOLDERS;
                        }
                        dialog.SetOptions(flags)?;

                        match dialog.Show(None) {
                            Ok(()) => {}
                            Err(e)
                                if e.code()
                                    == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) =>
                            {
                                return Ok(None);
                            }
                            Err(e) => return Err(e),
                        }

                        let results = dialog.GetResults()?;
                        let count = results.GetCount()?;
                        let mut paths = Vec::with_capacity(count as usize);
                        for i in 0..count {
                            let item = results.GetItemAt(i)?;
                            let name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
                            let path_str = name.to_string()?;
                            paths.push(std::path::PathBuf::from(path_str));
                            CoTaskMemFree(Some(name.as_ptr() as *const _));
                        }
                        Ok(Some(paths))
                    }
                },
            )
            .join()
            .map_err(|_| PlatformError::Dialog {
                message: "File dialog thread panicked".to_string(),
            })?
            .map_err(|error| PlatformError::Dialog {
                message: error.to_string(),
            })?;
            Ok(result)
        })
    }

    fn prompt_for_new_path(
        &self,
        directory: &std::path::Path,
        suggested_name: Option<&str>,
    ) -> crate::task::Task<Result<Option<std::path::PathBuf>, PlatformError>> {
        let dir = directory.to_path_buf();
        let name = suggested_name.map(std::string::ToString::to_string);
        let executor = self.background_executor.clone();
        executor.spawn(async move {
            // Same raw-`windows::core::Result` closure + single typed
            // mapping shape as `prompt_for_paths` above.
            let result = std::thread::spawn(
                move || -> windows::core::Result<Option<std::path::PathBuf>> {
                    // SAFETY: see `prompt_for_paths` above — same
                    // dedicated-STA-thread, checked-COM-wrapper reasoning.
                    // `dir_wide` is explicitly NUL-terminated and outlives the
                    // `SHCreateItemFromParsingName` call that borrows its
                    // pointer; the single `CoTaskMemFree` below frees the one
                    // `name` this function's single `IFileSaveDialog::GetResult`
                    // path produces, after `name.to_string()` has already copied
                    // the string out.
                    unsafe {
                        use windows::Win32::{
                            System::Com::{
                                CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance,
                                CoInitializeEx, CoTaskMemFree,
                            },
                            UI::Shell::{
                                FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
                                FileSaveDialog, IFileSaveDialog, IShellItem,
                                SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
                            },
                        };

                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                        let dialog: IFileSaveDialog =
                            CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL)?;

                        dialog.SetOptions(
                            FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT,
                        )?;

                        // Set initial directory
                        let dir_wide: Vec<u16> = dir
                            .to_string_lossy()
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        if let Ok(folder) = SHCreateItemFromParsingName::<PCWSTR, _, IShellItem>(
                            PCWSTR(dir_wide.as_ptr()),
                            None,
                        ) {
                            let _ = dialog.SetFolder(&folder);
                        }

                        // Set suggested file name
                        if let Some(ref name) = name {
                            let name_hstring = windows::core::HSTRING::from(name.as_str());
                            let _ = dialog.SetFileName(&name_hstring);
                        }

                        match dialog.Show(None) {
                            Ok(()) => {}
                            Err(e)
                                if e.code()
                                    == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) =>
                            {
                                return Ok(None);
                            }
                            Err(e) => return Err(e),
                        }

                        let result = dialog.GetResult()?;
                        let name = result.GetDisplayName(SIGDN_FILESYSPATH)?;
                        let path_str = name.to_string()?;
                        let path = std::path::PathBuf::from(path_str);
                        CoTaskMemFree(Some(name.as_ptr() as *const _));
                        Ok(Some(path))
                    }
                },
            )
            .join()
            .map_err(|_| PlatformError::Dialog {
                message: "File dialog thread panicked".to_string(),
            })?
            .map_err(|error| PlatformError::Dialog {
                message: error.to_string(),
            })?;
            Ok(result)
        })
    }

    fn compositor_name(&self) -> &'static str {
        // Nothing reads it; the name leaves `Platform` with the other services.
        ""
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
/// lock and memory messages, and the loop keeps the application's hooks.
impl host::Lifecycle<WindowsLoop> for WindowsLoop {
    fn on_session_end(
        owner: &WindowsLoop,
        callback: Box<
            dyn FnMut(flui_platform::SessionEndPhase) -> flui_platform::SessionEndAnswer + Send,
        >,
    ) {
        owner.register_handler("on_session_end", callback, |handlers| {
            &mut handlers.session_end
        });
    }

    fn on_host_notice(
        owner: &WindowsLoop,
        callback: Box<dyn FnMut(flui_platform::SystemNotice) + Send>,
    ) {
        match owner.owner_control.shares("on_host_notice") {
            Ok(shares) => shares.notices.set_hook(callback),
            Err(reason) => {
                tracing::debug!(?reason, "host notice registration refused");
                drop(callback);
            }
        }
    }

    fn release_session_end(owner: &WindowsLoop) {
        if let Ok(shares) = owner.owner_control.shares("release_session_end") {
            shares.lifecycle.release_session_end();
        }
    }

    fn on_reopen(_: &WindowsLoop, callback: Box<dyn FnMut() + Send>) {
        // Windows sends no reopen signal; nothing will call the callback.
        drop(callback);
    }

    fn activate(_: &WindowsLoop, _ignoring_other_apps: bool) {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

        // SAFETY: `GetForegroundWindow`/`SetForegroundWindow` take no
        // pointer arguments — `hwnd` is an opaque value, never dereferenced.
        unsafe {
            // Bring the foreground window to front
            let hwnd = GetForegroundWindow();
            if !hwnd.is_invalid()
                && let Err(error) = SetForegroundWindow(hwnd).ok()
            {
                tracing::warn!(
                    ?hwnd,
                    ?error,
                    "SetForegroundWindow failed in Platform::activate"
                );
            }
        }
    }
}

impl Drop for WindowsLoop {
    fn drop(&mut self) {
        tracing::debug!("Dropping WindowsLoop");

        // Destroy message window
        //
        // SAFETY: `DestroyWindow` takes `self.message_window` by value; the
        // `is_invalid()` guard skips the call for a handle that was never
        // successfully created. This window carries no `GWLP_USERDATA`
        // context (it is message-only and never routed through
        // `WindowsWindow::new`), so there is no allocation to reclaim here.
        if !self.message_window.is_invalid() {
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
