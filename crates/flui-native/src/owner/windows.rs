//! The Windows host's owner thread: the Win32 message loop and its wake
//! deadlines, the owner's message-only window ([`control`]), the thread's COM
//! apartment ([`com_apartment`]), FLUI's window class with its raw window
//! procedure, and the hidden top-level window that hears the system's
//! broadcasts ([`broadcast`]).
//!
//! The window class and the broadcast receiver serve several features, so they
//! name none of them: the procedure offers each message to the
//! [`WindowMessages`] hooks the host registers, and the receiver tells its
//! [`broadcast::Subscriber`]s what the system broadcast (ADR-0151 §10).

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
mod wake_deadline;

use std::{
    any::TypeId,
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, OnceLock, Weak},
};

use flui_platform::CursorError;
use parking_lot::Mutex;
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
                HCURSOR, HICON, HWND_MESSAGE, IDC_ARROW, IsWindowVisible, LoadCursorW, MSG,
                MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW,
                PostQuitMessage, QS_ALLINPUT, RegisterClassW, SW_SHOWNORMAL, TranslateMessage,
                WINDOW_EX_STYLE, WINDOW_STYLE, WM_QUIT, WNDCLASSW,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::owner::call_point::{Field, Once, SharedField, call, install, retire};
use crate::platforms::windows::platform::WindowMap;
use crate::{
    config::WindowConfiguration, error::PlatformError, executor::BackgroundExecutor, host,
    owner::OwnerHooks, owner::OwnerPlatform, owner::handlers::PlatformHandlers, platform::Platform,
    platform::PlatformExecutor, platform::PlatformReadyCallback,
    shared::hwnd_affinity::UserDataRefusal, shared::system_notice::StaleOnWake,
};
use broadcast::Subscriber;
use com_apartment::ComApartment;
use flui_platform::{DisplayId, DisplaySnapshot};
use wake_deadline::{DeadlineArm, DeadlineDelivery, WakeDeadlineHook};
pub(crate) use wake_deadline::{FrameCount, ModalDelivery};

/// The name of FLUI's window class, which every window the host opens has.
pub(crate) const WINDOW_CLASS_NAME: PCWSTR = w!("FluiWindowClass");

/// Windows' registration of FLUI's window class: the host whose hooks the
/// class's procedure dispatches to, or why the registration failed. Windows
/// holds the class, and the procedure it was registered with, once per
/// process; a second host is refused rather than silently given the first
/// one's hooks.
static WINDOW_CLASS: OnceLock<Result<TypeId, String>> = OnceLock::new();

/// The hooks of FLUI's window class, in the order a message reaches them.
pub(crate) type WindowHooks = &'static [&'static dyn WindowMessages];

/// What a Windows host's table names for its owner thread (ADR-0151 §10): the
/// hooks of FLUI's window class and the subscribers of the broadcast receiver,
/// each in the order a message reaches them, and the displays the loop holds.
/// The owner dispatches through them and names no feature.
pub(crate) trait WindowsHost: 'static {
    /// The hooks of FLUI's window class; its procedure is
    /// `window_proc::<Self>`.
    const WINDOW_HOOKS: WindowHooks;

    /// The broadcast receiver's subscribers, made from the owner's objects.
    fn broadcast_subscribers(owner: &control::OwnerObjects<'_>) -> Vec<Rc<dyn Subscriber>>;

    /// The displays, made once per loop: the loop holds them for the table's
    /// displays position and hands them to every window it opens.
    fn displays() -> Arc<dyn HostDisplays>;
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

    /// The handle of the monitor nearest `hwnd`, `0` for none: what a window
    /// compares to learn that a move took it to another monitor.
    fn nearest_monitor(&self, hwnd: HWND) -> isize;

    /// How many times the monitors were read.
    #[cfg(test)]
    fn sample_count(&self) -> usize;
}

/// One feature's part of the procedure of FLUI's window class.
///
/// The class and its raw procedure are the owner's: the procedure holds the
/// panic boundary and offers each message to the hooks in the order the host
/// names them ([`WindowsHost::WINDOW_HOOKS`]); the first that answers ends the
/// message's dispatch, and a message no hook answers goes to `DefWindowProcW`.
/// A window of the class, once created, is announced to every hook
/// ([`Self::created`]).
pub(crate) trait WindowMessages {
    /// `window` was created and its context installed, on its owner thread.
    fn created(&self, _window: &CreatedWindow) {}

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
    /// Its client size in device pixels.
    pub(crate) client_size: (i32, i32),
    /// The owner thread's COM apartment, when the window was opened with it.
    pub(crate) apartment: Option<Rc<ComApartment>>,
}

/// Announces `window`, which FLUI's class just created, to every one of
/// `hooks`: the host's, which the owner's state hands every window it opens.
pub(crate) fn window_created(hooks: WindowHooks, window: &CreatedWindow) {
    for hook in hooks {
        hook.created(window);
    }
}

/// Offers one message of a window of FLUI's class to `H`'s hooks, in order; a
/// message no hook answers goes to `DefWindowProcW`.
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
    for hook in H::WINDOW_HOOKS {
        // SAFETY: the arguments as Win32 delivered them, per this function's
        // `# Safety` contract.
        if let Some(result) = unsafe { hook.message(hwnd, msg, wparam, lparam) } {
            return result;
        }
    }
    // SAFETY: forwards the message verbatim, as Win32 delivered it.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
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

/// Identity of one native window's context, minted once per
/// [`WindowsWindow::new`](crate::platforms::windows::WindowsWindow::new) and
/// held by both the context and every wrapper clone. Unlike the context's
/// address it is never reused, so a wrapper can tell its own window from one
/// the OS built behind a recycled handle. The owner's message-only window has
/// one too, drawn from the same source.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
                Self::admit_with_config::<tests::TestHost>(WindowConfiguration::default(), &source)
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
    /// OLE drag and drop: the platform's one data-transfer source.
    pub(crate) drag: Arc<crate::platforms::windows::drag::OleTransfer>,
    /// Message-only window for platform messages
    message_window: HWND,

    /// All created windows (keyed by HWND)
    pub(crate) windows: Arc<WindowMap>,

    // The platform-level handlers (callbacks from platform to framework)
    // are not a field: they live in the owner control context, reachable
    // only on the owner thread through `owner_control.shares`.
    /// Background executor for async tasks
    background_executor: Arc<BackgroundExecutor>,

    /// Window configuration (shared across all windows)
    pub(crate) config: WindowConfiguration,

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

// SAFETY, per field: `windows` is an `Arc<Mutex<..>>`, `owner_control` holds
// only an `Arc`-shared signal and a locked address (its owner-thread state,
// callbacks included, sits behind the owner window's `GWLP_USERDATA` and is
// reached only through the owner-thread gate `OwnerControl::shares`), the
// executors are `Arc`-shared and internally synchronized, `displays` is
// `Send + Sync` by its trait's bound, and `config` is plain data. The
// only non-`Sync` member is `message_window: HWND`, a bare address that is
// never dereferenced here.
//
// NOT claimed — an earlier version of this comment claimed both, wrongly: that
// an HWND is "thread-safe by design" (it is thread-AFFINE; its message queue
// belongs to the creating thread, and `DestroyWindow` must run there), and that
// the struct is itself just a handle. Sending the struct is sound because the
// address alone aliases nothing; any Win32 call made through it still owes the
// thread-affinity obligation. The soundness-relevant slice of that obligation
// — dereferencing per-window `GWLP_USERDATA` contexts, and `DestroyWindow` —
// is discharged by the `with_window_context`/`teardown_route` gates of
// `platforms::windows::platform` (see `WindowsWindow`'s `Send`/`Sync` docs);
// the remaining affine calls here
// (`quit`'s `PostQuitMessage`, `open_window`'s queue binding) are logic-level
// rather than memory-safety and stay guarded by `affinity.debug_assert_owner`
// — see ADR-0039 (event-loop affinity). `apartment` is `Send + Sync` by its
// own owner-thread gate, and `drag` keeps its COM objects in an owner slot.
unsafe impl Send for WindowsLoop {}
// SAFETY: as for `Send` — `&WindowsLoop` grants no more than shared access
// to already-synchronized members plus a never-dereferenced address.
unsafe impl Sync for WindowsLoop {}

impl std::fmt::Debug for WindowsLoop {
    // Hand-written: the remaining fields are raw platform handles and callback
    // payloads with no useful Debug form.
    //
    // `try_lock`, never `lock`: `parking_lot::Mutex` is not reentrant and
    // BLOCKS rather than panicking, so formatting this value while the same
    // thread already holds `windows` would deadlock silently — and a Debug
    // impl gets called from assertion messages and `tracing` fields, which is
    // exactly where a lock is likely to be held. Same pattern
    // `parking_lot::Mutex<T>: Debug` itself uses.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("WindowsLoop");
        match self.windows.try_lock() {
            Some(windows) => out.field("windows", &windows.len()),
            None => out.field("windows", &format_args!("<locked>")),
        };
        out.finish_non_exhaustive()
    }
}

impl WindowsLoop {
    /// A loop whose windows follow `config` (hotkeys, debouncing, fullscreen
    /// behavior) and dispatch through the hooks and subscribers `H` names.
    ///
    /// # Errors
    ///
    /// [`PlatformError::Init`] when the process's window class is already
    /// registered for another host, and whatever the native setup refuses.
    pub(crate) fn with_config<H: WindowsHost>(
        config: WindowConfiguration,
    ) -> Result<Self, PlatformError> {
        Self::admit_with_config::<H>(config, WindowIdentity::source())
    }

    /// [`Self::with_config`] with the identity drawn from `source`. The
    /// owner control's identity is reserved first: exhaustion refuses before
    /// COM is initialized or any HWND exists, so a refusal (and every caught
    /// retry) leaves nothing to release.
    fn admit_with_config<H: WindowsHost>(
        config: WindowConfiguration,
        source: &std::sync::atomic::AtomicU64,
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
        let windows = Arc::new(Mutex::new(HashMap::new()));
        let drag = crate::platforms::windows::drag::OleTransfer::new();
        let displays = H::displays();
        let notices = Arc::new(crate::shared::system_notice::SystemNotices::default());
        notices.mark_held_stale_on_wake(Arc::downgrade(&displays) as _);
        let owner_control = control::OwnerControl::new::<H>(
            owner_identity,
            Arc::downgrade(&windows),
            Arc::clone(&drag),
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
            config,
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
    /// the platform's window-event handler) that can panic. A panic must not
    /// unwind across this `extern "system"` frame: unwinding across a non-unwind ABI
    /// boundary is a guaranteed process abort since Rust 1.81, and was
    /// undefined behavior before that (Rust Reference, *Panic* chapter,
    /// "unwinding across FFI boundaries") — and that language-imposed abort
    /// reports nothing through this framework's diagnostics. So the panic
    /// is caught here, its payload logged via `tracing`, and the process
    /// terminated deliberately — the same outcome, made deterministic and
    /// observable on every toolchain. Deliberately NOT
    /// swallowed-and-continued (`DefWindowProcW` as a fallback answer): a
    /// panic mid-`WM_DESTROY` or mid-dispatch leaves framework state torn,
    /// and pumping further messages over torn state converts one crash into
    /// arbitrary later misbehavior — per `docs/PANIC-POLICY.md`, a panic is
    /// a bug report, never control flow.
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
        // `AssertUnwindSafe` is sound here: on `Err` the process aborts
        // before any code can observe whatever state the unwind tore.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: arguments forwarded verbatim from Win32's dispatch of
            // this registered `WNDPROC` — see `# Safety` above.
            unsafe { dispatch_window_message::<H>(hwnd, msg, wparam, lparam) }
        }));
        match outcome {
            Ok(result) => result,
            Err(payload) => {
                tracing::error!(
                    msg,
                    hwnd = ?hwnd,
                    panic = crate::owner::panic_boundary::panic_payload_message(&*payload),
                    "panic in a window-procedure callback; aborting — it must not \
                     unwind across the extern \"system\" boundary, and the framework \
                     state behind this message may be torn"
                );
                std::process::abort();
            }
        }
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
                        Self::redraw_deadline_windows(&self.windows);
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

    /// Ask every window of `windows` for the frame a delivered wake deadline
    /// is due: the main loop's delivery, and the modal drag loop's.
    pub(crate) fn redraw_deadline_windows(windows: &WindowMap) {
        // Traverse a bounded identity snapshot without a per-frame Vec.
        // A minimized window's frame request runs user code, which may open
        // or close windows; each owning Arc is cloned under the registry
        // lock and used only after releasing it.
        let limit = windows
            .lock()
            .values()
            .map(|window| window.identity.0.get())
            .max()
            .unwrap_or(0);
        let mut after = 0;
        loop {
            let next = windows
                .lock()
                .values()
                .filter(|window| {
                    let identity = window.identity.0.get();
                    identity > after && identity <= limit
                })
                .min_by_key(|window| window.identity.0.get())
                .cloned();
            let Some(window) = next else {
                break;
            };
            after = window.identity.0.get();
            let _ = crate::platforms::windows::platform::with_window_context_checked(
                window.hwnd(),
                "deadline redraw",
                |context| {
                    if context.identity != window.identity {
                        return;
                    }
                    // SAFETY: a query of the live HWND this context belongs to,
                    // on its creating thread.
                    let shown = unsafe { IsWindowVisible(window.hwnd()) }.as_bool();
                    if context.mode.get().is_minimized() || !shown {
                        // WM_PAINT skips a minimized window's frame request,
                        // Windows rarely paints one at all, and never paints a
                        // hidden one, so an invalidation would strand the
                        // delivered deadline until unrelated input. Request the
                        // frame directly instead: its frame services timers and
                        // gestures for a window nothing can see.
                        context.request_frame();
                    } else {
                        flui_platform::PlatformWindow::request_redraw(window.as_ref());
                    }
                },
            );
        }
    }

    /// What the main loop would deliver, while `DoDragDrop`'s modal loop
    /// stands in for it and dispatches neither deadlines nor paints: a due
    /// wake deadline is delivered as the main loop delivers it, once per
    /// frame generation, and every window with an invalid region is painted
    /// (`UpdateWindow` sends its `WM_PAINT` directly), so every runtime on
    /// the thread keeps getting frames. Returns how many milliseconds to wait
    /// before the next request. The hook is user code, run with no borrow
    /// held and in the main loop's custody.
    pub(crate) fn deliver_modal_frames(
        handlers: &RefCell<PlatformHandlers>,
        windows: &Weak<WindowMap>,
        frames: &FrameCount,
        delivered: &ModalDelivery,
    ) -> u32 {
        // A replaced hook's final captures may replace the registration
        // again: the query retires its clone before the answer is checked
        // against it.
        let queried = Self::ask_wake_deadline(handlers);
        // The answer of a hook that is no longer installed is not its
        // replacement's: as in the main loop, the delivery record is keyed by
        // the hook as well as the instant.
        let current = queried.and_then(|(deadline, identity)| {
            let installed = handlers
                .borrow()
                .wake_deadline
                .as_ref()
                .is_some_and(|hook| Weak::ptr_eq(&identity, &Arc::downgrade(hook)));
            installed.then_some((deadline?, identity))
        });
        let now = web_time::Instant::now();
        let deadline = current.as_ref().map(|(deadline, _)| *deadline);
        match current {
            Some((deadline, identity))
                if deadline <= now && delivered.admits(deadline, &identity, frames.get()) =>
            {
                if let Some(windows) = windows.upgrade() {
                    delivered.record(deadline, identity, frames.get());
                    Self::redraw_deadline_windows(&windows);
                }
            }
            Some(_) => {}
            None => delivered.clear(),
        }
        // A snapshot: a paint runs user code, which may open or close windows.
        let painted: Vec<HWND> = windows
            .upgrade()
            .map(|windows| {
                windows
                    .lock()
                    .values()
                    .map(|window| window.hwnd())
                    .collect()
            })
            .unwrap_or_default();
        for hwnd in painted {
            // SAFETY: a window this thread created; a handle closed by an
            // earlier paint fails the call harmlessly. Nothing is borrowed.
            let _ = unsafe { windows::Win32::Graphics::Gdi::UpdateWindow(hwnd) };
        }
        crate::shared::ole_drag_drop::modal_frame_wait_ms(deadline, now)
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
                    "platform callback registration refused: platform callbacks are registered \
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

impl host::Input<WindowsLoop> for WindowsLoop {
    fn keyboard_layout(_: &WindowsLoop) -> String {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayoutNameW;
        // SAFETY: `buffer` is a stack-local, zero-initialized `[u16; 9]`
        // sized to `KL_NAMELENGTH`, the exact size this API requires;
        // `&mut buffer` gives it a valid, correctly-sized out-parameter, and
        // it is only read (`is_ok()` branch) after the call reports success.
        unsafe {
            let mut buffer = [0u16; 9]; // KL_NAMELENGTH = 9
            if GetKeyboardLayoutNameW(&mut buffer).is_ok() {
                String::from_utf16_lossy(
                    &buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len())],
                )
            } else {
                String::new()
            }
        }
    }

    fn on_keyboard_layout_change(owner: &WindowsLoop, callback: Box<dyn FnMut() + Send>) {
        owner.register_handler("on_keyboard_layout_change", callback, |handlers| {
            &mut handlers.keyboard_layout_changed
        });
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
