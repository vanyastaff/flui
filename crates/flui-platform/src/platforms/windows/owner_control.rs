//! Dedicated message-only HWND: its userdata is never a visible WindowContext.
//!
//! Its `GWLP_USERDATA` slot holds the [`OwnerControlContext`]: the platform's
//! owner-thread state. Only the owner thread can reach it (through
//! [`OwnerControl::shares`], or the window procedure), and it is freed by
//! `WM_NCDESTROY` on that thread, so what it owns is never run or dropped
//! anywhere else. A platform dropped off its owner thread keeps the native
//! window, and so leaks the context instead.
//!
//! The owner window is also where the exit policy is decided: a request for
//! it (the last tracked window's `WM_DESTROY`, or
//! `Platform::request_exit_policy_reevaluation` from any thread) is posted
//! here as one coalesced message, so the hook always runs on a fresh owner
//! turn rather than nested inside whatever code destroyed the window.
use super::platform::{WindowIdentity, WindowMap};
use crate::{
    PlatformError, WakeRegistrationError,
    shared::{
        PlatformHandlers,
        hwnd_affinity::{UserDataRefusal, UserDataVerdict, classify_user_data_access},
        owner_signal::{OwnerSignal, OwnerTurnSlot},
        panic_boundary::contain_owner_callback,
    },
    traits::{
        OpenWindowError, Platform, WindowOptions,
        owner::{DirectOwnerHooks, OwnerHooks, ProxyTransport, WindowOpen},
    },
};
use parking_lot::Mutex;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClassNameW,
            GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW,
            PostQuitMessage, RegisterClassW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE,
            WM_APP, WM_NCDESTROY, WNDCLASSW,
        },
    },
    core::{PCWSTR, w},
};
const WAKE: u32 = WM_APP + 19;
/// Asks the owner to consult the exit policy; see [`ExitPolicyRequest`].
const EXIT_POLICY: u32 = WM_APP + 20;
const OWNER_CLASS: PCWSTR = w!("FLUIOwnerSignalWindow");
static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

/// The platform's owner-thread state, boxed into the owner window's
/// `GWLP_USERDATA` slot. `!Send`: it is created, used and freed on the owner
/// thread only.
pub(super) struct OwnerControlContext {
    /// Which platform's owner window this context belongs to, so a gate
    /// holding a recycled handle cannot reach another platform's state.
    identity: WindowIdentity,
    signal: Arc<OwnerSignal>,
    handlers: Rc<RefCell<PlatformHandlers>>,
    turn: Rc<OwnerTurnSlot>,
    /// The platform's tracked top-level windows, read by the exit-policy
    /// check. Weak: the owner window does not keep the platform's state alive.
    windows: Weak<WindowMap>,
    /// Set while an [`EXIT_POLICY`] message is queued; see
    /// [`ExitPolicyRequest`].
    exit_policy_pending: Arc<AtomicBool>,
}

/// Owner-thread handles cloned out of the [`OwnerControlContext`]. Holding
/// clones instead of a borrow means no reference into the context survives
/// user code, which may tear the owner window down.
pub(super) struct OwnerShares {
    /// The platform-level handlers (`on_quit`, `on_window_event`, ...),
    /// shared with every window context this platform opens.
    pub(super) handlers: Rc<RefCell<PlatformHandlers>>,
    /// Where the owner-turn callback waits between turns.
    pub(super) turn: Rc<OwnerTurnSlot>,
    /// Posts the owner's exit-policy check; handed to every window context
    /// so the last window's `WM_DESTROY` can ask for it.
    pub(super) exit_policy: ExitPolicyRequest,
}

/// A coalesced request, postable from any thread, that the owner consult the
/// exit policy: if the platform tracks no top-level window, the
/// `PlatformHandlers::exit_policy` hook (or, with none installed, the default
/// "exit when the last window closes") decides whether the message loop
/// quits. Requests made while one is already queued cost nothing; once the
/// owner window is closed they are refused, like a wake.
#[derive(Clone)]
pub(super) struct ExitPolicyRequest {
    address: Arc<Mutex<Option<isize>>>,
    pending: Arc<AtomicBool>,
}

impl ExitPolicyRequest {
    pub(super) fn post(&self) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let address = self.address.lock();
        let Some(raw) = *address else {
            tracing::debug!("exit-policy request refused: the owner message window is closed");
            return;
        };
        // SAFETY: the address is read under the lock `OwnerControl::close`
        // takes it out under before destroying the window, so it names the
        // live owner window; posting carries no pointer or closure and never
        // runs the procedure synchronously.
        let posted =
            unsafe { PostMessageW(Some(HWND(raw as *mut _)), EXIT_POLICY, WPARAM(0), LPARAM(0)) };
        if let Err(error) = posted {
            self.pending.store(false, Ordering::Release);
            tracing::warn!(%error, "exit-policy request could not be posted to the owner");
        }
    }
}

/// The owner window's address, and the owner-thread gate onto its context.
/// `Send + Sync`: it carries only the address, so the owner hooks can hold
/// one; reaching the context through it still requires the owner thread.
#[derive(Clone)]
pub(super) struct OwnerGate {
    address: Arc<Mutex<Option<isize>>>,
    identity: WindowIdentity,
    exit_policy_pending: Arc<AtomicBool>,
}

pub(super) struct OwnerControl {
    pub(super) signal: Arc<OwnerSignal>,
    gate: OwnerGate,
}
impl OwnerControl {
    pub(super) fn new(windows: Weak<WindowMap>) -> Result<Self, PlatformError> {
        let registration = REGISTERED.get_or_init(|| {
            // SAFETY: class name lives forever; this exact procedure implements its ABI.
            unsafe {
                let instance = GetModuleHandleW(None).map_err(|error| error.to_string())?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance.into(),
                    lpszClassName: OWNER_CLASS,
                    ..Default::default()
                };
                if RegisterClassW(&raw const class) == 0 {
                    Err(windows::core::Error::from_thread().to_string())
                } else {
                    Ok(())
                }
            }
        });
        registration
            .as_ref()
            .map_err(|message| PlatformError::Init {
                message: message.clone(),
            })?;
        let address = Arc::new(Mutex::new(None::<isize>));
        let target = Arc::clone(&address);
        let signal = OwnerSignal::new(Arc::new(move || {
            let address = target.lock();
            let raw = address.ok_or_else(|| PlatformError::EventLoop {
                message: "owner message window is closed".into(),
            })?;
            // SAFETY: admission is closed before this address is retired. Posting
            // carries no pointer or user closure and never synchronously invokes the procedure.
            unsafe { PostMessageW(Some(HWND(raw as *mut _)), WAKE, WPARAM(0), LPARAM(0)) }.map_err(
                |error| PlatformError::EventLoop {
                    message: error.to_string(),
                },
            )
        }));
        let identity = WindowIdentity::mint();
        let exit_policy_pending = Arc::new(AtomicBool::new(false));
        let context = Box::new(OwnerControlContext {
            identity,
            signal: Arc::clone(&signal),
            handlers: Rc::new(RefCell::new(PlatformHandlers::default())),
            turn: Rc::new(OwnerTurnSlot::default()),
            windows,
            exit_policy_pending: Arc::clone(&exit_policy_pending),
        });
        // SAFETY: dedicated registered class, created on this (the owner) thread;
        // userdata is the boxed context, reclaimed exactly once by WM_NCDESTROY.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                OWNER_CLASS,
                w!("FLUI owner"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(context) as isize);
            hwnd
        };
        {
            let mut address_slot = address.lock();
            *address_slot = Some(hwnd.0 as isize);
        }
        Ok(Self {
            signal,
            gate: OwnerGate {
                address,
                identity,
                exit_policy_pending,
            },
        })
    }

    /// See [`OwnerGate::shares`].
    pub(super) fn shares(&self, op: &'static str) -> Result<OwnerShares, UserDataRefusal> {
        self.gate.shares(op)
    }

    /// A gate the owner hooks can hold.
    pub(super) fn gate(&self) -> OwnerGate {
        self.gate.clone()
    }

    /// See [`ExitPolicyRequest`].
    pub(super) fn exit_policy_request(&self) -> ExitPolicyRequest {
        self.gate.exit_policy_request()
    }

    /// Stops owner turns. On the owner thread the registered turn callback
    /// is dropped at once; elsewhere it stays in the owner context, which the
    /// owner frees when it destroys the owner window. If the platform is
    /// dropped off the owner instead, [`Self::close`] keeps the native window
    /// and the context, callback included, is leaked rather than freed on
    /// the wrong thread.
    pub(super) fn close_signal(&self) {
        self.signal.close();
        if let Ok(shares) = self.shares("close owner turns") {
            shares.turn.clear();
        }
    }

    pub(super) fn close(&self) {
        self.close_signal();
        let address = self.gate.address.lock().take();
        if let Some(address) = address {
            // SAFETY: native destruction occurs on the recorded owner, outside
            // the address/state locks. WM_NCDESTROY owns userdata reclamation.
            if self.signal.owner() == std::thread::current().id() {
                if let Err(error) = unsafe { DestroyWindow(HWND(address as *mut _)) } {
                    tracing::error!(%error, "owner message window destruction failed");
                }
            } else {
                tracing::error!("owner message window dropped off owner; native resource retained");
            }
        }
    }
}

impl OwnerGate {
    fn exit_policy_request(&self) -> ExitPolicyRequest {
        ExitPolicyRequest {
            address: Arc::clone(&self.address),
            pending: Arc::clone(&self.exit_policy_pending),
        }
    }

    /// Clones the owner-thread handles out of the owner context, or says why
    /// this thread may not have them: [`UserDataRefusal::ForeignThread`] off
    /// the owner, [`UserDataRefusal::WindowGone`] or
    /// [`UserDataRefusal::EmptySlot`] once the platform has shut down,
    /// including when its handle now names another platform's owner window.
    pub(super) fn shares(&self, op: &'static str) -> Result<OwnerShares, UserDataRefusal> {
        let raw = *self.address.lock();
        let Some(raw) = raw else {
            tracing::debug!(op, "refusing owner state: the owner window is closed");
            return Err(UserDataRefusal::WindowGone);
        };
        let hwnd = HWND(raw as *mut _);
        // SAFETY: thread and slot queries take the handle by value and
        // dereference nothing; none of them pumps this thread's queue.
        let (owner_thread, current_thread, slot) = unsafe {
            (
                GetWindowThreadProcessId(hwnd, None),
                GetCurrentThreadId(),
                GetWindowLongPtrW(hwnd, GWLP_USERDATA),
            )
        };
        match classify_user_data_access(owner_thread, current_thread, names_owner_class(hwnd), slot)
        {
            UserDataVerdict::Deref => {
                // SAFETY: the verdict established that this is the owner
                // thread and the live window is of the owner class, whose
                // non-null userdata is the `Box<OwnerControlContext>` `new`
                // installed. Only `WM_NCDESTROY`, dispatched on this same
                // thread, frees it, and nothing between the slot read and
                // these clones dispatches a message; no reference outlives
                // this block.
                let context = unsafe { &*(slot as *const OwnerControlContext) };
                if context.identity != self.identity {
                    // The handle was recycled for another platform's owner
                    // window after this platform's was destroyed.
                    tracing::debug!(op, "refusing owner state: the handle names another owner");
                    return Err(UserDataRefusal::WindowGone);
                }
                Ok(OwnerShares {
                    handlers: Rc::clone(&context.handlers),
                    turn: Rc::clone(&context.turn),
                    exit_policy: self.exit_policy_request(),
                })
            }
            UserDataVerdict::Refuse(reason) => {
                tracing::debug!(op, ?reason, "refusing owner state");
                Err(reason)
            }
        }
    }
}

/// The Win32 [`OwnerHooks`]: window creation and the proxy transport as
/// [`DirectOwnerHooks`] provides them, with the owner-turn callback
/// registered into the owner context's [`OwnerTurnSlot`] instead of the
/// signal's shared slot.
pub(super) struct WindowsOwnerHooks {
    direct: DirectOwnerHooks,
    signal: Arc<OwnerSignal>,
    gate: OwnerGate,
}

impl WindowsOwnerHooks {
    pub(super) fn new(platform: Arc<dyn Platform>, control: &OwnerControl) -> Self {
        Self {
            direct: DirectOwnerHooks::with_signal(platform, Arc::clone(&control.signal)),
            signal: Arc::clone(&control.signal),
            gate: control.gate(),
        }
    }
}

impl OwnerHooks for WindowsOwnerHooks {
    fn on_wake(&self, callback: Box<dyn FnMut() + Send>) -> Result<(), WakeRegistrationError> {
        // `OwnerPlatform`, the only caller, is `!Send`, so this runs on the
        // owner thread; a refusal here means the owner window is gone.
        match self.gate.shares("on_wake") {
            Ok(shares) => self.signal.register_in(&*shares.turn, callback),
            Err(reason) => {
                tracing::debug!(?reason, "owner-turn registration refused");
                drop(callback);
                Err(WakeRegistrationError::OwnerGone)
            }
        }
    }

    fn open_owner_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError> {
        self.direct.open_owner_window(options)
    }

    fn transport(&self) -> Arc<dyn ProxyTransport> {
        self.direct.transport()
    }
}
impl Drop for OwnerControl {
    fn drop(&mut self) {
        self.close();
    }
}

/// Whether the live window behind `hwnd` is of the owner class, the only
/// class whose userdata is an [`OwnerControlContext`].
fn names_owner_class(hwnd: HWND) -> bool {
    let mut name = [0_u16; 256];
    // SAFETY: `GetClassNameW` writes at most `name.len()` units into the
    // live stack buffer and returns the count excluding the terminator (0
    // for a dead handle); `OWNER_CLASS` is a NUL-terminated static literal
    // measured without its terminator, the convention `class_name_matches`
    // expects.
    let (copied, expected) = unsafe { (GetClassNameW(hwnd, &mut name), OWNER_CLASS.as_wide()) };
    crate::shared::hwnd_affinity::class_name_matches(&name, copied, expected)
}

/// Ends the owner's message loop, as `Platform::quit` does: admission and
/// owner turns stop, then `WM_QUIT` is posted to this (the owner) thread.
fn quit_owner_loop(signal: &OwnerSignal, turn: &OwnerTurnSlot) {
    signal.close();
    turn.clear();
    // SAFETY: `PostQuitMessage` takes a plain exit code; this runs in the
    // owner procedure, so it posts to the owner thread's queue.
    unsafe { PostQuitMessage(0) };
}

/// The owner's exit decision: no tracked top-level window, the platform
/// still accepting work (a quit already under way is not decided twice),
/// and the exit-policy hook allowing it. No hook installed allows it.
///
/// The hook is leased out of its slot and run with no `RefCell` borrow
/// held, because it re-enters the embedder, which may install a new hook,
/// open a window or quit; it goes back only if the slot is still empty, so
/// a hook installed meanwhile wins. A window opened while it ran vetoes the
/// exit regardless of its answer, and so does a hook that panics.
fn exit_policy_allows_exit(
    signal: &OwnerSignal,
    handlers: &RefCell<PlatformHandlers>,
    windows: &Weak<WindowMap>,
) -> bool {
    let no_windows = || windows.upgrade().is_some_and(|map| map.lock().is_empty());
    if !signal.accepting() || !no_windows() {
        return false;
    }
    let hook = handlers.borrow_mut().exit_policy.take();
    // The hook is embedder code running inside this window procedure, where
    // an unwind cannot cross the FFI boundary. A panic is contained and
    // counts as a veto; the hook stays installed.
    let mut allowed = false;
    contain_owner_callback(|| allowed = hook.as_ref().is_none_or(|hook| hook()));
    if let Some(hook) = hook {
        let superseded = {
            let mut handlers = handlers.borrow_mut();
            if handlers.exit_policy.is_none() {
                handlers.exit_policy = Some(hook);
                None
            } else {
                Some(hook)
            }
        };
        contain_owner_callback(|| drop(superseded));
    }
    allowed && signal.accepting() && no_windows()
}

// SAFETY: Win32 invokes this exact signature for the dedicated registered class.
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut result = LRESULT(0);
    contain_owner_callback(|| {
        // SAFETY: only this class stores `OwnerControlContext` userdata, and
        // Win32 runs this procedure on the owner thread, the only thread
        // that frees it (`WM_NCDESTROY`, below). A wake turn clones the
        // signal out before running callbacks, so a callback that tears the
        // window down cannot free what the turn still uses.
        unsafe {
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OwnerControlContext;
            if message == WM_NCDESTROY && !pointer.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
            } else if message == WAKE && !pointer.is_null() {
                let signal = Arc::clone(&(*pointer).signal);
                let turn = Rc::clone(&(*pointer).turn);
                if signal.drive_in(&*turn) {
                    quit_owner_loop(&signal, &turn);
                }
                return;
            } else if message == EXIT_POLICY && !pointer.is_null() {
                let context = &*pointer;
                context.exit_policy_pending.store(false, Ordering::Release);
                let signal = Arc::clone(&context.signal);
                let turn = Rc::clone(&context.turn);
                let handlers = Rc::clone(&context.handlers);
                let windows = context.windows.clone();
                if exit_policy_allows_exit(&signal, &handlers, &windows) {
                    quit_owner_loop(&signal, &turn);
                }
                return;
            }
            result = DefWindowProcW(hwnd, message, wparam, lparam);
        }
    });
    result
}
