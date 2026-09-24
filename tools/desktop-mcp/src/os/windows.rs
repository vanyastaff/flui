//! Win32 calls the portable crates do not cover: DPI awareness, the
//! foreground window and what lies under a point, window and process
//! identity, absolute pointer moves across the virtual desktop, keyboard
//! layout lookups, and the job object that ties launched children to this
//! process.
#![expect(
    unsafe_code,
    reason = "Win32 FFI; each call takes plain values or pointers to locals"
)]

use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Ole::{
    SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_PER_MONITOR_AWARE,
    GetAwarenessFromDpiAwarenessContext, GetThreadDpiAwarenessContext,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MOUSEEVENTF_MOVE, MOUSEINPUT,
    MapVirtualKeyExW, SendInput, ToUnicodeEx, VIRTUAL_KEY, VK_CAPITAL, VK_CONTROL, VK_MENU,
    VK_SHIFT, VkKeyScanExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GUITHREADINFO, GetAncestor, GetClassNameW, GetCursorPos,
    GetForegroundWindow, GetGUIThreadInfo, GetSystemMetrics, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, SM_SWAPBUTTON, SW_RESTORE,
    SetCursorPos, SetForegroundWindow, ShowWindow, WindowFromPoint,
};
use windows::core::{BOOL, Interface};

use super::Focus;

use crate::error::{ToolError, ToolResult};

/// Makes every coordinate this process sees physical pixels: UIA rects,
/// xcap captures and injected input then share one space on every monitor.
pub fn init_dpi() -> ToolResult<()> {
    // SAFETY: a predefined context constant; the call only sets process state.
    let set = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // A refusal is harmless when the process is already per-monitor aware
    // (a manifest or a host set it first); anything else is not.
    // SAFETY: reads this thread's awareness context; no preconditions.
    let awareness = unsafe { GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext()) };
    if awareness == DPI_AWARENESS_PER_MONITOR_AWARE {
        return Ok(());
    }
    set.map_err(|e| ToolError::platform("setting per-monitor DPI awareness", e))
}

fn hwnd(id: u32) -> HWND {
    HWND(id as usize as *mut core::ffi::c_void)
}

fn id_and_pid(h: HWND) -> Option<(u32, u32)> {
    if h.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `h` is a window handle; `pid` is a local the call writes.
    unsafe { GetWindowThreadProcessId(h, Some(&raw mut pid)) };
    Some((h.0 as usize as u32, pid))
}

/// The foreground window's id (its `HWND`) and owning process.
pub fn foreground() -> Option<(u32, u32)> {
    // SAFETY: no arguments.
    id_and_pid(unsafe { GetForegroundWindow() })
}

/// The top-level window that would receive a click at the point with its
/// process, and the process of the deepest window there, which can differ:
/// a top-level window can host another process's child window (a preview
/// pane, an embedded browser), and that child is what takes the click.
pub fn window_at(x: i32, y: i32) -> Option<super::Under> {
    // SAFETY: plain value arguments.
    let deepest = unsafe { WindowFromPoint(POINT { x, y }) };
    // SAFETY: `GetAncestor` accepts any handle.
    let (id, pid) = id_and_pid(unsafe { GetAncestor(deepest, GA_ROOT) })?;
    let (_, inner_pid) = id_and_pid(deepest)?;
    Some(super::Under { id, pid, inner_pid })
}

/// Where keyboard input goes inside foreground window `fg` (the one the
/// caller just checked): the process of the window holding keyboard focus
/// in its thread. A child window of another process (an embedded browser, a
/// preview pane) can hold it inside a top-level window of the target. An
/// error when the foreground is no longer `fg`, so the answer is never about
/// a window nobody checked.
pub fn focus(fg: u32) -> ToolResult<Focus> {
    // SAFETY: no arguments.
    let now = unsafe { GetForegroundWindow() };
    if id_and_pid(now).map(|(id, _)| id) != Some(fg) {
        return Err(ToolError::NotForeground {
            target: format!("window {fg}"),
            foreground: "another window took the foreground during the check".into(),
        });
    }
    // SAFETY: a window handle; a null pid pointer is allowed.
    let thread = unsafe { GetWindowThreadProcessId(now, None) };
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..GUITHREADINFO::default()
    };
    // SAFETY: `info` is a local with its size set, as the call requires.
    unsafe { GetGUIThreadInfo(thread, &raw mut info) }
        .map_err(|e| ToolError::platform("reading which window has keyboard focus", e))?;
    // Read again: a window that took the foreground during the lookup would
    // get the input, while the focus read describes the old one.
    // SAFETY: no arguments.
    if unsafe { GetForegroundWindow() } != now {
        return Err(ToolError::NotForeground {
            target: format!("window {fg}"),
            foreground: "another window took the foreground during the check".into(),
        });
    }
    // No focus window: keys reach the foreground window itself.
    Ok(id_and_pid(info.hwndFocus).map_or(Focus::Foreground, |(_, pid)| Focus::Pid(pid)))
}

/// How long UI Automation waits for a provider to answer one call, and to
/// finish connecting to one: a hung application fails the call instead of
/// holding the one desktop thread every tool shares.
const UIA_TRANSACTION_TIMEOUT_MS: u32 = 5_000;
const UIA_CONNECTION_TIMEOUT_MS: u32 = 2_000;

/// A UI Automation client with call timeouts (`CUIAutomation8`, through
/// `IUIAutomation2`), for a thread already in COM. `None` where that object
/// is unavailable; the caller then keeps the one without timeouts.
pub fn uia_with_timeouts() -> Option<IUIAutomation> {
    // SAFETY: the calling thread has initialized COM (the caller created a
    // UI Automation client on it first); no outer object.
    let automation: IUIAutomation2 =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.ok()?;
    // SAFETY: plain value arguments on a live interface.
    unsafe {
        automation
            .SetTransactionTimeout(UIA_TRANSACTION_TIMEOUT_MS)
            .ok()?;
        automation
            .SetConnectionTimeout(UIA_CONNECTION_TIMEOUT_MS)
            .ok()?;
    }
    automation.cast().ok()
}

/// The longest runtime id read: UI Automation's own are a handful of
/// integers; a provider claiming more is not trusted with an allocation.
const MAX_RUNTIME_ID_LEN: i64 = 64;

/// `element`'s runtime id, read live, with the array freed (the
/// `uiautomation` crate's reader leaks every array it reads). `Ok(None)` for
/// an element without one.
pub fn runtime_id(
    element: &IUIAutomationElement,
) -> Result<Option<Vec<i32>>, windows::core::Error> {
    // SAFETY: a live interface; the returned array is ours to destroy.
    let array = unsafe { element.GetRuntimeId() }?;
    if array.is_null() {
        return Ok(None);
    }
    // SAFETY: `array` is the one-dimensional `VT_I4` array `GetRuntimeId`
    // returns, read by index within its bounds, then destroyed exactly once.
    let ids = unsafe {
        let read = || -> Result<Vec<i32>, windows::core::Error> {
            // The bounds come from the provider: anything but a short
            // one-dimensional array is refused before a single element is
            // read, so a hostile provider cannot make the read allocate or
            // loop without bound.
            let (low, high) = (SafeArrayGetLBound(array, 1)?, SafeArrayGetUBound(array, 1)?);
            let len = i64::from(high) - i64::from(low) + 1;
            if SafeArrayGetDim(array) != 1 || !(0..=MAX_RUNTIME_ID_LEN).contains(&len) {
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_INVALIDARG,
                ));
            }
            let mut ids = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
            for index in low..=high {
                let mut value = 0_i32;
                SafeArrayGetElement(array, &raw const index, (&raw mut value).cast())?;
                ids.push(value);
            }
            Ok(ids)
        };
        let ids = read();
        let _ = SafeArrayDestroy(array);
        ids?
    };
    Ok((!ids.is_empty()).then_some(ids))
}

/// Whether the primary and secondary mouse buttons are swapped: injected
/// button events are physical, so a logical left click is then a physical
/// right one.
pub fn buttons_swapped() -> bool {
    // SAFETY: a plain metric index.
    unsafe { GetSystemMetrics(SM_SWAPBUTTON) != 0 }
}

/// Releases a Unicode unit a partial send left down; whether it went out.
pub fn release_unicode(unit: u16) -> bool {
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: one fully initialized `INPUT` and the size of one.
    unsafe { SendInput(&[up], size_of::<INPUT>() as i32) == 1 }
}

/// Types `c` as Unicode input, each UTF-16 unit pressed and released with
/// itself (enigo releases a surrogate pair's low unit with the high one).
/// On failure, also the unit left down, if a partial send left one whose
/// release did not go through: the caller keeps it to release later.
pub fn send_unicode(c: char) -> Result<(), (ToolError, Option<u16>)> {
    let mut units = [0_u16; 2];
    let key = |unit: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: if up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs: Vec<INPUT> = c
        .encode_utf16(&mut units)
        .iter()
        .flat_map(|&unit| [key(unit, false), key(unit, true)])
        .collect();
    // SAFETY: fully initialized `INPUT` values and the size of one.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) } as usize;
    if sent == inputs.len() {
        return Ok(());
    }
    let blocked = ToolError::platform(
        "typing a character",
        "SendInput was blocked (UIPI: the target may run elevated)",
    );
    if sent == 0 {
        return Err((blocked, None));
    }
    // Part of it went in. The events alternate down and up per unit, so an
    // odd count left a unit down: release it, and say the character may
    // have arrived in part.
    let mut stuck = None;
    if sent % 2 == 1 {
        // SAFETY: the down event just sent is the one before `sent`.
        let unit = unsafe { inputs[sent - 1].Anonymous.ki.wScan };
        let released = (0..3).any(|attempt| {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            release_unicode(unit)
        });
        if !released {
            stuck = Some(unit);
        }
    }
    let released = stuck.is_none();
    Err((
        ToolError::Interrupted {
            cause: Box::new(blocked),
            what: if released {
                format!(
                    "part of `{c}` was typed before the rest was blocked; check the text before retrying"
                )
            } else {
                format!(
                    "part of `{c}` was typed, and releasing its last unit failed, so a key may still be held; check the text before retrying"
                )
            },
        },
        stuck,
    ))
}

/// The top-level window `id` belongs to (itself when it is one), by the
/// same definition [`window_at`] uses.
pub fn root_window(id: u32) -> Option<u32> {
    // SAFETY: `GetAncestor` accepts any handle.
    id_and_pid(unsafe { GetAncestor(hwnd(id), GA_ROOT) }).map(|(root, _)| root)
}

/// The process that owns window `id`.
pub fn window_pid(id: u32) -> Option<u32> {
    let h = hwnd(id);
    // SAFETY: `IsWindow` accepts any value and validates it.
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        return None;
    }
    id_and_pid(h).map(|(_, pid)| pid)
}

/// Window `id`'s bounds as the window list reports them (the visible frame,
/// without the invisible resize border), else its window rect.
pub fn window_rect(id: u32) -> Option<crate::geometry::Rect> {
    let h = hwnd(id);
    let mut r = RECT::default();
    // SAFETY: `r` is a local RECT and the size passed is its size.
    let framed = unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut r).cast(),
            size_of::<RECT>() as u32,
        )
    };
    // SAFETY: `r` is a local the call writes.
    if framed.is_err() && unsafe { GetWindowRect(h, &raw mut r) }.is_err() {
        return None;
    }
    Some(crate::geometry::Rect::from_ltrb(
        r.left, r.top, r.right, r.bottom,
    ))
}

/// A fingerprint of window `id`'s class name. An `HWND` value names a new
/// window only once the slot's reuse counter wraps, and then usually one of
/// another class; with the owner's process identity this tells a window from
/// its replacement without a per-window creation time, which Windows keeps
/// none of.
pub fn window_class(id: u32) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let mut buffer = [0_u16; 256];
    // SAFETY: the buffer is a local slice the call writes at most its length of.
    let len = unsafe { GetClassNameW(hwnd(id), &mut buffer) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    buffer[..len].hash(&mut hasher);
    Some(hasher.finish())
}

/// Window `id`'s title.
pub fn window_title(id: u32) -> Option<String> {
    let mut buffer = [0_u16; 256];
    // SAFETY: the buffer is a local slice the call writes at most its length of.
    let len = unsafe { GetWindowTextW(hwnd(id), &mut buffer) };
    let len = usize::try_from(len).ok()?;
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// When process `pid` started, as a `FILETIME` count: with the pid, an
/// identity Windows does not recycle.
pub fn process_started(pid: u32) -> Option<u64> {
    // SAFETY: plain value arguments; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: an open process handle and four locals the call writes.
    let times = unsafe {
        GetProcessTimes(
            process,
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    };
    // SAFETY: the handle opened above, closed once.
    let _ = unsafe { CloseHandle(process) };
    times.ok()?;
    Some(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
}

/// Process `pid`'s visible top-level windows, front to back — its popup
/// menus, drop-downs and tooltips included, which the capture window list
/// leaves out.
pub fn process_windows(pid: u32) -> ToolResult<Vec<u32>> {
    struct Search {
        pid: u32,
        found: Vec<u32>,
    }
    unsafe extern "system" fn visit(h: HWND, state: LPARAM) -> BOOL {
        // SAFETY: `state` is the `Search` `process_windows` passes, alive and
        // borrowed by nothing else for the whole enumeration.
        let search = unsafe { &mut *(state.0 as *mut Search) };
        // SAFETY: `h` is a window the enumeration hands over.
        if unsafe { IsWindowVisible(h) }.as_bool()
            && let Some((id, pid)) = id_and_pid(h)
            && pid == search.pid
        {
            search.found.push(id);
        }
        true.into()
    }
    let mut search = Search {
        pid,
        found: Vec::new(),
    };
    // SAFETY: `visit` matches `WNDENUMPROC`; the pointer is to a local that
    // outlives the call, which returns only when the enumeration is done.
    // The callback never stops the enumeration, so a failure is the API's
    // own, possibly after only some windows: not a complete list.
    unsafe { EnumWindows(Some(visit), LPARAM((&raw mut search) as isize)) }
        .map_err(|e| ToolError::platform("enumerating top-level windows", e))?;
    Ok(search.found)
}

/// The virtual key that types `c` on the foreground window's keyboard
/// layout, and the modifiers it needs (bit 1 Shift, 2 Ctrl, 4 Alt), as
/// `VkKeyScanExW` reports them. `None` when no key on that layout types it
/// as a plain key press: none does, it needs a state this server cannot
/// hold (Kana, Hankaku), or it is a dead key, which types nothing itself
/// and changes the key after it.
pub fn char_key(c: char, command: bool) -> Option<(u16, u8)> {
    let mut units = [0_u16; 2];
    let [unit] = c.encode_utf16(&mut units) else {
        return None;
    };
    // Layouts are per thread: the one that counts is the thread of the
    // window holding keyboard focus (an editor thread can use another layout
    // than its top-level window's), else the foreground window's.
    // SAFETY: a null window yields thread 0, whose layout is the caller's.
    let foreground_thread = unsafe { GetWindowThreadProcessId(GetForegroundWindow(), None) };
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..GUITHREADINFO::default()
    };
    // SAFETY: `info` is a local with its size set, as the call requires.
    let read = unsafe { GetGUIThreadInfo(foreground_thread, &raw mut info) }.is_ok();
    let focus_thread = if read && !info.hwndFocus.is_invalid() {
        // SAFETY: a window handle the call just returned.
        unsafe { GetWindowThreadProcessId(info.hwndFocus, None) }
    } else {
        foreground_thread
    };
    // SAFETY: plain value argument.
    let layout = unsafe { GetKeyboardLayout(focus_thread) };
    // SAFETY: plain value arguments.
    let scan = unsafe { VkKeyScanExW(*unit, layout) };
    let [vk, shift] = scan.to_le_bytes();
    if scan == -1 || shift & !0b111 != 0 {
        return None;
    }
    // Dead under the modifiers it needs, not bare: `^` is Shift+6 on US
    // International, and only the shifted 6 is dead. `ToUnicodeEx` with flag
    // 4 translates without touching the keyboard state it would otherwise
    // leave a pending dead key in; a negative result is a dead key.
    // The live Caps Lock toggle is part of the state the key meets: with it
    // on, a letter needs the opposite Shift from what `VkKeyScanExW` says.
    // SAFETY: plain value argument.
    // Not for a shortcut: there the physical key counts, whatever Caps
    // Lock would make it type.
    let caps = !command && unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 != 0;
    // SAFETY: plain value arguments.
    let scan = unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC, Some(layout)) };
    let types = |shift: u8| {
        let mut state = [0_u8; 256];
        for (bit, vk_mod) in [(1, VK_SHIFT), (2, VK_CONTROL), (4, VK_MENU)] {
            if shift & bit != 0 {
                state[usize::from(vk_mod.0)] = 0x80;
            }
        }
        if caps {
            state[usize::from(VK_CAPITAL.0)] = 0x01;
        }
        let mut out = [0_u16; 8];
        // SAFETY: plain values, a local key-state table and a local buffer.
        let typed = unsafe { ToUnicodeEx(u32::from(vk), scan, &state, &mut out, 4, Some(layout)) };
        // One unit, and the one asked for: a dead key (negative) or another
        // character is not this key.
        typed == 1 && out[0] == *unit
    };
    if types(shift) {
        Some((u16::from(vk), shift))
    } else if types(shift ^ 1) {
        Some((u16::from(vk), shift ^ 1))
    } else {
        None
    }
}

/// Restores `id` if minimized and asks Windows to put it in front. Windows
/// may refuse (foreground lock); the caller checks [`foreground`] afterwards.
pub fn bring_to_front(id: u32) -> ToolResult<()> {
    let h = hwnd(id);
    // SAFETY: `IsWindow` accepts any value and validates it.
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        return Err(ToolError::NotFound(format!("window {id} no longer exists")));
    }
    // SAFETY: `h` was just validated as a window handle.
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
    Ok(())
}

/// Moves the pointer to a physical screen point anywhere on the virtual
/// desktop. enigo's absolute move normalizes against the primary monitor
/// only (points on other monitors are clamped), and the normalized
/// 0..=65535 space of an absolute `SendInput` rounds by a few pixels on a
/// large virtual desktop. `SetCursorPos` is exact in physical pixels under
/// per-monitor DPI awareness; a zero relative `SendInput` move after it makes
/// the target see a genuine mouse-move event at the new position.
pub fn move_pointer(x: i32, y: i32) -> ToolResult<()> {
    // SAFETY: plain value arguments.
    unsafe { SetCursorPos(x, y) }
        .map_err(|e| ToolError::platform(format!("moving the pointer to ({x}, {y})"), e))?;
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: one fully initialized INPUT and its exact size.
    let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
    // The pointer has moved either way (`SetCursorPos` succeeded): a blocked
    // move event only means the window under it (an elevated one, UIPI)
    // saw no mouse-move message, and the caller's checks go on from where
    // the pointer really is.
    if sent != 1 {
        tracing::debug!("the mouse-move event after a pointer move was blocked (UIPI)");
    }
    Ok(())
}

/// The pointer's current physical position.
pub fn cursor() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    // SAFETY: `p` is a local the call writes.
    unsafe { GetCursorPos(&raw mut p) }.ok()?;
    Some((p.x, p.y))
}

/// A job object that kills every assigned process when this process exits,
/// however it exits.
#[derive(Debug)]
pub struct KillOnExitJob {
    handle: HANDLE,
    /// Whether dropping this closes the handle. Not once this process is in
    /// the job itself: closing the last handle kills every process in the
    /// job, this one included, so the handle then lives exactly as long as
    /// the process and the OS closes it at exit.
    closes: bool,
}

// SAFETY: a job handle is a kernel object reference, usable from any thread.
unsafe impl Send for KillOnExitJob {}
// SAFETY: as above; the job APIs used here are thread-safe.
unsafe impl Sync for KillOnExitJob {}

impl KillOnExitJob {
    /// Creates the job with `KILL_ON_JOB_CLOSE`.
    pub fn new() -> ToolResult<Self> {
        // SAFETY: anonymous job, default security.
        let job = unsafe { CreateJobObjectW(None, None) }
            .map_err(|e| ToolError::platform("creating the child job object", e))?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` is the struct this information class expects, with
        // its exact size.
        let set = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if let Err(e) = set {
            // SAFETY: `job` is the handle created above, closed once.
            let _ = unsafe { CloseHandle(job) };
            return Err(ToolError::platform("configuring the child job object", e));
        }
        Ok(Self {
            handle: job,
            closes: true,
        })
    }

    /// Puts this process itself in the job. Every process it starts from
    /// then on is in the job from its creation, and so is everything those
    /// start: nothing can slip out between a child's spawn and its
    /// assignment. The job ends them all when this process's handle to it
    /// closes, which from then on is only at this process's exit.
    pub fn assign_self(&mut self) -> ToolResult<()> {
        // SAFETY: the job is owned by `self`; the pseudo handle needs no close.
        unsafe { AssignProcessToJobObject(self.handle, GetCurrentProcess()) }
            .map_err(|e| ToolError::platform("adding the server to its child job object", e))?;
        self.closes = false;
        Ok(())
    }

    /// Ties `child` to the job.
    ///
    /// A child of a server that joined the job ([`Self::assign_self`]) is in
    /// it from its creation; that counts as assigned whatever the explicit
    /// assignment answers.
    pub fn assign(&self, child: &std::process::Child) -> ToolResult<()> {
        let process = HANDLE(child.as_raw_handle());
        // SAFETY: both handles are open for the duration of the call: the job
        // is owned by `self`, the process by `child`.
        let assigned = unsafe { AssignProcessToJobObject(self.handle, process) };
        if assigned.is_ok() {
            return Ok(());
        }
        let mut inside = BOOL::default();
        // SAFETY: as above; `inside` is a local the call writes.
        let checked = unsafe { IsProcessInJob(process, Some(self.handle), &raw mut inside) };
        if checked.is_ok() && inside.as_bool() {
            return Ok(());
        }
        assigned.map_err(|e| ToolError::platform("adding the child to the job object", e))
    }
}

impl Drop for KillOnExitJob {
    fn drop(&mut self) {
        if self.closes {
            // SAFETY: the handle is owned by `self` and closed exactly once.
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}
