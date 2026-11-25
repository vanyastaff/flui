//! The Win32 calls the Windows lane makes: the desktop it runs on, the app's windows, the
//! foreground and what lies under a point, the physical input held, and `SendInput` and
//! synthetic pointer injection, every event tagged as the lane's ([`LANE_TAG`]).
#![expect(
    unsafe_code,
    reason = "Win32 FFI; each call takes plain values or pointers to locals"
)]

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, POINT, RECT, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::StationsAndDesktops::{
    GetThreadDesktop, GetUserObjectInformationW, UOI_IO,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
    POINTER_FEEDBACK_DEFAULT, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
    SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
    KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MAPVK_VK_TO_VSC, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MapVirtualKeyW, SendInput,
    VIRTUAL_KEY, VkKeyScanExW,
};
use windows::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE,
    POINTER_FLAG_PRIMARY, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_FLAGS, POINTER_INFO,
    POINTER_PEN_INFO, POINTER_TOUCH_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GetAncestor, GetClientRect, GetCursorPos, GetForegroundWindow,
    GetSystemMetrics, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, PT_PEN, PT_TOUCH, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_SWAPBUTTON,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, SetForegroundWindow, ShowWindow,
    WindowFromPoint,
};
use windows::core::BOOL;

use crate::coordinates::DevicePoint;
use crate::error::{Error, Result};
use crate::lane::SendFailure;
use crate::lane::windows::activity::LANE_TAG;
use crate::lane::windows::contacts::{Frame, Phase, PointerKind};
use crate::lane::windows::keys::{Modifier, VirtualKey};
use crate::lane::windows::safety::LANE;
use crate::lane::windows::send_input;

/// A window, by the value of its `HWND`: a value that names a window, kept as an integer so the
/// lane that holds it stays `Send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Hwnd(isize);

impl Hwnd {
    fn of(hwnd: HWND) -> Option<Self> {
        (!hwnd.is_invalid()).then_some(Self(hwnd.0 as isize))
    }

    pub(super) fn raw(self) -> HWND {
        HWND(self.0 as *mut core::ffi::c_void)
    }
}

/// Per-monitor DPI awareness for the calling thread while this lives, so every rectangle it reads
/// and every point it sends is in device pixels; the thread's previous awareness after.
pub(super) struct DeviceUnits(DPI_AWARENESS_CONTEXT);

impl DeviceUnits {
    pub(super) fn enter() -> Self {
        // SAFETY: a predefined context constant; the call changes only this thread's awareness,
        // which `Drop` restores.
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for DeviceUnits {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: the context the thread had before `enter`, as that call returned it.
            let _ = unsafe { SetThreadDpiAwarenessContext(self.0) };
        }
    }
}

/// The calling thread in COM's multithreaded apartment for as long as this lives, unless it was
/// in another apartment already, which then serves.
pub(super) struct Apartment {
    joined: bool,
}

impl Apartment {
    pub(super) fn enter() -> Result<Self> {
        // SAFETY: paired with `CoUninitialize` in `Drop` on this thread when it succeeds.
        let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if entered == RPC_E_CHANGED_MODE {
            return Ok(Self { joined: false });
        }
        entered.ok().map_err(|error| {
            lane_error(format!("COM could not start on the lane's thread: {error}"))
        })?;
        Ok(Self { joined: true })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.joined {
            // SAFETY: this thread's successful `CoInitializeEx` in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}

/// Whether the calling thread's desktop is the one taking the user's input (`UOI_IO`): a locked
/// workstation's secure desktop, a disconnected session and a switched-away desktop all answer
/// no, and so does a question Windows refuses to answer.
pub(super) fn on_input_desktop() -> bool {
    // SAFETY: the thread's own desktop handle, which belongs to the thread and is not closed.
    let Ok(desktop) = (unsafe { GetThreadDesktop(GetCurrentThreadId()) }) else {
        return false;
    };
    let mut receives = BOOL(0);
    let mut needed = 0_u32;
    // SAFETY: the buffer is the local `BOOL` `UOI_IO` writes, with its size.
    let read = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_IO,
            Some((&raw mut receives).cast()),
            u32::try_from(size_of::<BOOL>()).expect("BUG: a BOOL's size fits a u32"),
            Some(&raw mut needed),
        )
    };
    read.is_ok() && receives.as_bool()
}

/// The process that owns `window`; `None` for a window that no longer exists.
pub(super) fn window_pid(window: Hwnd) -> Option<u32> {
    // SAFETY: `IsWindow` accepts any value and validates it.
    if !unsafe { IsWindow(Some(window.raw())) }.as_bool() {
        return None;
    }
    let mut pid = 0_u32;
    // SAFETY: a window handle; `pid` is a local the call writes.
    unsafe { GetWindowThreadProcessId(window.raw(), Some(&raw mut pid)) };
    (pid != 0).then_some(pid)
}

/// `pid`'s visible top-level windows, front to back.
pub(super) fn process_windows(pid: u32) -> Result<Vec<Hwnd>> {
    struct Search {
        pid: u32,
        found: Vec<Hwnd>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, state: LPARAM) -> BOOL {
        // SAFETY: `state` is the `Search` `process_windows` passes, alive and borrowed by nothing
        // else for the whole enumeration.
        let search = unsafe { &mut *(state.0 as *mut Search) };
        let mut pid = 0_u32;
        // SAFETY: a window the enumeration hands over; `pid` is a local.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
        // SAFETY: a window the enumeration hands over.
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        if pid == search.pid
            && visible
            && let Some(window) = Hwnd::of(hwnd)
        {
            search.found.push(window);
        }
        true.into()
    }
    let mut search = Search {
        pid,
        found: Vec::new(),
    };
    // SAFETY: `visit` matches `WNDENUMPROC`; the pointer is to a local that outlives the call,
    // which returns only when the enumeration is done.
    unsafe { EnumWindows(Some(visit), LPARAM((&raw mut search) as isize)) }
        .map_err(|error| platform("enumerating the app's windows", &error))?;
    Ok(search.found)
}

/// `window`'s client area: its top-left on screen and its size, in device pixels, and its DPI.
pub(super) fn client_area(window: Hwnd) -> Result<(DevicePoint, i32, i32, u32)> {
    let mut rect = RECT::default();
    // SAFETY: a window handle; `rect` is a local the call writes.
    unsafe { GetClientRect(window.raw(), &raw mut rect) }
        .map_err(|error| platform("reading the app window's client area", &error))?;
    let mut origin = POINT::default();
    // SAFETY: as above; `origin` is a local the call converts in place.
    if !unsafe { ClientToScreen(window.raw(), &raw mut origin) }.as_bool() {
        return Err(lane_error(
            "the app window's client area has no place on screen",
        ));
    }
    // SAFETY: a window handle.
    let dpi = unsafe { GetDpiForWindow(window.raw()) };
    Ok((
        DevicePoint::new(origin.x, origin.y),
        rect.right - rect.left,
        rect.bottom - rect.top,
        dpi,
    ))
}

/// The window in front.
pub(super) fn foreground() -> Option<Hwnd> {
    // SAFETY: no arguments.
    Hwnd::of(unsafe { GetForegroundWindow() })
}

/// Restore `window` if it is minimized and ask Windows to put it in front, which Windows may
/// refuse (the foreground lock): the caller reads the foreground after.
pub(super) fn bring_to_front(window: Hwnd) {
    // SAFETY: a window handle; each call only requests a window-manager change.
    unsafe {
        if IsIconic(window.raw()).as_bool() {
            let _ = ShowWindow(window.raw(), SW_RESTORE);
        }
        let _ = SetForegroundWindow(window.raw());
    }
}

/// The top-level window under `point`, as the OS hit-tests a press there.
pub(super) fn root_at(point: DevicePoint) -> Option<Hwnd> {
    // SAFETY: plain value arguments; `GetAncestor` accepts any handle.
    Hwnd::of(unsafe {
        GetAncestor(
            WindowFromPoint(POINT {
                x: point.x,
                y: point.y,
            }),
            GA_ROOT,
        )
    })
}

/// `window` for people: its title and process.
pub(super) fn describe(window: Hwnd) -> String {
    let mut buffer = [0_u16; 128];
    // SAFETY: the buffer is a local slice the call writes at most its length of.
    let len = unsafe { GetWindowTextW(window.raw(), &mut buffer) };
    let title = String::from_utf16_lossy(&buffer[..usize::try_from(len).unwrap_or(0)]);
    match window_pid(window) {
        Some(pid) => format!("{title:?} (process {pid})"),
        None => format!("{title:?}"),
    }
}

fn down(vk: u16) -> bool {
    // SAFETY: a plain value argument.
    let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
    state < 0
}

/// The physical mouse buttons down, as `safety::ButtonBits`.
pub(super) fn buttons_down() -> u8 {
    [0x01_u16, 0x02, 0x04, 0x05, 0x06]
        .into_iter()
        .enumerate()
        .filter(|&(_, vk)| down(vk))
        .fold(0, |bits, (index, _)| bits | (1 << index))
}

/// The modifiers down.
pub(super) fn modifiers_down() -> Vec<Modifier> {
    let mut modifiers = Vec::new();
    for (vks, modifier) in [
        (&[0x10_u16][..], Modifier::Shift),
        (&[0x11][..], Modifier::Control),
        (&[0x12][..], Modifier::Alt),
        (&[0x5B, 0x5C][..], Modifier::Meta),
    ] {
        if vks.iter().any(|&vk| down(vk)) {
            modifiers.push(modifier);
        }
    }
    modifiers
}

/// A key down that is neither a mouse button nor a modifier, other than the Unicode packet key
/// and the keys in `held`.
pub(super) fn other_key_down(held: &[u16]) -> Option<u16> {
    const MOUSE: [u16; 5] = [0x01, 0x02, 0x04, 0x05, 0x06];
    // Shift, Control, Alt, both Windows keys, their left and right forms, and `VK_PACKET`.
    const MODIFIERS: [u16; 12] = [
        0x10, 0x11, 0x12, 0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xE7,
    ];
    (0x08_u16..=0xFE)
        .filter(|vk| !MOUSE.contains(vk) && !MODIFIERS.contains(vk) && !held.contains(vk))
        .find(|&vk| down(vk))
}

/// Whether the primary and secondary buttons are swapped: an injected button is physical, so a
/// swapped mouse's primary press is its right button.
pub(super) fn buttons_swapped() -> bool {
    // SAFETY: a plain metric index.
    unsafe { GetSystemMetrics(SM_SWAPBUTTON) != 0 }
}

/// Where the pointer is.
pub(super) fn cursor() -> Option<DevicePoint> {
    let mut point = POINT::default();
    // SAFETY: `point` is a local the call writes.
    unsafe { GetCursorPos(&raw mut point) }.ok()?;
    Some(DevicePoint::new(point.x, point.y))
}

/// Insert `inputs` in one `SendInput` call: one that Windows inserted in part fails as partly
/// sent, since the events it did insert reached the desktop.
fn send(inputs: &[INPUT], what: &str) -> std::result::Result<(), SendFailure> {
    let size = i32::try_from(size_of::<INPUT>()).expect("BUG: an INPUT is a few dozen bytes");
    // SAFETY: fully initialized `INPUT`s and the size of one.
    let sent = unsafe { SendInput(inputs, size) };
    send_input::outcome(sent, inputs.len(), what)
}

fn mouse(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: LANE_TAG,
            },
        },
    }
}

/// Move the pointer to `point` by an absolute move over the virtual desktop, then read where it
/// landed.
///
/// # Errors
///
/// When the point is off the desktop, the move is blocked, or the pointer landed elsewhere.
pub(super) fn move_pointer(point: DevicePoint) -> Result<()> {
    // SAFETY: plain metric indexes.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let normalized = |at, start, extent| {
        crate::lane::windows::screen::normalized(at, start, extent).ok_or_else(|| {
            Error::outside_target(format!(
                "({}, {}) is off the desktop ({left}, {top}, {width} x {height})",
                point.x, point.y
            ))
        })
    };
    let dx = normalized(point.x, left, width)?;
    let dy = normalized(point.y, top, height)?;
    send(
        &[mouse(
            dx,
            dy,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )],
        "a pointer move",
    )?;
    match cursor() {
        Some(landed) if landed == point => Ok(()),
        landed => Err(lane_error(format!(
            "the pointer landed at {landed:?}, not at ({}, {})",
            point.x, point.y
        ))),
    }
}

/// A physical mouse button: 0 left, 1 right, 2 middle, 3 and 4 the extra buttons.
pub(super) fn mouse_button(button: u8, pressed: bool) -> std::result::Result<(), SendFailure> {
    let (flags, data) = match (button, pressed) {
        (0, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (0, false) => (MOUSEEVENTF_LEFTUP, 0),
        (1, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (1, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (2, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (2, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (3, true) => (MOUSEEVENTF_XDOWN, 1),
        (3, false) => (MOUSEEVENTF_XUP, 1),
        (4, true) => (MOUSEEVENTF_XDOWN, 2),
        (_, _) => (MOUSEEVENTF_XUP, 2),
    };
    send(&[mouse(0, 0, data, flags)], "a mouse button")
}

/// Turn the wheel by `delta_x` rightwards and `delta_y` downwards, in wheel units.
pub(super) fn wheel(delta_x: i32, delta_y: i32) -> std::result::Result<(), SendFailure> {
    let mut inputs = Vec::new();
    if delta_y != 0 {
        // Windows' wheel turns up for a positive delta.
        inputs.push(mouse(
            0,
            0,
            delta_y.wrapping_neg().cast_unsigned(),
            MOUSEEVENTF_WHEEL,
        ));
    }
    if delta_x != 0 {
        inputs.push(mouse(0, 0, delta_x.cast_unsigned(), MOUSEEVENTF_HWHEEL));
    }
    if inputs.is_empty() {
        return Ok(());
    }
    send(&inputs, "a turn of the wheel")
}

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: LANE_TAG,
            },
        },
    }
}

/// Press or release `key`, with its scan code, as a keyboard sends it.
pub(super) fn key(key: VirtualKey, pressed: bool) -> std::result::Result<(), SendFailure> {
    // SAFETY: a pure table lookup.
    let scan = unsafe { MapVirtualKeyW(u32::from(key.code), MAPVK_VK_TO_VSC) };
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !pressed {
        flags |= KEYEVENTF_KEYUP;
    }
    send(
        &[key_input(key.code, u16::try_from(scan).unwrap_or(0), flags)],
        "a key",
    )
}

/// Send `events`, the keyboard events of one stroke ([`send_input::packet_events`]), in one call.
pub(super) fn keyboard(
    events: &[send_input::KeyboardEvent],
    what: &str,
) -> std::result::Result<(), SendFailure> {
    let inputs: Vec<INPUT> = events
        .iter()
        .map(|event| {
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if event.unicode {
                flags |= KEYEVENTF_UNICODE;
            }
            if event.up {
                flags |= KEYEVENTF_KEYUP;
            }
            key_input(event.virtual_key, event.scan, flags)
        })
        .collect();
    send(&inputs, what)
}

/// The key that types `c` on the keyboard layout of `window`'s thread, and whether it needs
/// Shift; `None` when no key types it with Shift alone.
pub(super) fn character_key(c: char, window: Hwnd) -> Option<(VirtualKey, bool)> {
    let mut units = [0_u16; 2];
    let [unit] = c.encode_utf16(&mut units) else {
        return None;
    };
    // SAFETY: a window handle; a thread id of zero names the caller's own layout.
    let layout = unsafe { GetKeyboardLayout(GetWindowThreadProcessId(window.raw(), None)) };
    // SAFETY: plain value arguments.
    let scan = unsafe { VkKeyScanExW(*unit, layout) };
    let [vk, shift] = scan.to_le_bytes();
    if scan == -1 || shift & !1 != 0 {
        return None;
    }
    Some((
        VirtualKey {
            code: u16::from(vk),
            extended: false,
        },
        shift & 1 != 0,
    ))
}

/// A synthetic touch or pen device, destroyed when dropped; kept by the value of its handle so
/// the lane that holds it stays `Send`.
#[derive(Debug)]
pub(super) struct PointerDevice {
    handle: isize,
    kind: PointerKind,
}

impl PointerDevice {
    pub(super) fn create(kind: PointerKind) -> Result<Self> {
        let (pointer_type, contacts) = match kind {
            PointerKind::Touch => (PT_TOUCH, crate::lane::windows::contacts::MAX_TOUCH_CONTACTS),
            _ => (PT_PEN, 1),
        };
        // SAFETY: plain value arguments; the handle is destroyed once, in `Drop`.
        let handle = unsafe {
            CreateSyntheticPointerDevice(pointer_type, contacts, POINTER_FEEDBACK_DEFAULT)
        }
        .map_err(|error| platform("creating a synthetic pointer device", &error))?;
        Ok(Self {
            handle: handle.0 as isize,
            kind,
        })
    }

    pub(super) fn kind(&self) -> PointerKind {
        self.kind
    }

    /// Inject `frame`.
    pub(super) fn inject(&self, frame: &Frame) -> Result<()> {
        let infos: Vec<POINTER_TYPE_INFO> = frame
            .contacts
            .iter()
            .map(|contact| {
                let mut flags = match contact.phase {
                    Phase::Down => {
                        POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT
                    }
                    Phase::Update => {
                        POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT
                    }
                    Phase::Up => POINTER_FLAG_UP | POINTER_FLAG_INRANGE,
                    Phase::Hover => POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE,
                    _ => POINTER_FLAG_UPDATE,
                };
                if contact.primary {
                    flags |= POINTER_FLAG_PRIMARY;
                }
                pointer_info(self.kind, contact.id, contact.at, flags, contact.phase)
            })
            .collect();
        let device = HSYNTHETICPOINTERDEVICE(self.handle as *mut core::ffi::c_void);
        // SAFETY: the device this value was created as and not yet destroyed, and fully
        // initialized frames.
        unsafe { InjectSyntheticPointerInput(device, &infos) }
            .map_err(|error| platform("injecting synthetic pointer input", &error))
    }
}

impl Drop for PointerDevice {
    fn drop(&mut self) {
        // SAFETY: the device created in `create`, destroyed once.
        unsafe {
            DestroySyntheticPointerDevice(HSYNTHETICPOINTERDEVICE(
                self.handle as *mut core::ffi::c_void,
            ));
        }
    }
}

/// The contact area a touch reports: a 4 by 4 pixel square around its point.
const CONTACT_RADIUS: i32 = 2;

/// The pressure a pen in contact reports, of 1024.
const PEN_PRESSURE: u32 = 512;

fn pointer_info(
    kind: PointerKind,
    id: u32,
    at: DevicePoint,
    flags: POINTER_FLAGS,
    phase: Phase,
) -> POINTER_TYPE_INFO {
    let point = POINT { x: at.x, y: at.y };
    match kind {
        PointerKind::Touch => POINTER_TYPE_INFO {
            r#type: PT_TOUCH,
            Anonymous: POINTER_TYPE_INFO_0 {
                touchInfo: POINTER_TOUCH_INFO {
                    pointerInfo: POINTER_INFO {
                        pointerType: PT_TOUCH,
                        pointerId: id,
                        pointerFlags: flags,
                        ptPixelLocation: point,
                        ..POINTER_INFO::default()
                    },
                    // `TOUCH_MASK_CONTACTAREA`.
                    touchMask: 1,
                    rcContact: RECT {
                        left: at.x - CONTACT_RADIUS,
                        top: at.y - CONTACT_RADIUS,
                        right: at.x + CONTACT_RADIUS,
                        bottom: at.y + CONTACT_RADIUS,
                    },
                    ..POINTER_TOUCH_INFO::default()
                },
            },
        },
        _ => POINTER_TYPE_INFO {
            r#type: PT_PEN,
            Anonymous: POINTER_TYPE_INFO_0 {
                penInfo: POINTER_PEN_INFO {
                    pointerInfo: POINTER_INFO {
                        pointerType: PT_PEN,
                        pointerId: id,
                        pointerFlags: flags,
                        ptPixelLocation: point,
                        ..POINTER_INFO::default()
                    },
                    // `PEN_MASK_PRESSURE`.
                    penMask: 1,
                    pressure: if matches!(phase, Phase::Down | Phase::Update) {
                        PEN_PRESSURE
                    } else {
                        0
                    },
                    ..POINTER_PEN_INFO::default()
                },
            },
        },
    }
}

fn platform(what: &str, error: &windows::core::Error) -> Error {
    lane_error(format!("{what}: {error}"))
}

pub(super) fn lane_error(message: impl Into<String>) -> Error {
    Error::Lane {
        lane: LANE.to_owned(),
        unavailable: false,
        message: message.into(),
    }
}
