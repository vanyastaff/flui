//! Win32 calls the portable crates do not cover: DPI awareness, the
//! foreground window, absolute pointer moves across the virtual desktop, and
//! the job object that ties launched children to this process.
#![expect(
    unsafe_code,
    reason = "Win32 FFI; each call takes plain values or pointers to locals"
)]

use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, POINT};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    IsWindow, SW_RESTORE, SetCursorPos, SetForegroundWindow, ShowWindow, WindowFromPoint,
};

use crate::error::{ToolError, ToolResult};

/// Makes every coordinate this process sees physical pixels: UIA rects,
/// xcap captures and injected input then share one space on every monitor.
pub fn init_dpi() -> ToolResult<()> {
    // SAFETY: a predefined context constant; the call only sets process state.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        .map_err(|e| ToolError::platform("setting per-monitor DPI awareness", e))
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

/// The top-level window that would receive a click at the point, and its
/// process.
pub fn window_at(x: i32, y: i32) -> Option<(u32, u32)> {
    // SAFETY: plain value arguments; `GetAncestor` accepts any handle.
    let root = unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) };
    id_and_pid(root)
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
    if sent == 1 {
        Ok(())
    } else {
        Err(ToolError::platform(
            "moving the pointer",
            "SendInput was blocked (UIPI: the target may run elevated)",
        ))
    }
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
pub struct KillOnExitJob(HANDLE);

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
        Ok(Self(job))
    }

    /// Ties `child` to the job.
    pub fn assign(&self, child: &std::process::Child) -> ToolResult<()> {
        let process = HANDLE(child.as_raw_handle());
        // SAFETY: both handles are open for the duration of the call: the job
        // is owned by `self`, the process by `child`.
        unsafe { AssignProcessToJobObject(self.0, process) }
            .map_err(|e| ToolError::platform("adding the child to the job object", e))
    }
}

impl Drop for KillOnExitJob {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by `self` and closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}
