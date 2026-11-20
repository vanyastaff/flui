//! The Windows lane's user-activity guard: the low-level mouse and keyboard hooks and raw input
//! from the touch and pen digitizers, on a thread of its own while armed, each event classified
//! by [`activity::foreign`](super::activity::foreign).
//!
//! A low-level hook procedure receives no context, and a process-global cell to find one would be
//! a trampoline this crate may not hold (ADR-0097). So a hook procedure only posts what it saw to
//! its own thread's queue (the hooks run on the thread that installed them, inside its message
//! loop) and returns; the loop, which owns the state, classifies the posted message. Raw input
//! arrives as `WM_INPUT` on the same queue, at a message-only window of the system's `STATIC`
//! class, and is read before it is dispatched.
#![expect(
    unsafe_code,
    reason = "Win32 hooks, raw input and a message loop; each call takes plain values or pointers \
              to locals"
)]

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, HRAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RID_DEVICE_INFO, RID_HEADER, RIDEV_INPUTSINK, RIDI_DEVICEINFO, RIM_TYPEHID,
    RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, HHOOK,
    HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT,
    PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_INPUT, WM_MOUSEHWHEEL,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT,
};
use windows::core::w;

use crate::error::Result;
use crate::lane::windows::activity::{self, MouseMotion, Observed};
use crate::lane::windows::win32::lane_error;
use crate::lane::{ActivityGuard, ForeignInput};

/// What the mouse hook posts: `WPARAM` the motion and the injected flag, `LPARAM` the extra
/// information.
const SAW_MOUSE: u32 = WM_APP + 1;
/// What the keyboard hook posts, as [`SAW_MOUSE`].
const SAW_KEY: u32 = WM_APP + 2;

/// The bit of a posted `WPARAM` that says the event was injected.
const INJECTED: usize = 0x100;

/// How long arming waits for the hooks to be installed.
const ARM_WITHIN: Duration = Duration::from_secs(5);

/// The HID usage page of digitizers, and its touch screen and pen usages.
const DIGITIZER_PAGE: u16 = 0x0D;
const TOUCH_SCREEN: u16 = 0x04;
const PEN: u16 = 0x02;

/// The guard: see the [module docs](self).
#[derive(Debug, Default)]
pub struct WindowsGuard {
    watching: Option<Watch>,
    reported: bool,
}

#[derive(Debug)]
struct Watch {
    thread_id: u32,
    thread: JoinHandle<()>,
    seen: Receiver<&'static str>,
}

impl WindowsGuard {
    /// A guard that is not watching yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ActivityGuard for WindowsGuard {
    fn arm(&mut self) -> Result<()> {
        self.disarm();
        self.reported = false;
        let (ready, armed) = mpsc::channel::<std::result::Result<u32, String>>();
        let (report, seen) = mpsc::channel::<&'static str>();
        let thread = thread::Builder::new()
            .name("flui-driver-windows-guard".into())
            .spawn(move || watch(&ready, &report))?;
        let thread_id = match armed.recv_timeout(ARM_WITHIN) {
            Ok(Ok(thread_id)) => thread_id,
            Ok(Err(why)) => {
                let _ = thread.join();
                return Err(lane_error(format!(
                    "the user-activity guard could not watch: {why}"
                )));
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                return Err(lane_error(
                    "the user-activity guard did not start watching in time",
                ));
            }
        };
        self.watching = Some(Watch {
            thread_id,
            thread,
            seen,
        });
        Ok(())
    }

    fn foreign_input(&mut self) -> Option<ForeignInput> {
        if self.reported {
            return None;
        }
        let what = self.watching.as_ref()?.seen.try_recv().ok()?;
        self.reported = true;
        Some(ForeignInput::new(what))
    }

    fn disarm(&mut self) {
        if let Some(watch) = self.watching.take() {
            // SAFETY: a thread id and a message with no payload; a thread that already ended
            // makes the post fail, and the join below returns at once.
            let _ = unsafe { PostThreadMessageW(watch.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let _ = watch.thread.join();
        }
    }
}

impl Drop for WindowsGuard {
    fn drop(&mut self) {
        self.disarm();
    }
}

/// Uninstalls the hooks and destroys the raw-input window when the watching thread ends.
struct Installed {
    hooks: Vec<HHOOK>,
    window: Option<HWND>,
}

impl Drop for Installed {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            // SAFETY: a hook this thread installed, removed once.
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
        if let Some(window) = self.window.take() {
            // SAFETY: the window this thread created, destroyed once on its own thread.
            let _ = unsafe { DestroyWindow(window) };
        }
    }
}

/// The watching thread: install, report ready, then classify until `WM_QUIT`.
fn watch(
    ready: &mpsc::Sender<std::result::Result<u32, String>>,
    report: &mpsc::Sender<&'static str>,
) {
    let mut message = MSG::default();
    // SAFETY: `message` is a local; the call makes the thread's queue exist before its id is
    // handed out, so a quit posted right after arming is not lost.
    let _ = unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_NOREMOVE) };
    let installed = match install() {
        Ok(installed) => installed,
        Err(why) => {
            let _ = ready.send(Err(why));
            return;
        }
    };
    // SAFETY: no arguments.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    let mut sent = false;
    loop {
        // SAFETY: `message` is a local the call writes; any window of this thread.
        let got = unsafe { GetMessageW(&raw mut message, None, 0, 0) };
        if got.0 == 0 || got.0 == -1 || message.message == WM_QUIT {
            break;
        }
        let observed = match message.message {
            SAW_MOUSE => Some(Observed::Mouse {
                motion: match message.wParam.0 & 0xFF {
                    0 => MouseMotion::Move,
                    1 => MouseMotion::Button,
                    _ => MouseMotion::Wheel,
                },
                injected: message.wParam.0 & INJECTED != 0,
                extra: message.lParam.0.cast_unsigned(),
            }),
            SAW_KEY => Some(Observed::Key {
                injected: message.wParam.0 & INJECTED != 0,
                extra: message.lParam.0.cast_unsigned(),
            }),
            WM_INPUT => digitizer(message.lParam),
            _ => None,
        };
        if message.message == WM_INPUT {
            // The raw input's cleanup is the window procedure's (`DefWindowProc`).
            // SAFETY: a message this thread's queue returned.
            unsafe { DispatchMessageW(&raw const message) };
        }
        if !sent && let Some(what) = observed.and_then(activity::foreign) {
            sent = report.send(what).is_ok();
        }
    }
    drop(installed);
}

fn install() -> std::result::Result<Installed, String> {
    let mut installed = Installed {
        hooks: Vec::new(),
        window: None,
    };
    // SAFETY: the module of this process, which outlives the hooks.
    let module = unsafe { GetModuleHandleW(None) }
        .map_err(|error| format!("this process's module is unreadable: {error}"))?;
    for (hook, procedure) in [
        (
            WH_MOUSE_LL,
            mouse_hook as unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT,
        ),
        (WH_KEYBOARD_LL, key_hook),
    ] {
        // SAFETY: a low-level hook procedure of this module; low-level hooks run on this thread,
        // inside its message loop, until `Installed` removes them.
        let handle = unsafe { SetWindowsHookExW(hook, Some(procedure), Some(module.into()), 0) }
            .map_err(|error| format!("a low-level hook could not be installed: {error}"))?;
        installed.hooks.push(handle);
    }
    // SAFETY: a message-only window of a system class, with no creation data; destroyed by
    // `Installed` on this thread.
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("flui-driver guard"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(|error| format!("the raw-input window could not be created: {error}"))?;
    installed.window = Some(window);
    let devices = [TOUCH_SCREEN, PEN].map(|usage| RAWINPUTDEVICE {
        usUsagePage: DIGITIZER_PAGE,
        usUsage: usage,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: window,
    });
    // SAFETY: two fully initialized registrations and the size of one. A desktop without a
    // digitizer still takes the registration.
    unsafe {
        RegisterRawInputDevices(
            &devices,
            u32::try_from(size_of::<RAWINPUTDEVICE>()).expect("BUG: a registration is 16 bytes"),
        )
    }
    .map_err(|error| format!("raw input from digitizers could not be registered: {error}"))?;
    Ok(installed)
}

/// The low-level mouse hook: post what moved, whether it was injected and its extra information
/// to this thread's own queue, and pass the event on.
unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for a non-negative code, `lparam` points at the event's `MSLLHOOKSTRUCT`,
        // valid for the call.
        let event = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let motion: usize = match u32::try_from(wparam.0).unwrap_or(0) {
            WM_MOUSEMOVE => 0,
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => 2,
            _ => 1,
        };
        let injected = if event.flags & LLMHF_INJECTED != 0 {
            INJECTED
        } else {
            0
        };
        // SAFETY: plain values to this thread's own queue.
        let _ = unsafe {
            PostThreadMessageW(
                GetCurrentThreadId(),
                SAW_MOUSE,
                WPARAM(motion | injected),
                LPARAM(event.dwExtraInfo.cast_signed()),
            )
        };
    }
    // SAFETY: the arguments this hook was called with.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The low-level keyboard hook, as [`mouse_hook`].
unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: for a non-negative code, `lparam` points at the event's `KBDLLHOOKSTRUCT`,
        // valid for the call.
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let injected = if event.flags.0 & LLKHF_INJECTED.0 != 0 {
            INJECTED
        } else {
            0
        };
        // SAFETY: plain values to this thread's own queue.
        let _ = unsafe {
            PostThreadMessageW(
                GetCurrentThreadId(),
                SAW_KEY,
                WPARAM(injected),
                LPARAM(event.dwExtraInfo.cast_signed()),
            )
        };
    }
    // SAFETY: the arguments this hook was called with.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The digitizer event a `WM_INPUT` carries: whether a device sent it, and whether a pen did.
fn digitizer(lparam: LPARAM) -> Option<Observed> {
    let mut header = RAWINPUTHEADER::default();
    let mut size = u32::try_from(size_of::<RAWINPUTHEADER>()).expect("BUG: a header is small");
    // SAFETY: the handle `WM_INPUT` carries, read once before dispatch; `header` is a local of
    // the size passed.
    let read = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as *mut core::ffi::c_void),
            RID_HEADER,
            Some((&raw mut header).cast()),
            &raw mut size,
            size,
        )
    };
    if read == u32::MAX || header.dwType != RIM_TYPEHID.0 {
        return None;
    }
    let from_device = !header.hDevice.is_invalid();
    Some(Observed::Digitizer {
        pen: from_device && usage(header.hDevice) == Some(PEN),
        from_device,
    })
}

/// The HID usage of the device `device`.
fn usage(device: HANDLE) -> Option<u16> {
    let mut info = RID_DEVICE_INFO {
        cbSize: u32::try_from(size_of::<RID_DEVICE_INFO>()).expect("BUG: the info is small"),
        ..RID_DEVICE_INFO::default()
    };
    let mut size = info.cbSize;
    // SAFETY: a device handle raw input named; `info` is a local of the size passed, its
    // `cbSize` set as the call requires.
    let read = unsafe {
        GetRawInputDeviceInfoW(
            Some(device),
            RIDI_DEVICEINFO,
            Some((&raw mut info).cast()),
            &raw mut size,
        )
    };
    if read == u32::MAX || info.dwType != RIM_TYPEHID {
        return None;
    }
    // SAFETY: a HID device's info, as `dwType` says, fills the union's `hid` member.
    Some(unsafe { info.Anonymous.hid }.usUsage)
}
