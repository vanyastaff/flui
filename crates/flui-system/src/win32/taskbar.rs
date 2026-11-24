//! The application's taskbar progress and count over `ITaskbarList3`, on
//! the owner thread, which owns the windows.
//!
//! Every top-level window has a taskbar button of its own, so the entry is
//! shown on the button of each window a [`TaskbarEntry`](crate::taskbar::TaskbarEntry)
//! was acquired in, one state for all of them; a window that closes is
//! forgotten. The taskbar takes a change only once a window's button
//! exists, and forgets it when Explorer restarts; both times it sends the
//! window the registered `TaskbarButtonCreated` message. The provider
//! subclasses each window it shows the entry on (`SetWindowSubclass`, found
//! by ordinal, the window procedure's own chain, with no hook in the host)
//! to hear that message and show the entry again, and removes the subclass
//! when the window closes. The count is an overlay icon drawn here
//! ([`badge_pixels`]): a disk with the count, `99+` above 99. Nothing is
//! called with a borrow held, since a call to Explorer may pump messages
//! back into the subclass.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use flui_platform::WindowId;
use flui_platform::capability::{WindowCloseWatch, WindowRef};
use rustc_hash::FxHashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, HGDIOBJ,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    ITaskbarList3, SUBCLASSPROC, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TBPFLAG,
    TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, CreateIconIndirect, DestroyIcon, HICON, ICONINFO, MSGFLT_ALLOW,
    RegisterWindowMessageW, SM_CXSMICON, WM_NCDESTROY,
};
use windows::core::{BOOL, HSTRING, PCSTR, w};

use super::{Owner, os_code};
use crate::taskbar::{EntryState, Fraction, Honoured, Progress, TaskbarBackend};
use flui_foundation::panic::{AfterPanic, os_entry};

/// The scale `ITaskbarList3` reports progress against.
const PROGRESS_TOTAL: u64 = 10_000;

/// The state and value a progress is shown with: the value first, since
/// setting one leaves the no-progress and indeterminate states.
pub(super) fn progress_call(progress: Progress) -> (TBPFLAG, Option<u64>) {
    let value = |fraction: Fraction| {
        // A fraction is finite and within 0 and 1: the product is within
        // 0 and the total, exactly representable.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a value within 0 and PROGRESS_TOTAL, rounded"
        )]
        let completed = (fraction.get() * PROGRESS_TOTAL as f64).round() as u64;
        Some(completed.min(PROGRESS_TOTAL))
    };
    match progress {
        Progress::Hidden => (TBPF_NOPROGRESS, None),
        Progress::Indeterminate => (TBPF_INDETERMINATE, None),
        Progress::Done(fraction) => (TBPF_NORMAL, value(fraction)),
    }
}

/// The text a count's badge shows: the count, `99+` above 99; none for no
/// badge.
pub(super) fn badge_label(count: Option<u64>) -> Option<String> {
    match count {
        Some(count) if count > 99 => Some("99+".to_owned()),
        Some(count) => Some(count.to_string()),
        None => None,
    }
}

/// A 3 by 5 glyph of a badge's label, one row per entry, bit 2 leftmost.
fn glyph(character: char) -> [u8; 5] {
    match character {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        _ => [0; 5],
    }
}

/// The badge's pixels, `size` by `size`, top row first, each `0xAARRGGBB`
/// with straight alpha: a red disk, edged smoothly, and the label in white
/// pixels scaled to the icon (one glyph pixel per 16th of it).
pub(super) fn badge_pixels(size: u32, label: Option<&str>) -> Vec<u32> {
    const DISK: (u32, u32, u32) = (0xD1, 0x34, 0x38);
    let side = size as usize;
    let mut pixels = vec![0_u32; side * side];
    let radius = f64::from(size) / 2.0;
    let mut text = vec![false; side * side];
    if let Some(label) = label {
        let scale = (size / 16).max(1) as usize;
        let glyphs = label.chars().count();
        let width = (glyphs * 4).saturating_sub(1) * scale;
        let height = 5 * scale;
        let left = side.saturating_sub(width) / 2;
        let top = side.saturating_sub(height) / 2;
        for (index, character) in label.chars().enumerate() {
            for (row, bits) in glyph(character).into_iter().enumerate() {
                for column in 0..3 {
                    if bits & (0b100 >> column) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = left + (index * 4 + column) * scale + dx;
                            let y = top + row * scale + dy;
                            if x < side && y < side {
                                text[y * side + x] = true;
                            }
                        }
                    }
                }
            }
        }
    }
    for y in 0..side {
        for x in 0..side {
            let dx = x as f64 + 0.5 - radius;
            let dy = y as f64 + 0.5 - radius;
            let coverage = (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a coverage within 0 and 1, scaled to a byte"
            )]
            let alpha = (coverage * 255.0).round() as u32;
            let (red, green, blue) = if text[y * side + x] {
                (0xFF, 0xFF, 0xFF)
            } else {
                DISK
            };
            pixels[y * side + x] = (alpha << 24) | (red << 16) | (green << 8) | blue;
        }
    }
    pixels
}

/// The taskbar as the provider reaches it, on the owner thread. Each
/// failure is the `HRESULT` or Win32 error reported.
pub(crate) trait TaskbarSystem: 'static {
    /// Show `state` (and `value` of [`PROGRESS_TOTAL`]) on `owner`'s button.
    fn progress(&self, owner: Owner, state: TBPFLAG, value: Option<u64>) -> Result<(), i32>;

    /// Show `count` over `owner`'s button, or no badge.
    fn badge(&self, owner: Owner, count: Option<u64>) -> Result<(), i32>;

    /// Hear `owner`'s `TaskbarButtonCreated` and tell `buttons`, until
    /// [`unwatch`](Self::unwatch).
    fn watch(&self, owner: Owner, buttons: &Rc<TaskbarButtons>) -> Result<(), i32>;

    /// Stop hearing `owner`'s button.
    fn unwatch(&self, owner: Owner);
}

/// A window whose button shows the entry.
struct Button {
    owner: Owner,
    _watch: WindowCloseWatch,
}

/// Makes the taskbar system of a runtime, the first time one is asked for.
type MakeSystem = Box<dyn Fn() -> Result<Box<dyn TaskbarSystem>, i32>>;

/// One UI runtime's entry: what it shows and the buttons it shows it on.
pub(crate) struct TaskbarButtons {
    buttons: RefCell<FxHashMap<WindowId, Button>>,
    state: Cell<EntryState>,
    system: OnceCell<Result<Box<dyn TaskbarSystem>, i32>>,
    make: MakeSystem,
}

impl Default for TaskbarButtons {
    fn default() -> Self {
        Self::over(Box::new(|| {
            ComTaskbar::create().map(|taskbar| Box::new(taskbar) as Box<dyn TaskbarSystem>)
        }))
    }
}

impl TaskbarButtons {
    pub(super) fn over(make: MakeSystem) -> Self {
        Self {
            buttons: RefCell::default(),
            state: Cell::new(EntryState::default()),
            system: OnceCell::new(),
            make,
        }
    }

    /// The taskbar, or `None` in a session without one (a server core, a
    /// session without Explorer).
    fn system(&self) -> Option<&dyn TaskbarSystem> {
        self.system.get_or_init(|| (self.make)()).as_deref().ok()
    }

    /// Show the entry on `window`'s button from now on, until it closes. A
    /// window with no Win32 handle, or one already shown on, is left as it
    /// is.
    fn add(self: &Rc<Self>, window: &WindowRef) {
        let (Some(owner), Some(system)) = (Owner::of(window), self.system()) else {
            return;
        };
        let id = window.id();
        if window.lifetime().is_closed() || self.buttons.borrow().contains_key(&id) {
            return;
        }
        // The subclass first: a button made from here on is heard.
        if let Err(code) = system.watch(owner, self) {
            tracing::warn!(code, "the taskbar button's subclass was refused");
            return;
        }
        let closing: Weak<Self> = Rc::downgrade(self);
        let watch = window.lifetime().on_close(move || {
            if let Some(buttons) = closing.upgrade() {
                buttons.forget(id);
            }
        });
        let replaced = self.buttons.borrow_mut().insert(
            id,
            Button {
                owner,
                _watch: watch,
            },
        );
        drop(replaced);
        show_on(system, owner, self.state.get());
    }

    /// Show `state` on every button. With no button the taskbar is not
    /// reached: a window added later shows the state it finds.
    fn show(&self, state: EntryState) {
        self.state.set(state);
        let owners: Vec<Owner> = self
            .buttons
            .borrow()
            .values()
            .map(|button| button.owner)
            .collect();
        if owners.is_empty() {
            return;
        }
        let Some(system) = self.system() else {
            return;
        };
        for owner in owners {
            show_on(system, owner, state);
        }
    }

    /// `owner`'s button was made, or made again: show the entry on it.
    pub(super) fn button_created(&self, owner: Owner) {
        let known = self
            .buttons
            .try_borrow()
            .is_ok_and(|buttons| buttons.values().any(|button| button.owner == owner));
        if let (true, Some(system)) = (known, self.system()) {
            show_on(system, owner, self.state.get());
        }
    }

    /// The window closed: stop hearing its button and forget it.
    fn forget(&self, id: WindowId) {
        let removed = self.buttons.borrow_mut().remove(&id);
        if let (Some(button), Some(system)) = (&removed, self.system()) {
            system.unwatch(button.owner);
        }
        drop(removed);
    }
}

impl Drop for TaskbarButtons {
    fn drop(&mut self) {
        let buttons = std::mem::take(self.buttons.get_mut());
        if let Some(Ok(system)) = self.system.get() {
            for button in buttons.values() {
                system.unwatch(button.owner);
            }
        }
    }
}

/// Show `state` on `owner`'s button. A button not made yet refuses; it is
/// shown again when the taskbar makes it.
fn show_on(system: &dyn TaskbarSystem, owner: Owner, state: EntryState) {
    let (flag, value) = progress_call(state.progress);
    let progressed = system.progress(owner, flag, value);
    let badged = system.badge(owner, state.count);
    if let Err(code) = progressed.and(badged) {
        tracing::debug!(code, "the taskbar button took no change yet");
    }
}

/// [`TaskbarBackend`] over `ITaskbarList3`.
pub(crate) struct WindowsTaskbar {
    buttons: Rc<TaskbarButtons>,
}

impl WindowsTaskbar {
    /// The entry of `buttons`' runtime, shown from now on on `window`'s
    /// button as well, when there is one.
    pub(crate) fn new(buttons: Rc<TaskbarButtons>, window: Option<&WindowRef>) -> Self {
        if let Some(window) = window {
            buttons.add(window);
        }
        Self { buttons }
    }
}

impl TaskbarBackend for WindowsTaskbar {
    fn show(&self, state: EntryState) {
        self.buttons.show(state);
    }

    fn honours(&self) -> Honoured {
        if self.buttons.system().is_some() {
            // `ITaskbarList3` shows every part, on a button it confirms.
            Honoured::new(true, true, true, true)
        } else {
            Honoured::new(false, false, false, false)
        }
    }
}

/// The subclass id the provider installs on a window.
const SUBCLASS: usize = 0x666C_7569;

/// The real taskbar.
struct ComTaskbar {
    taskbar: ITaskbarList3,
    button_created: u32,
    subclassing: Subclassing,
}

impl ComTaskbar {
    fn create() -> Result<Self, i32> {
        let subclassing = Subclassing::load()?;
        // SAFETY: created and used on the owner thread, inside the
        // apartment the host entered there.
        let taskbar: ITaskbarList3 =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| os_code(error.code()))?;
        // SAFETY: a checked method of the live object.
        unsafe { taskbar.HrInit() }.map_err(|error| os_code(error.code()))?;
        // SAFETY: a constant, NUL-terminated name.
        let button_created = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
        Ok(Self {
            taskbar,
            button_created,
            subclassing,
        })
    }
}

type SetSubclass = unsafe extern "system" fn(HWND, SUBCLASSPROC, usize, usize) -> BOOL;
type GetSubclass = unsafe extern "system" fn(HWND, SUBCLASSPROC, usize, *mut usize) -> BOOL;
type RemoveSubclass = unsafe extern "system" fn(HWND, SUBCLASSPROC, usize) -> BOOL;
type DefaultSubclass = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// The common controls' subclass calls, found by ordinal.
///
/// Version 5.82 of `comctl32.dll`, which a process loads unless its
/// manifest asks for version 6, exports them by ordinal only (410 to 413),
/// as the SDK's import library links them; a link by name would keep an
/// application without that manifest from starting.
#[derive(Clone, Copy)]
struct Subclassing {
    set: SetSubclass,
    get: GetSubclass,
    remove: RemoveSubclass,
    default: DefaultSubclass,
}

impl Subclassing {
    fn load() -> Result<Self, i32> {
        let failed = || os_code(windows::core::Error::from_thread().code());
        // SAFETY: loads (or finds loaded) the common controls from the
        // system folder only, never a `comctl32.dll` beside the executable;
        // the module stays loaded for the process's life, which the pointers
        // below rely on.
        let module =
            unsafe { LoadLibraryExW(w!("comctl32.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                .map_err(|error| os_code(error.code()))?;
        let find = |ordinal: usize| {
            // SAFETY: an ordinal passed as the name, as `MAKEINTRESOURCE`
            // does; the module is the one loaded above.
            unsafe { GetProcAddress(module, PCSTR(ordinal as *const u8)) }.ok_or_else(failed)
        };
        let (set, get, remove, default) = (find(410)?, find(411)?, find(412)?, find(413)?);
        // SAFETY: ordinals 410 to 413 of `comctl32.dll` are
        // `SetWindowSubclass`, `GetWindowSubclass`, `RemoveWindowSubclass`
        // and `DefSubclassProc` in every version since 5.8, with these
        // signatures (commctrl.h).
        unsafe {
            Ok(Self {
                set: std::mem::transmute::<unsafe extern "system" fn() -> isize, SetSubclass>(set),
                get: std::mem::transmute::<unsafe extern "system" fn() -> isize, GetSubclass>(get),
                remove: std::mem::transmute::<unsafe extern "system" fn() -> isize, RemoveSubclass>(
                    remove,
                ),
                default: std::mem::transmute::<unsafe extern "system" fn() -> isize, DefaultSubclass>(
                    default,
                ),
            })
        }
    }
}

/// An icon destroyed when dropped.
struct Icon(HICON);

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: an icon `badge_icon` made and nothing else holds; the
        // taskbar copies the icon it is given.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

/// A GDI object deleted when dropped.
struct GdiObject(HGDIOBJ);

impl Drop for GdiObject {
    fn drop(&mut self) {
        // SAFETY: an object made here and selected into no device context.
        let _ = unsafe { DeleteObject(self.0) };
    }
}

/// The icon of `label` at the size of a small icon for `owner`'s scale.
fn badge_icon(owner: Owner, label: Option<&str>) -> Result<Icon, i32> {
    let failed = || os_code(windows::core::Error::from_thread().code());
    // SAFETY: plain values; the window is the owner thread's.
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(owner.hwnd())) };
    let size = u32::try_from(size)
        .ok()
        .filter(|size| *size > 0)
        .unwrap_or(16);
    let pixels = badge_pixels(size, label);
    let side = i32::try_from(size).unwrap_or(16);
    let header = BITMAPINFOHEADER {
        biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>())
            .expect("BUG: BITMAPINFOHEADER is 40 bytes"),
        biWidth: side,
        // Negative: top row first, as the pixels are.
        biHeight: -side,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: `info` describes a 32-bit top-down bitmap of `size` squared
    // pixels; the call writes the address of its own pixel memory to `bits`.
    let color = unsafe {
        CreateDIBSection(
            None,
            &raw const info,
            DIB_RGB_COLORS,
            &raw mut bits,
            None,
            0,
        )
    }
    .map_err(|error| os_code(error.code()))?;
    let color = GdiObject(color.into());
    if bits.is_null() {
        return Err(failed());
    }
    // SAFETY: `bits` is the section's memory, `size` squared 32-bit pixels,
    // which `pixels` holds exactly; nothing else writes it.
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
    }
    // A monochrome mask of zeros, rows padded to 16 bits: the color's
    // alpha decides what shows.
    let row = size.div_ceil(16) as usize * 2;
    let zeros = vec![0_u8; row * size as usize];
    // SAFETY: `zeros` holds the mask's rows and outlives the call.
    let mask = unsafe { CreateBitmap(side, side, 1, 1, Some(zeros.as_ptr().cast())) };
    if mask.is_invalid() {
        return Err(failed());
    }
    let mask = GdiObject(mask.into());
    let icon = ICONINFO {
        fIcon: true.into(),
        hbmMask: windows::Win32::Graphics::Gdi::HBITMAP(mask.0.0),
        hbmColor: windows::Win32::Graphics::Gdi::HBITMAP(color.0.0),
        ..Default::default()
    };
    // SAFETY: `icon` names the two live bitmaps above, which the call copies.
    unsafe { CreateIconIndirect(&raw const icon) }
        .map(Icon)
        .map_err(|error| os_code(error.code()))
}

impl TaskbarSystem for ComTaskbar {
    fn progress(&self, owner: Owner, state: TBPFLAG, value: Option<u64>) -> Result<(), i32> {
        // SAFETY: checked methods of the live object on the owner thread,
        // for the owner thread's window.
        unsafe {
            if let Some(value) = value {
                self.taskbar
                    .SetProgressValue(owner.hwnd(), value, PROGRESS_TOTAL)
                    .map_err(|error| os_code(error.code()))?;
            }
            self.taskbar
                .SetProgressState(owner.hwnd(), state)
                .map_err(|error| os_code(error.code()))
        }
    }

    fn badge(&self, owner: Owner, count: Option<u64>) -> Result<(), i32> {
        let label = badge_label(count);
        let icon = match &label {
            None => None,
            Some(label) => Some(badge_icon(owner, Some(label))?),
        };
        let description = HSTRING::from(label.unwrap_or_default());
        // SAFETY: as above; the taskbar copies the icon, which is destroyed
        // after the call.
        unsafe {
            self.taskbar.SetOverlayIcon(
                owner.hwnd(),
                icon.as_ref().map_or_default(|icon| icon.0),
                &description,
            )
        }
        .map_err(|error| os_code(error.code()))
    }

    fn watch(&self, owner: Owner, buttons: &Rc<TaskbarButtons>) -> Result<(), i32> {
        let reference = Box::into_raw(Box::new(Watching {
            buttons: Rc::downgrade(buttons),
            button_created: self.button_created,
            subclassing: self.subclassing,
        }));
        // SAFETY: the window is the owner thread's; the reference data is a
        // box this subclass owns, freed by the subclass procedure when the
        // window is destroyed or by `unwatch` when it is removed.
        let installed = unsafe {
            (self.subclassing.set)(
                owner.hwnd(),
                Some(button_procedure),
                SUBCLASS,
                reference as usize,
            )
        };
        if installed.as_bool() {
            // An elevated process hears Explorer, which runs at medium
            // integrity, only for the messages its windows let through: let
            // this one through on this window.
            // SAFETY: the window is the owner thread's; no out-parameter.
            let _ = unsafe {
                ChangeWindowMessageFilterEx(owner.hwnd(), self.button_created, MSGFLT_ALLOW, None)
            };
            Ok(())
        } else {
            // SAFETY: the box was not handed to a subclass: freed here, once.
            drop(unsafe { Box::from_raw(reference) });
            Err(os_code(windows::core::Error::from_thread().code()))
        }
    }

    fn unwatch(&self, owner: Owner) {
        let mut reference = 0_usize;
        // SAFETY: reads back this subclass's reference data, if installed.
        let installed = unsafe {
            (self.subclassing.get)(
                owner.hwnd(),
                Some(button_procedure),
                SUBCLASS,
                &raw mut reference,
            )
        };
        // SAFETY: removes this subclass from the owner thread's window.
        let removed =
            unsafe { (self.subclassing.remove)(owner.hwnd(), Some(button_procedure), SUBCLASS) };
        if installed.as_bool() && removed.as_bool() && reference != 0 {
            // SAFETY: the box `watch` leaked for this subclass, freed once
            // now that the subclass that held it is gone.
            drop(unsafe { Box::from_raw(reference as *mut Watching) });
        }
    }
}

/// What a window's subclass holds: the runtime's buttons, the message the
/// taskbar makes a button with, and the calls the subclass chains through.
struct Watching {
    buttons: Weak<TaskbarButtons>,
    button_created: u32,
    subclassing: Subclassing,
}

/// What the OS calls the subclass procedure with.
type SubclassCall = (HWND, u32, WPARAM, LPARAM, usize, usize);

/// The subclass procedure: on `TaskbarButtonCreated`, show the window's
/// button again; on `WM_NCDESTROY`, remove itself. It calls nothing of the
/// application's: the buttons show a value through the taskbar. A panic in
/// it is contained, and the message goes on down the window's chain.
unsafe extern "system" fn button_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass: usize,
    reference: usize,
) -> LRESULT {
    os_entry(
        "the taskbar button's subclass",
        (window, message, wparam, lparam, subclass, reference),
        AfterPanic::Answer(chain_on),
        || button_message((window, message, wparam, lparam, subclass, reference)),
    )
}

/// The subclass's work for one message, then the rest of the chain.
fn button_message(call: SubclassCall) -> LRESULT {
    let (window, message, wparam, lparam, _, reference) = call;
    let raw = reference as *mut Watching;
    // SAFETY: the reference data is the live box `watch` installed with
    // this subclass, on this thread, until it is freed below or by
    // `unwatch`, each of which first removes the subclass.
    let Some(watching) = (unsafe { raw.as_ref() }) else {
        return LRESULT(0);
    };
    let subclassing = watching.subclassing;
    if message == watching.button_created
        && let Some(buttons) = watching.buttons.upgrade()
        && let Some(owner) = std::num::NonZeroIsize::new(window.0 as isize)
    {
        buttons.button_created(Owner(owner));
    }
    if message == WM_NCDESTROY {
        // SAFETY: removes this subclass, then frees its box once, the last
        // use of it; the calls it chains through were copied out first.
        unsafe {
            let _ = (subclassing.remove)(window, Some(button_procedure), SUBCLASS);
            drop(Box::from_raw(raw));
        }
    }
    // SAFETY: the rest of the window's chain, with the arguments given.
    unsafe { (subclassing.default)(window, message, wparam, lparam) }
}

/// After a panic in [`button_message`], the rest of the window's chain: the
/// box is still the subclass's, since freeing it is the work's last step
/// and runs nothing that panics.
fn chain_on(call: SubclassCall) -> LRESULT {
    let (window, message, wparam, lparam, _, reference) = call;
    // SAFETY: as in `button_message`: the live box of this subclass.
    let Some(watching) = (unsafe { (reference as *const Watching).as_ref() }) else {
        return LRESULT(0);
    };
    // SAFETY: the rest of the window's chain, with the arguments given.
    unsafe { (watching.subclassing.default)(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use windows::Win32::UI::Shell::{TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL};

    use super::{
        Owner, TaskbarButtons, TaskbarSystem, WindowsTaskbar, badge_label, badge_pixels,
        progress_call,
    };
    use crate::taskbar::{EntryState, Fraction, Honoured, Progress, TaskbarBackend};
    use crate::testing::win32::{FakeTaskbar, TaskbarCall, win32_window};

    #[test]
    fn a_progress_becomes_the_taskbars_state_and_value() {
        let half = Fraction::try_new(0.5).expect("a fraction");
        let whole = Fraction::try_new(1.0).expect("a fraction");
        let cases = [
            (Progress::Hidden, (TBPF_NOPROGRESS, None)),
            (Progress::Indeterminate, (TBPF_INDETERMINATE, None)),
            (Progress::Done(half), (TBPF_NORMAL, Some(5_000))),
            (Progress::Done(whole), (TBPF_NORMAL, Some(10_000))),
        ];
        for (progress, expected) in cases {
            assert_eq!(progress_call(progress), expected, "{progress:?}");
        }
    }

    #[test]
    fn a_badge_shows_its_count_up_to_99() {
        assert_eq!(badge_label(Some(0)).as_deref(), Some("0"));
        assert_eq!(badge_label(Some(99)).as_deref(), Some("99"));
        assert_eq!(badge_label(Some(100)).as_deref(), Some("99+"));
        assert_eq!(badge_label(None), None);

        let empty = badge_pixels(16, None);
        let seven = badge_pixels(16, Some("7"));
        assert_eq!(empty.len(), 256);
        assert_eq!(empty[0] >> 24, 0, "a corner is outside the disk");
        assert_eq!(empty[8 * 16 + 8] >> 24, 0xFF, "the centre is inside it");
        assert!(
            seven.iter().any(|pixel| pixel & 0x00FF_FFFF == 0x00FF_FFFF),
            "a count draws its digits in white"
        );
        let wide = badge_pixels(32, Some("99+"));
        assert_eq!(wide.len(), 1024, "an icon of a scaled display");
    }

    /// The calls a fake taskbar took.
    type Calls = Rc<RefCell<Vec<TaskbarCall>>>;

    /// Buttons over a fake taskbar, the calls it took and whether its
    /// buttons are made.
    fn buttons() -> (Rc<TaskbarButtons>, Calls, Rc<Cell<bool>>) {
        let calls: Calls = Rc::default();
        let made = Rc::new(Cell::new(false));
        let system = RefCell::new(Some(FakeTaskbar {
            calls: Rc::clone(&calls),
            made: Rc::clone(&made),
        }));
        let buttons = Rc::new(TaskbarButtons::over(Box::new(move || {
            Ok(Box::new(system.borrow_mut().take().expect("made once")) as Box<dyn TaskbarSystem>)
        })));
        (buttons, calls, made)
    }

    #[test]
    fn a_button_made_late_or_again_shows_the_entry_last_set() {
        let (buttons, calls, made) = buttons();
        let (window, closer) = win32_window(0x1234);
        let owner = Owner::of(&window).expect("a Win32 window");
        let taskbar = WindowsTaskbar::new(Rc::clone(&buttons), Some(&window));
        let half = Fraction::try_new(0.5).expect("a fraction");
        let state = EntryState::new(Progress::Done(half), Some(3));

        taskbar.show(state);
        assert_eq!(
            taskbar.honours(),
            Honoured::new(true, true, true, true),
            "the taskbar shows every part"
        );
        calls.borrow_mut().clear();

        made.set(true);
        buttons.button_created(owner);
        assert_eq!(
            *calls.borrow(),
            [
                TaskbarCall::Progress(TBPF_NORMAL, Some(5_000)),
                TaskbarCall::Badge(Some(3))
            ],
            "the button shows the last entry once it is made"
        );

        calls.borrow_mut().clear();
        closer.close();
        assert_eq!(
            *calls.borrow(),
            [TaskbarCall::Unwatch],
            "a closed window is forgotten"
        );
        calls.borrow_mut().clear();
        buttons.button_created(owner);
        taskbar.show(EntryState::default());
        assert!(calls.borrow().is_empty(), "nothing is shown on it again");
    }

    #[test]
    fn the_entry_is_shown_on_every_window_it_was_acquired_in() {
        let (buttons, calls, made) = buttons();
        made.set(true);
        let (first, _first_closer) = win32_window(0x1);
        let (second, _second_closer) = win32_window(0x2);
        let one = WindowsTaskbar::new(Rc::clone(&buttons), Some(&first));
        one.show(EntryState::new(Progress::Indeterminate, None));
        calls.borrow_mut().clear();

        // A window acquiring later shows the entry as it stands.
        let _two = WindowsTaskbar::new(Rc::clone(&buttons), Some(&second));
        assert_eq!(
            *calls.borrow(),
            [
                TaskbarCall::Watch,
                TaskbarCall::Progress(TBPF_INDETERMINATE, None),
                TaskbarCall::Badge(None)
            ]
        );
        calls.borrow_mut().clear();

        one.show(EntryState::new(Progress::Hidden, Some(1)));
        let shown = calls
            .borrow()
            .iter()
            .filter(|call| **call == TaskbarCall::Badge(Some(1)))
            .count();
        assert_eq!(shown, 2, "both buttons: {:?}", calls.borrow());
    }

    #[test]
    fn a_session_without_a_taskbar_honours_nothing() {
        let buttons = Rc::new(TaskbarButtons::over(Box::new(|| Err(1))));
        let (window, _closer) = win32_window(0x1);
        let taskbar = WindowsTaskbar::new(buttons, Some(&window));
        taskbar.show(EntryState::new(Progress::Indeterminate, Some(2)));
        assert_eq!(taskbar.honours(), Honoured::new(false, false, false, false));
    }
}
