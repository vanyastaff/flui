//! Dedicated message-only HWND: its userdata is never a visible WindowContext.
//!
//! Its `GWLP_USERDATA` slot holds the [`OwnerControlContext`]: the platform's
//! owner-thread state. Only the owner thread can reach it (through
//! [`OwnerControl::shares`], or the window procedure), and it is freed by
//! `WM_NCDESTROY` on that thread, so what it owns is never run or dropped
//! anywhere else. The control that creates and destroys the window belongs to
//! the loop, which is neither `Send` nor `Sync` (ADR-0327 §5), so the window
//! is destroyed on the thread that created it.
//!
//! The owner window is also where the exit policy is decided: a request for
//! it (the last tracked window's `WM_DESTROY`, or
//! `Platform::request_exit_policy_reevaluation` on the owner thread) is posted
//! here as one coalesced message (sent from the owner's next turn when the
//! queue refuses it, [`OwnerQueue`]), so the hook always runs on a fresh owner
//! turn rather than nested inside whatever code destroyed the window.
use super::modal_loop::{
    MODAL_FRAME, ModalFrames, ModalLoops, ModalPacing, ModalWake, dispatch_posted_messages,
};
use super::{ClassWindows, FrameCount, WindowIdentity};
use crate::{
    PlatformError, WakeRegistrationError,
    owner::DirectOwnerHooks,
    owner::OpenWindowError,
    owner::OwnerHooks,
    owner::ProxyTransport,
    owner::WindowOpen,
    owner::app_delivery::AppDelivery,
    owner::hwnd_affinity::UserDataRefusal,
    owner::hwnd_affinity::UserDataVerdict,
    owner::hwnd_affinity::classify_user_data_access,
    owner::notices::NoticeChannel,
    owner::panic_boundary::{AfterPanic, contain_os_entry, contain_owner_callback, os_entry},
    owner::signal::OwnerSignal,
    owner::signal::OwnerTurnSlot,
    owner::sources::SystemPreferenceSource,
    platform::Platform,
};
use flui_platform::WindowOptions;
use parking_lot::Mutex;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Arc, OnceLock, Weak},
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
mod queue;
mod wake;
#[cfg(test)]
pub(crate) use queue::testing;
pub(crate) use queue::{OwnerQueue, PostRefusal};
/// Asks the owner to consult the exit policy; see [`ExitPolicyRequest`].
const EXIT_POLICY: u32 = WM_APP + 20;
const OWNER_CLASS: PCWSTR = w!("FLUIOwnerSignalWindow");
static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

/// The platform's owner-thread state, boxed into the owner window's
/// `GWLP_USERDATA` slot. `!Send`: it is created, used and freed on the owner
/// thread only.
pub(crate) struct OwnerControlContext {
    /// Which platform's owner window this context belongs to, so a gate
    /// holding a recycled handle cannot reach another platform's state.
    identity: WindowIdentity,
    signal: Arc<OwnerSignal>,
    app_hooks: Rc<RefCell<super::AppHooks>>,
    turn: Rc<OwnerTurnSlot>,
    frames: Rc<FrameCount>,
    preferences: Rc<dyn OwnerPreferences>,
    /// The hidden top-level window that hears the system's broadcasts and
    /// tells the host's subscribers. Declared after the preference source,
    /// which closes its admission as it drops, so a message the receiver still
    /// hears while it is destroyed wakes nobody.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "held for its lifetime: the receiver window lives as long as the owner's state"
        )
    )]
    broadcasts: super::broadcast::Receiver,
    /// The session, power, lock and memory events the broadcast receiver tells
    /// the lifecycle.
    lifecycle: Rc<dyn OwnerLifecycle>,
    /// The hooks of the host's window class, which each window it opens tells
    /// of its creation.
    window_hooks: super::WindowHooks,
    /// Where system notices go; see [`NoticeChannel`].
    notices: Arc<dyn NoticeChannel>,
    displays: Arc<dyn super::HostDisplays>,
    /// The platform's tracked top-level windows, read by the exit-policy
    /// check. Weak: the owner window does not keep the platform's state alive.
    windows: Weak<dyn ClassWindows>,
    /// Set while an [`EXIT_POLICY`] message is queued; see
    /// [`ExitPolicyRequest`].
    exit_policy_pending: Rc<Cell<bool>>,
    /// The features' hooks on this window (OLE drag and drop, whose sessions
    /// run from it), in the order a message reaches them.
    owner_hooks: Rc<[Arc<dyn OwnerMessages>]>,
    /// The pacing of a modal loop run from this window (OLE's drag loop),
    /// whose clock sends this window [`MODAL_FRAME`].
    modal_pacing: Rc<ModalPacing>,
    /// The modal loops running on the thread, told when this window ran the
    /// platform's code.
    modal_loops: Rc<ModalLoops>,
}

/// One feature's part of the procedure of the owner's message-only window: the
/// messages the feature posts or sends there so that they run on a fresh owner
/// turn, with no window callback in progress, and the window's end.
///
/// The procedure answers its own messages (the wake, the exit policy) and
/// offers any other to its hooks in order (the loop's: the drag-and-drop
/// transport the host makes, [`WindowsHost::drag`](super::WindowsHost::drag)):
/// the first that answers ends the message's dispatch, and a message no hook
/// answers goes to `DefWindowProcW`. A feature's messages are `WM_APP`
/// messages of its own, distinct from every other hook's and the owner's.
pub(crate) trait OwnerMessages {
    /// The owner window exists: a message posted through `owner` reaches the
    /// procedure until the window is closed.
    fn attach(&self, owner: OwnerPost);

    /// Answers `message` for the owner window, or passes it on with `None`.
    /// User code may run here and destroy the window: `owner` holds clones of
    /// what it offers, not a borrow of the owner's state.
    fn message(&self, owner: &OwnerWindow, message: u32) -> Option<LRESULT>;

    /// The owner window is destroyed, and the owner's state with it: what the
    /// feature was to run from the window never runs. Every hook is told, each
    /// apart, so one that panics leaves the next one told.
    fn destroyed(&self);
}

/// Posts a message to the owner window while it exists. `Send + Sync`: the
/// OLE transport that holds one is a `DragTransport`, which `flui-platform`
/// still bounds `Send + Sync` (ADR-0323 §4). It carries only the window's
/// address, read under the lock the owner control clears before it destroys
/// the window.
#[derive(Clone)]
pub(crate) struct OwnerPost {
    address: Arc<Mutex<Option<isize>>>,
}

impl OwnerPost {
    /// Post `message`, with no parameters, to the owner window; `false` once
    /// the window is closed or when the post fails.
    pub(crate) fn post(&self, message: u32) -> bool {
        let address = self.address.lock();
        let Some(raw) = *address else {
            return false;
        };
        // SAFETY: the address is read under the lock the owner control
        // clears before destroying its window, so it names the live owner
        // window; the message carries no pointer.
        unsafe { PostMessageW(Some(HWND(raw as *mut _)), message, WPARAM(0), LPARAM(0)) }.is_ok()
    }

    /// The owner window, while it is open.
    #[cfg(test)]
    pub(crate) fn hwnd(&self) -> Option<HWND> {
        (*self.address.lock()).map(|raw| HWND(raw as *mut _))
    }
}

/// The owner window as an [`OwnerMessages`] hook sees it for one message: its
/// handle, and what the owner delivers on the hook's behalf. It holds the
/// owner's state weakly, so a hook that runs user code keeps none of it alive
/// past the window.
pub(crate) struct OwnerWindow {
    hwnd: HWND,
    pacing: std::rc::Weak<ModalPacing>,
    #[cfg(feature = "single-instance")]
    signal: Weak<OwnerSignal>,
    #[cfg(feature = "single-instance")]
    app: Weak<AppDelivery>,
    #[cfg(feature = "single-instance")]
    windows: Weak<dyn ClassWindows>,
}

impl OwnerWindow {
    /// The owner window's handle.
    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// The delivery to the application's handler, while the owner's state
    /// lives and the platform accepts work.
    #[cfg(feature = "single-instance")]
    pub(crate) fn app(&self) -> Option<Arc<AppDelivery>> {
        self.signal.upgrade().filter(|signal| signal.accepting())?;
        self.app.upgrade()
    }

    /// The platform's windows, while they live.
    #[cfg(feature = "single-instance")]
    pub(crate) fn windows(&self) -> Option<Arc<dyn ClassWindows>> {
        self.windows.upgrade()
    }

    /// The pacing of a modal loop run from the owner window, which a hook
    /// enters while its loop runs; `None` once the owner's state is gone.
    pub(crate) fn modal_pacing(&self) -> Option<Rc<ModalPacing>> {
        self.pacing.upgrade()
    }
}

/// Owner-thread handles cloned out of the [`OwnerControlContext`]. Holding
/// clones instead of a borrow means no reference into the context survives
/// user code, which may tear the owner window down.
pub(crate) struct OwnerShares {
    /// What the application gave the loop (its handler's delivery, the keyboard
    /// layout callback),
    /// shared with every window context this platform opens.
    pub(crate) app_hooks: Rc<RefCell<super::AppHooks>>,
    /// Where the owner-turn callback waits between turns.
    pub(crate) turn: Rc<OwnerTurnSlot>,
    /// Frame callbacks dispatched by every window this platform opens.
    pub(crate) frames: Rc<FrameCount>,
    /// The system preference source the host's table built.
    pub(crate) preferences: Rc<dyn OwnerPreferences>,
    /// The lifecycle the host's table built: the system events the receiver
    /// forwards to the owner.
    pub(crate) lifecycle: Rc<dyn OwnerLifecycle>,
    /// The hooks of the host's window class; a window tells them of its
    /// creation.
    pub(crate) window_hooks: super::WindowHooks,
    /// The broadcast receiver's window, which a test sends messages to.
    #[cfg(test)]
    pub(crate) broadcast_receiver: HWND,
    /// Where the platform hands system notices to the host.
    pub(crate) notices: Arc<dyn NoticeChannel>,
    /// The platform's monitors; a window invalidates them on a DPI change and
    /// names its display through them.
    pub(crate) displays: Arc<dyn super::HostDisplays>,
    /// Posts the owner's exit-policy check; handed to every window context
    /// so the last window's `WM_DESTROY` can ask for it.
    pub(crate) exit_policy: ExitPolicyRequest,
    /// The modal loops running on the thread; a window's pacing joins them.
    pub(crate) modal_loops: Rc<ModalLoops>,
    /// The pacing of a modal loop run from the owner window, which a test
    /// runs a drag session under.
    #[cfg(test)]
    pub(crate) modal_pacing: Rc<ModalPacing>,
}

/// A coalesced request, made on the owner thread, that the owner consult the
/// exit policy: if the platform tracks no top-level window, the
/// application's handler (or, with none bound, the default "exit when the last
/// window closes") decides whether the message loop quits. Requests made while
/// one is already queued cost nothing, and one the owner's queue refuses is
/// decided in the owner's next turn ([`OwnerQueue`]); once the owner window is
/// closed they are refused, like a wake.
///
/// Its two askers, the last window's `WM_DESTROY` and
/// `Platform::request_exit_policy_reevaluation`, are on the owner thread, so
/// the flag is a `Cell`: a request made while the procedure answers the
/// queued one (a handler that asks again from its exit policy) finds the flag
/// already cleared and queues the next.
#[derive(Clone)]
pub(crate) struct ExitPolicyRequest {
    address: Arc<Mutex<Option<isize>>>,
    /// Set while an [`EXIT_POLICY`] message is queued.
    pending: Rc<Cell<bool>>,
    queue: OwnerQueue,
    /// The owner window's identity, which the message names.
    identity: WindowIdentity,
}

impl ExitPolicyRequest {
    pub(crate) fn post(&self) {
        if self.pending.replace(true) {
            return;
        }
        let address = self.address.lock();
        let Some(raw) = *address else {
            return;
        };
        // Read under the lock `OwnerControl::close` takes the address out
        // under before destroying the window, so it names the live owner
        // window; the post never runs the procedure synchronously.
        if self
            .queue
            .post(HWND(raw as *mut _), EXIT_POLICY, self.identity)
            .is_err()
        {
            // No owner window takes it: nothing is queued, and the next
            // request posts again.
            self.pending.set(false);
        }
    }
}

/// The owner window's address, and the owner-thread gate onto its context.
/// Owner-local, like the loop that holds it: the address is shared, under its
/// lock, with the owner signal's cross-thread wake, and reaching the context
/// through the gate still checks the thread, since the slot is a raw pointer.
#[derive(Clone)]
pub(crate) struct OwnerGate {
    address: Arc<Mutex<Option<isize>>>,
    identity: WindowIdentity,
    exit_policy_pending: Rc<Cell<bool>>,
    queue: OwnerQueue,
}

/// The owner's objects a host makes its broadcast subscribers from
/// ([`WindowsHost::broadcast_subscribers`](super::WindowsHost::broadcast_subscribers)).
/// The owner holds a feature's state as the hook its table built, or by name
/// until the feature takes its own.
pub(crate) struct OwnerObjects<'a> {
    /// The system preference source.
    pub(crate) preferences: &'a Rc<dyn OwnerPreferences>,
    /// The session, power, lock and memory events of the lifecycle.
    pub(crate) lifecycle: &'a Rc<dyn OwnerLifecycle>,
    /// The displays the table made ([`WindowsHost::displays`](super::WindowsHost::displays)).
    pub(crate) displays: &'a Arc<dyn super::HostDisplays>,
    /// The owner's signal, which a subscriber wakes when what it heard needs
    /// an owner turn.
    pub(crate) signal: &'a Arc<OwnerSignal>,
}

/// What the owner hands its host's table to build the features' sources it
/// keeps ([`WindowsHost::preferences`](super::WindowsHost::preferences),
/// [`WindowsHost::lifecycle`](super::WindowsHost::lifecycle)).
pub(crate) struct OwnerSeeds<'a> {
    /// Wakes the owner; a source wakes it when what it read changes.
    pub(crate) signal: &'a Arc<OwnerSignal>,
    /// The delivery to the application's handler, which the lifecycle asks at a
    /// session end.
    pub(crate) app: &'a Arc<AppDelivery>,
    /// Where the lifecycle hands system notices.
    pub(crate) notices: &'a Arc<dyn NoticeChannel>,
    /// The door to the thread's modal loops, which the owner's queue knocks
    /// at.
    pub(crate) modal: &'a ModalWake,
}

impl OwnerSeeds<'_> {
    /// The owner thread's queue, for a source that posts to a window of the
    /// thread from another.
    pub(crate) fn queue(&self) -> OwnerQueue {
        OwnerQueue::new(Arc::downgrade(self.signal), self.modal.clone())
    }
}

/// The system preference source the owner keeps for its host (ADR-0172).
///
/// The message loop waits no longer than its retry, a registration for owner
/// turns wakes the owner for a change it has not read, the broadcast receiver
/// tells it of setting and display changes, and the host's preference position
/// samples it. `Any` lets the feature's tests reach their own type again
/// through the owner's shares.
pub(crate) trait OwnerPreferences: SystemPreferenceSource + std::any::Any {
    /// A change waits to be read.
    fn pending(&self) -> bool;

    /// When a read that failed is due again; `None` while none is owed or the
    /// owner no longer accepts work.
    fn retry_deadline(&self) -> Option<web_time::Instant>;

    /// Wake the owner for a retry that is due, and pace the next one.
    fn retry_if_due(&self);

    /// The source's subscriber to the system's broadcasts.
    fn broadcasts(&self) -> Rc<dyn super::broadcast::Subscriber>;

    /// The system's mouse gesture settings at `dpi`, read now and together:
    /// the double-click interval and the mouse geometry (the double-click
    /// rectangle and the drag threshold) a window's input and its gesture
    /// geometry are measured in.
    ///
    /// # Errors
    /// What the system's metrics refuse.
    fn mouse_gestures(
        &self,
        dpi: u32,
    ) -> Result<flui_platform::GesturePreferences, flui_platform::PreferenceQueryError>;
}

/// The lifecycle the owner keeps for its host: the broadcast receiver's
/// subscriber for the session, power, lock and memory messages.
pub(crate) trait OwnerLifecycle: super::broadcast::Subscriber {
    /// Let go of the shutdown block a session-end query left, so the system
    /// carries on (`Platform::release_session_end`).
    fn release_session_end(&self);
}

pub(crate) struct OwnerControl {
    pub(crate) signal: Arc<OwnerSignal>,
    pub(crate) wake: Arc<wake::OwnerWake>,
    gate: OwnerGate,
}
impl OwnerControl {
    /// Build the owner control under `identity`, which the caller reserved
    /// before any native acquisition of its own (see
    /// `WindowsLoop::try_new`), with the broadcast subscribers and window
    /// hooks `H` names and the owner window's `owner_hooks`.
    pub(crate) fn try_new<H: super::WindowsHost>(
        identity: WindowIdentity,
        windows: Weak<dyn ClassWindows>,
        owner_hooks: Vec<Arc<dyn OwnerMessages>>,
        displays: Arc<dyn super::HostDisplays>,
        notices: Arc<dyn NoticeChannel>,
        app: Arc<AppDelivery>,
    ) -> Result<Self, PlatformError> {
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
        let wake = Arc::new(
            wake::OwnerWake::try_new().map_err(|error| PlatformError::Init {
                message: error.to_string(),
            })?,
        );
        let class = super::window_class::<H>().ok_or_else(|| PlatformError::Init {
            message: "FLUI's window class: the thread's storage is torn down".into(),
        })?;
        let (window_hooks, modal_loops) = (class.hooks, class.modal_loops);
        let transport = Arc::clone(&wake);
        let knock = modal_loops.wake().clone();
        let signal = OwnerSignal::new(Arc::new(move || {
            let address = target.lock();
            let raw = address.ok_or_else(|| PlatformError::EventLoop {
                message: "owner message window is closed".into(),
            })?;
            let posted =
                transport
                    .post(HWND(raw as *mut _))
                    .map_err(|error| PlatformError::EventLoop {
                        message: error.to_string(),
                    });
            // The wake is handed over (posted, or set on the kernel event):
            // a drag loop resting in OLE dispatches neither, its pacing does.
            knock.wake();
            posted
        }));
        let exit_policy_pending = Rc::new(Cell::new(false));
        let app_hooks = Rc::new(RefCell::new(super::AppHooks::new(
            Arc::clone(&app),
            Arc::downgrade(&signal),
        )));
        let seeds = OwnerSeeds {
            signal: &signal,
            app: &app,
            notices: &notices,
            modal: modal_loops.wake(),
        };
        let lifecycle = H::lifecycle(&seeds);
        let preferences = H::preferences(&seeds)?;
        let broadcasts = super::broadcast::Receiver::try_new(
            H::broadcast_subscribers(&OwnerObjects {
                preferences: &preferences,
                lifecycle: &lifecycle,
                displays: &displays,
                signal: &signal,
            }),
            Rc::clone(&modal_loops),
        )
        .map_err(|error| PlatformError::Init {
            message: format!("the broadcast receiver: {error}"),
        })?;
        let owner_hooks: Rc<[Arc<dyn OwnerMessages>]> = owner_hooks.into();
        let queue_knock = modal_loops.wake().clone();
        let frames = Rc::new(FrameCount::default());
        let context = Box::new(OwnerControlContext {
            identity,
            signal: Arc::clone(&signal),
            app_hooks: Rc::clone(&app_hooks),
            turn: Rc::new(OwnerTurnSlot::default()),
            frames: Rc::clone(&frames),
            preferences,
            broadcasts,
            lifecycle,
            window_hooks,
            notices,
            displays,
            windows: windows.clone(),
            exit_policy_pending: Rc::clone(&exit_policy_pending),
            owner_hooks: Rc::clone(&owner_hooks),
            modal_pacing: ModalPacing::new(
                ModalFrames::new(app, windows, frames, Arc::downgrade(&signal)),
                Rc::clone(&modal_loops),
            ),
            modal_loops,
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
        for hook in owner_hooks.iter() {
            hook.attach(OwnerPost {
                address: Arc::clone(&address),
            });
        }
        let queue = OwnerQueue::new(Arc::downgrade(&signal), queue_knock);
        Ok(Self {
            signal,
            wake,
            gate: OwnerGate {
                address,
                identity,
                exit_policy_pending,
                queue,
            },
        })
    }

    /// See [`OwnerGate::shares`].
    pub(crate) fn shares(&self, op: &'static str) -> Result<OwnerShares, UserDataRefusal> {
        self.gate.shares(op)
    }

    /// A gate the owner hooks can hold.
    pub(crate) fn gate(&self) -> OwnerGate {
        self.gate.clone()
    }

    pub(crate) fn drive_fallback(&self) {
        contain_owner_callback(|| {
            if let Ok(shares) = self.shares("fallback owner wake")
                && self.signal.drive_in(shares.turn.as_ref())
            {
                quit_owner_loop(&self.signal, shares.turn.as_ref());
            }
        });
    }

    /// See [`ExitPolicyRequest`].
    pub(crate) fn exit_policy_request(&self) -> ExitPolicyRequest {
        self.gate.exit_policy_request()
    }

    /// Stops owner turns and drops the registered turn callback; a closed
    /// owner window has none left to drop.
    pub(crate) fn close_signal(&self) {
        self.signal.close();
        if let Ok(shares) = self.shares("close owner turns") {
            shares.turn.clear();
        }
    }

    /// Stops owner turns and destroys the owner window, whose `WM_NCDESTROY`
    /// frees the owner context. The address is taken out first, under the
    /// lock a cross-thread wake reads it under, so no post reaches the window
    /// once it is being destroyed. A second call finds no address and does
    /// nothing.
    pub(crate) fn close(&self) {
        self.close_signal();
        let address = self.gate.address.lock().take();
        if let Some(address) = address {
            // SAFETY: `DestroyWindow` takes the handle by value. The control
            // belongs to the loop, which is neither `Send` nor `Sync`, so this
            // runs on the thread that created the window, the only one Win32
            // lets destroy it; no lock is held across the call.
            if let Err(error) = unsafe { DestroyWindow(HWND(address as *mut _)) } {
                tracing::error!(%error, "owner message window destruction failed");
            }
        }
    }
}

impl OwnerGate {
    fn exit_policy_request(&self) -> ExitPolicyRequest {
        ExitPolicyRequest {
            address: Arc::clone(&self.address),
            pending: Rc::clone(&self.exit_policy_pending),
            queue: self.queue.clone(),
            identity: self.identity,
        }
    }

    /// Clones the owner-thread handles out of the owner context, or says why
    /// this thread may not have them: [`UserDataRefusal::ForeignThread`] off
    /// the owner, [`UserDataRefusal::WindowGone`] or
    /// [`UserDataRefusal::EmptySlot`] once the platform has shut down,
    /// including when its handle now names another platform's owner window.
    pub(crate) fn shares(&self, op: &'static str) -> Result<OwnerShares, UserDataRefusal> {
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
                    app_hooks: Rc::clone(&context.app_hooks),
                    turn: Rc::clone(&context.turn),
                    frames: Rc::clone(&context.frames),
                    preferences: Rc::clone(&context.preferences),
                    lifecycle: Rc::clone(&context.lifecycle),
                    window_hooks: Rc::clone(&context.window_hooks),
                    #[cfg(test)]
                    broadcast_receiver: context.broadcasts.hwnd_for_test(),
                    notices: Arc::clone(&context.notices),
                    displays: Arc::clone(&context.displays),
                    exit_policy: self.exit_policy_request(),
                    modal_loops: Rc::clone(&context.modal_loops),
                    #[cfg(test)]
                    modal_pacing: Rc::clone(&context.modal_pacing),
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
/// registered into the owner context's [`OwnerTurnSlot`] only while the owner
/// window still holds that context.
pub(crate) struct WindowsOwnerHooks {
    direct: DirectOwnerHooks,
    signal: Arc<OwnerSignal>,
    gate: OwnerGate,
}

impl WindowsOwnerHooks {
    pub(crate) fn new(platform: Rc<dyn Platform>, control: &OwnerControl) -> Self {
        Self {
            // The callback goes to the owner context's slot through the gate
            // below; the direct hooks get a slot of their own, never
            // registered into, so the context's slot dies with the context.
            direct: DirectOwnerHooks::with_signal(
                platform,
                Arc::clone(&control.signal),
                Rc::default(),
            ),
            signal: Arc::clone(&control.signal),
            gate: control.gate(),
        }
    }
}

impl OwnerHooks for WindowsOwnerHooks {
    fn on_wake(&self, callback: Box<dyn FnMut()>) -> Result<(), WakeRegistrationError> {
        // `OwnerPlatform`, the only caller, is `!Send`, so this runs on the
        // owner thread; a refusal here means the owner window is gone.
        match self.gate.shares("on_wake") {
            Ok(shares) => {
                self.signal.register_in(&shares.turn, callback)?;
                if shares.preferences.pending() {
                    let _ = self.signal.wake();
                }
                Ok(())
            }
            Err(reason) => {
                // A subscriber and the callback's captures are user code (ADR-0287).
                contain_owner_callback(|| {
                    tracing::debug!(?reason, "owner-turn registration refused");
                });
                contain_owner_callback(|| drop(callback));
                Err(WakeRegistrationError::OwnerGone)
            }
        }
    }

    fn open_owner_window(
        &self,
        options: WindowOptions,
        handler: std::rc::Rc<dyn flui_platform::WindowHandler>,
    ) -> Result<WindowOpen, OpenWindowError> {
        self.direct.open_owner_window(options, handler)
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
    crate::owner::hwnd_affinity::class_name_matches(&name, copied, expected)
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
/// and the application's handler allowing it. No handler bound allows it.
///
/// The handler is asked through the call point with no `RefCell` borrow held,
/// because it re-enters the embedder, which may open a window or quit. A
/// window opened while it ran vetoes the exit regardless of its answer; a
/// handler that panics leaves the exit to the host (`AppDelivery::exit_policy`).
fn exit_policy_allows_exit(
    signal: &OwnerSignal,
    app: &AppDelivery,
    windows: &Weak<dyn ClassWindows>,
) -> bool {
    let no_windows = || windows.upgrade().is_some_and(|windows| windows.is_empty());
    if !signal.accepting() || !no_windows() {
        return false;
    }
    // A handler with no policy, or one that panicked (contained by the
    // delivery), leaves the exit to the host: a desktop loop ends with its
    // last window.
    app.exit_policy()
        .or_contain(
            crate::platform::ExitAnswer::HostDecides,
            "the application's exit policy",
        )
        .ends_loop(true)
        && signal.accepting()
        && no_windows()
}

// SAFETY: Win32 invokes this exact signature for the dedicated registered class.
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // A wake turn or an exit-policy check that panicked is answered 0, its
    // panic logged with the message.
    os_entry(
        PROCEDURE,
        message,
        AfterPanic::Answer(|_| LRESULT(0)),
        || {
            // SAFETY: Win32's own arguments to this class's procedure, on the
            // owner thread that created the window.
            unsafe { receive(hwnd, message, wparam, lparam) }
        },
    )
}

/// The owner window's procedure as the log names it, and each hook it runs.
const PROCEDURE: &str = "the owner's message window";

/// The owner window's dispatch: a wake turn, the exit policy, its teardown,
/// a drag loop's pacing, then the features' hooks in order until one answers.
/// The hooks and the teardown's steps each run in a containment of their own,
/// logged with their panic's text: a hook that panics answers the message 0.
/// Every message but the pacing's own ran the platform's code, which a modal
/// loop running on the thread asks the deadlines again after.
///
/// # Safety
///
/// `hwnd`, `message`, `wparam` and `lparam` are Win32's arguments to this
/// class's procedure, on the thread that created the window.
unsafe fn receive(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: only this class stores `OwnerControlContext` userdata, and
    // Win32 runs this procedure on the owner thread, the only thread that
    // frees it (`WM_NCDESTROY`, below). A wake turn, the exit policy, the
    // pacing and a feature's hook clone what they use out of the context
    // before running callbacks, so a callback that tears the window down
    // cannot free what they still use.
    unsafe {
        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OwnerControlContext;
        if pointer.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if message == WM_NCDESTROY {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let context = Box::from_raw(pointer);
            let hooks = Rc::clone(&context.owner_hooks);
            // Each step is contained apart: a panic in one leaves the rest to
            // run.
            contain_os_entry(PROCEDURE, || drop(context));
            // What a feature was to run from this window never runs: it is
            // told so here, with nothing of the window left to reach.
            for hook in hooks.iter() {
                contain_os_entry(PROCEDURE, || hook.destroyed());
            }
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if message == MODAL_FRAME {
            // The drag loop's clock fired. OLE's loop, resting, dispatches
            // nothing another thread handed the owner, so this delivery does
            // what the main loop would: the posted messages of FLUI's range,
            // the owner's turn (posted or set on the fallback event), then
            // the pacing's deadlines and paints. The pacing sets its clock
            // again itself; more posted work than one delivery takes asks
            // for the next at once.
            let (signal, turn, pacing, loops) = {
                let context = &*pointer;
                (
                    Arc::clone(&context.signal),
                    Rc::clone(&context.turn),
                    Rc::clone(&context.modal_pacing),
                    Rc::clone(&context.modal_loops),
                )
            };
            if pacing.running() {
                let more = dispatch_posted_messages();
                if signal.drive_in(&turn) {
                    quit_owner_loop(&signal, &turn);
                }
                let _ = pacing.tick();
                if more {
                    loops.wake().wake();
                }
            }
            return LRESULT(0);
        }
        let _ask_again = ModalLoops::ask_again_after(&(*pointer).modal_loops);
        if message == WAKE {
            let signal = Arc::clone(&(*pointer).signal);
            let turn = Rc::clone(&(*pointer).turn);
            if signal.drive_in(&turn) {
                quit_owner_loop(&signal, &turn);
            }
            return LRESULT(0);
        }
        if message == EXIT_POLICY && (*pointer).identity.halves() == (wparam.0, lparam.0) {
            let context = &*pointer;
            context.exit_policy_pending.set(false);
            let signal = Arc::clone(&context.signal);
            let turn = Rc::clone(&context.turn);
            let app = Arc::clone(&context.app_hooks.borrow().app);
            let windows = context.windows.clone();
            if exit_policy_allows_exit(&signal, &app, &windows) {
                quit_owner_loop(&signal, &turn);
            }
            return LRESULT(0);
        }
        // Cloned out first: a hook runs user code (a drag session's modal
        // loop) that may tear this window down.
        let (hooks, owner) = {
            let context = &*pointer;
            let owner = OwnerWindow {
                hwnd,
                pacing: Rc::downgrade(&context.modal_pacing),
                #[cfg(feature = "single-instance")]
                signal: Arc::downgrade(&context.signal),
                #[cfg(feature = "single-instance")]
                app: Arc::downgrade(&context.app_hooks.borrow().app),
                #[cfg(feature = "single-instance")]
                windows: context.windows.clone(),
            };
            (Rc::clone(&context.owner_hooks), owner)
        };
        for hook in hooks.iter() {
            match contain_os_entry(PROCEDURE, || hook.message(&owner, message)) {
                Some(Some(answer)) => return answer,
                Some(None) => {}
                None => return LRESULT(0),
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}
