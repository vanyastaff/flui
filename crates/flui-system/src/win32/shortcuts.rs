//! Global shortcuts over `RegisterHotKey`, heard by a message-only window
//! of the UI runtime's own on the owner thread.
//!
//! The owner thread's loop dispatches every window of the thread, so the
//! window's `WM_HOTKEY` arrives without a hook in the host (ADR-0154 §9,
//! item 4). A press is handed to its registration's
//! [`ShortcutPresses`], which runs the listener at the owner's next turn,
//! never inside the window procedure. A chord names a physical key: a
//! letter, digit or punctuation key maps through the keyboard layout active
//! when it is registered (`MapVirtualKeyExW` on its scan code), every other
//! key to its fixed virtual key. `MOD_NOREPEAT` keeps a held chord one press.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use flui_platform::capability::Resolver;
use flui_platform::keyboard::{Code, Modifiers};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use rustc_hash::FxHashMap;
use windows::Win32::Foundation::{ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, HOT_KEY_MODIFIERS, MAPVK_VSC_TO_VK, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, MapVirtualKeyExW, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HWND_MESSAGE,
    RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE, WM_HOTKEY, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::w;

use crate::shortcut::{
    GlobalShortcutBackend, GlobalShortcutCapability, Shortcut, ShortcutError, ShortcutHold,
    ShortcutPresses,
};

/// The highest id an application's hot key may have.
const MAX_HOTKEY_ID: i32 = 0xBFFF;

/// Where a key's virtual key comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeySource {
    /// A key whose character the layout decides, by its set-1 scan code.
    Scan(u32),
    /// A key whose virtual key is the same on every layout.
    Fixed(u32),
}

/// How `code` is found, or `None` for a key a hot key cannot name.
#[expect(
    clippy::too_many_lines,
    reason = "one table row per key; split, the table would hide its gaps"
)]
pub(super) fn key_source(code: Code) -> Option<KeySource> {
    use KeySource::{Fixed, Scan};
    let source = match code {
        Code::KeyA => Scan(0x1E),
        Code::KeyB => Scan(0x30),
        Code::KeyC => Scan(0x2E),
        Code::KeyD => Scan(0x20),
        Code::KeyE => Scan(0x12),
        Code::KeyF => Scan(0x21),
        Code::KeyG => Scan(0x22),
        Code::KeyH => Scan(0x23),
        Code::KeyI => Scan(0x17),
        Code::KeyJ => Scan(0x24),
        Code::KeyK => Scan(0x25),
        Code::KeyL => Scan(0x26),
        Code::KeyM => Scan(0x32),
        Code::KeyN => Scan(0x31),
        Code::KeyO => Scan(0x18),
        Code::KeyP => Scan(0x19),
        Code::KeyQ => Scan(0x10),
        Code::KeyR => Scan(0x13),
        Code::KeyS => Scan(0x1F),
        Code::KeyT => Scan(0x14),
        Code::KeyU => Scan(0x16),
        Code::KeyV => Scan(0x2F),
        Code::KeyW => Scan(0x11),
        Code::KeyX => Scan(0x2D),
        Code::KeyY => Scan(0x15),
        Code::KeyZ => Scan(0x2C),
        Code::Digit1 => Scan(0x02),
        Code::Digit2 => Scan(0x03),
        Code::Digit3 => Scan(0x04),
        Code::Digit4 => Scan(0x05),
        Code::Digit5 => Scan(0x06),
        Code::Digit6 => Scan(0x07),
        Code::Digit7 => Scan(0x08),
        Code::Digit8 => Scan(0x09),
        Code::Digit9 => Scan(0x0A),
        Code::Digit0 => Scan(0x0B),
        Code::Minus => Scan(0x0C),
        Code::Equal => Scan(0x0D),
        Code::BracketLeft => Scan(0x1A),
        Code::BracketRight => Scan(0x1B),
        Code::Semicolon => Scan(0x27),
        Code::Quote => Scan(0x28),
        Code::Backquote => Scan(0x29),
        Code::Backslash => Scan(0x2B),
        Code::Comma => Scan(0x33),
        Code::Period => Scan(0x34),
        Code::Slash => Scan(0x35),
        Code::F1 => Fixed(0x70),
        Code::F2 => Fixed(0x71),
        Code::F3 => Fixed(0x72),
        Code::F4 => Fixed(0x73),
        Code::F5 => Fixed(0x74),
        Code::F6 => Fixed(0x75),
        Code::F7 => Fixed(0x76),
        Code::F8 => Fixed(0x77),
        Code::F9 => Fixed(0x78),
        Code::F10 => Fixed(0x79),
        Code::F11 => Fixed(0x7A),
        Code::F12 => Fixed(0x7B),
        Code::F13 => Fixed(0x7C),
        Code::F14 => Fixed(0x7D),
        Code::F15 => Fixed(0x7E),
        Code::F16 => Fixed(0x7F),
        Code::F17 => Fixed(0x80),
        Code::F18 => Fixed(0x81),
        Code::F19 => Fixed(0x82),
        Code::F20 => Fixed(0x83),
        Code::F21 => Fixed(0x84),
        Code::F22 => Fixed(0x85),
        Code::F23 => Fixed(0x86),
        Code::F24 => Fixed(0x87),
        Code::Space => Fixed(0x20),
        Code::Enter => Fixed(0x0D),
        Code::Escape => Fixed(0x1B),
        Code::Tab => Fixed(0x09),
        Code::Backspace => Fixed(0x08),
        Code::Insert => Fixed(0x2D),
        Code::Delete => Fixed(0x2E),
        Code::Home => Fixed(0x24),
        Code::End => Fixed(0x23),
        Code::PageUp => Fixed(0x21),
        Code::PageDown => Fixed(0x22),
        Code::ArrowLeft => Fixed(0x25),
        Code::ArrowUp => Fixed(0x26),
        Code::ArrowRight => Fixed(0x27),
        Code::ArrowDown => Fixed(0x28),
        Code::Numpad0 => Fixed(0x60),
        Code::Numpad1 => Fixed(0x61),
        Code::Numpad2 => Fixed(0x62),
        Code::Numpad3 => Fixed(0x63),
        Code::Numpad4 => Fixed(0x64),
        Code::Numpad5 => Fixed(0x65),
        Code::Numpad6 => Fixed(0x66),
        Code::Numpad7 => Fixed(0x67),
        Code::Numpad8 => Fixed(0x68),
        Code::Numpad9 => Fixed(0x69),
        Code::PrintScreen => Fixed(0x2C),
        Code::Pause => Fixed(0x13),
        Code::MediaPlayPause => Fixed(0xB3),
        Code::MediaStop => Fixed(0xB2),
        Code::MediaTrackNext => Fixed(0xB0),
        Code::MediaTrackPrevious => Fixed(0xB1),
        Code::AudioVolumeMute => Fixed(0xAD),
        Code::AudioVolumeDown => Fixed(0xAE),
        Code::AudioVolumeUp => Fixed(0xAF),
        _ => return None,
    };
    Some(source)
}

/// The hot-key modifiers of a shortcut's: every chord with `MOD_NOREPEAT`.
pub(super) fn hotkey_modifiers(modifiers: Modifiers) -> HOT_KEY_MODIFIERS {
    let mut hotkey = MOD_NOREPEAT;
    for (modifier, flag) in [
        (Modifiers::CONTROL, MOD_CONTROL),
        (Modifiers::ALT, MOD_ALT),
        (Modifiers::SHIFT, MOD_SHIFT),
        (Modifiers::META, MOD_WIN),
    ] {
        if modifiers.contains(modifier) {
            hotkey |= flag;
        }
    }
    hotkey
}

/// The hot keys as the provider reaches them, on the owner thread.
pub(crate) trait HotkeySystem: 'static {
    /// Register hot key `id`; the failure is the Win32 error.
    fn register(&self, id: i32, modifiers: HOT_KEY_MODIFIERS, key: KeySource) -> Result<(), i32>;

    /// Unregister hot key `id`.
    fn unregister(&self, id: i32);
}

/// The presses each registered hot key reports to, by id, and the ids free
/// for the next.
#[derive(Default)]
pub(super) struct HotkeyTable {
    entries: RefCell<FxHashMap<i32, ShortcutPresses>>,
    free: RefCell<Vec<i32>>,
    next: Cell<i32>,
}

impl HotkeyTable {
    /// An id no live hot key of this runtime has; `None` when all
    /// [`MAX_HOTKEY_ID`] are taken.
    fn mint(&self) -> Option<i32> {
        if let Some(id) = self.free.borrow_mut().pop() {
            return Some(id);
        }
        let next = self.next.get() + 1;
        (next <= MAX_HOTKEY_ID).then(|| {
            self.next.set(next);
            next
        })
    }

    /// Hot key `id` was pressed: its registration hears it at the owner's
    /// next turn. The table's borrow ends before the press is reported.
    pub(super) fn pressed(&self, id: i32) {
        let presses = self.entries.borrow().get(&id).cloned();
        if let Some(presses) = presses {
            presses.press();
        }
    }

    fn insert(&self, id: i32, presses: ShortcutPresses) {
        let replaced = self.entries.borrow_mut().insert(id, presses);
        drop(replaced);
    }

    fn remove(&self, id: i32) {
        let removed = self.entries.borrow_mut().remove(&id);
        self.free.borrow_mut().push(id);
        drop(removed);
    }
}

/// Makes the hot key system of a runtime, the first time one is asked for.
type MakeSystem = Box<dyn Fn(&Rc<HotkeyTable>) -> Result<Box<dyn HotkeySystem>, i32>>;

/// One UI runtime's hot keys: the table and the system that registers
/// them, made with the first registration.
pub(crate) struct HotkeyWindow {
    table: Rc<HotkeyTable>,
    system: OnceCell<Result<Box<dyn HotkeySystem>, i32>>,
    make: MakeSystem,
}

impl Default for HotkeyWindow {
    fn default() -> Self {
        Self::over(Box::new(|table: &Rc<HotkeyTable>| {
            MessageWindow::create(table).map(|window| Box::new(window) as Box<dyn HotkeySystem>)
        }))
    }
}

impl HotkeyWindow {
    pub(super) fn over(make: MakeSystem) -> Self {
        Self {
            table: Rc::default(),
            system: OnceCell::new(),
            make,
        }
    }

    fn system(&self) -> Result<&dyn HotkeySystem, i32> {
        self.system
            .get_or_init(|| (self.make)(&self.table))
            .as_deref()
            .map_err(|code| *code)
    }

    /// Register `shortcut` reporting to `presses`: its id, or why not.
    fn register(&self, shortcut: Shortcut, presses: ShortcutPresses) -> Result<i32, ShortcutError> {
        let key = key_source(shortcut.code()).ok_or(ShortcutError::KeyNotSupported)?;
        let system = self
            .system()
            .map_err(|code| ShortcutError::Failed(SystemFailure::Os(code)))?;
        let id = self
            .table
            .mint()
            .ok_or(ShortcutError::Unsupported(Unsupported::of::<
                GlobalShortcutCapability,
            >(
                UnsupportedReason::Unavailable
            )))?;
        // In the table first: a press the system reports at once finds it.
        self.table.insert(id, presses);
        if let Err(code) = system.register(id, hotkey_modifiers(shortcut.modifiers()), key) {
            self.table.remove(id);
            let taken =
                i32::try_from(ERROR_HOTKEY_ALREADY_REGISTERED.0).is_ok_and(|taken| taken == code);
            return Err(if taken {
                ShortcutError::Taken
            } else {
                ShortcutError::Failed(SystemFailure::Os(code))
            });
        }
        Ok(id)
    }

    fn unregister(&self, id: i32) {
        if let Ok(system) = self.system() {
            system.unregister(id);
        }
        self.table.remove(id);
    }
}

/// Unregisters its hot key when dropped.
struct Registered {
    hotkeys: Rc<HotkeyWindow>,
    id: i32,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.hotkeys.unregister(self.id);
    }
}

/// [`GlobalShortcutBackend`] over `RegisterHotKey`.
pub(crate) struct WindowsShortcuts {
    hotkeys: Rc<HotkeyWindow>,
}

impl WindowsShortcuts {
    pub(crate) fn new(hotkeys: Rc<HotkeyWindow>) -> Self {
        Self { hotkeys }
    }
}

impl GlobalShortcutBackend for WindowsShortcuts {
    fn register(
        &self,
        shortcut: Shortcut,
        presses: ShortcutPresses,
        answer: Resolver<Result<(), ShortcutError>>,
    ) -> ShortcutHold {
        match self.hotkeys.register(shortcut, presses) {
            Ok(id) => {
                let _ = answer.resolve(Ok(()));
                ShortcutHold::new(Registered {
                    hotkeys: Rc::clone(&self.hotkeys),
                    id,
                })
            }
            Err(error) => {
                let _ = answer.resolve(Err(error));
                ShortcutHold::new(())
            }
        }
    }
}

/// The real hot key system: a message-only window on the owner thread whose
/// user data is the runtime's [`HotkeyTable`].
struct MessageWindow {
    window: HWND,
    /// Kept for the window's life: its procedure reads the table through
    /// the user data pointer.
    _table: Rc<HotkeyTable>,
}

impl MessageWindow {
    fn create(table: &Rc<HotkeyTable>) -> Result<Self, i32> {
        let class = w!("FluiSystemHotkeys");
        let failed = || super::os_code(windows::core::Error::from_thread().code());
        // SAFETY: the module's own instance; nothing is passed.
        let instance =
            unsafe { GetModuleHandleW(None) }.map_err(|error| super::os_code(error.code()))?;
        let description = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>())
                .expect("BUG: WNDCLASSEXW is a few dozen bytes"),
            lpfnWndProc: Some(hotkey_procedure),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        // SAFETY: `description` outlives the call and names a procedure of
        // this module; a second registration of the identical class fails
        // and leaves the first in place.
        let _ = unsafe { RegisterClassExW(&raw const description) };
        // SAFETY: a message-only window of that class on this, the owner,
        // thread, with no parameter of ours.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                None,
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
        }
        .map_err(|_| failed())?;
        // SAFETY: the window is this thread's, just made; the pointer stored
        // is the table this value keeps alive until it destroys the window.
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, Rc::as_ptr(table) as isize) };
        Ok(Self {
            window,
            _table: Rc::clone(table),
        })
    }
}

impl HotkeySystem for MessageWindow {
    fn register(&self, id: i32, modifiers: HOT_KEY_MODIFIERS, key: KeySource) -> Result<(), i32> {
        let virtual_key = match key {
            KeySource::Fixed(virtual_key) => virtual_key,
            // SAFETY: plain values; the layout is this thread's active one.
            KeySource::Scan(scan) => unsafe {
                MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK, Some(GetKeyboardLayout(0)))
            },
        };
        if virtual_key == 0 {
            return Err(
                i32::try_from(windows::Win32::Foundation::ERROR_INVALID_PARAMETER.0)
                    .unwrap_or(i32::MAX),
            );
        }
        // SAFETY: the window is this thread's; a hot key belongs to it until
        // `unregister` or its destruction.
        unsafe { RegisterHotKey(Some(self.window), id, modifiers, virtual_key) }
            .map_err(|error| super::os_code(error.code()))
    }

    fn unregister(&self, id: i32) {
        // SAFETY: as above.
        let _ = unsafe { UnregisterHotKey(Some(self.window), id) };
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        // SAFETY: the window is this thread's (a runtime-local slot drops on
        // the owner thread); destroying it ends its hot keys and every later
        // dispatch to its procedure.
        let _ = unsafe { DestroyWindow(self.window) };
    }
}

/// The hot key window's procedure: a `WM_HOTKEY` is handed to the table.
unsafe extern "system" fn hotkey_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_HOTKEY {
        // SAFETY: the user data is null until `create` stores the table,
        // which its `MessageWindow` keeps alive until it destroys the window;
        // this procedure runs on the owner thread, the table's.
        let table = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const HotkeyTable;
        // SAFETY: as above.
        if let (Some(table), Ok(id)) = (unsafe { table.as_ref() }, i32::try_from(wparam.0)) {
            table.pressed(id);
        }
        return LRESULT(0);
    }
    // SAFETY: every other message goes to the default procedure with the
    // arguments the system passed.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use flui_platform::capability::Abandoned;
    use flui_platform::keyboard::{Code, Modifiers};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };

    use super::{
        HotkeySystem, HotkeyTable, HotkeyWindow, KeySource, WindowsShortcuts, hotkey_modifiers,
        key_source,
    };
    use crate::shortcut::{GlobalShortcuts, Shortcut, ShortcutError, ShortcutResult};
    use crate::testing::win32::{FakeHotkeys, Held, TestOwner};

    fn window_over(fake: FakeHotkeys) -> (Rc<HotkeyWindow>, Held) {
        let held = Rc::clone(&fake.held);
        let fake = RefCell::new(Some(fake));
        let window = HotkeyWindow::over(Box::new(move |_: &Rc<HotkeyTable>| {
            Ok(Box::new(fake.borrow_mut().take().expect("made once")) as Box<dyn HotkeySystem>)
        }));
        (Rc::new(window), held)
    }

    #[test]
    fn a_chord_names_a_physical_key_and_never_repeats() {
        assert_eq!(key_source(Code::KeyQ), Some(KeySource::Scan(0x10)));
        assert_eq!(key_source(Code::Digit0), Some(KeySource::Scan(0x0B)));
        assert_eq!(key_source(Code::F13), Some(KeySource::Fixed(0x7C)));
        assert_eq!(
            key_source(Code::MediaPlayPause),
            Some(KeySource::Fixed(0xB3))
        );
        assert_eq!(key_source(Code::Fn), None, "a key a hot key cannot name");
        assert_eq!(
            hotkey_modifiers(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::META),
            MOD_CONTROL | MOD_SHIFT | MOD_WIN | MOD_NOREPEAT
        );
    }

    #[test]
    fn a_press_runs_the_listener_at_the_next_turn_until_the_registration_drops() {
        let owner = TestOwner::new();
        let (hotkeys, held) = window_over(FakeHotkeys::default());
        let ctx = owner.ctx(None);
        let shortcuts = GlobalShortcuts::new(WindowsShortcuts::new(Rc::clone(&hotkeys)), &ctx);
        let shortcut = Shortcut::try_new(Modifiers::CONTROL, Code::KeyK).expect("a chord");
        let pressed = Rc::new(Cell::new(0_u32));
        let counting = Rc::clone(&pressed);
        let answered: Rc<RefCell<Option<Result<ShortcutResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&answered);
        shortcuts
            .register(shortcut, move || counting.set(counting.get() + 1))
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| answered.borrow().is_some());
        let registration = answered
            .borrow_mut()
            .take()
            .expect("answered")
            .expect("not abandoned")
            .expect("registered");
        let [(id, modifiers, key)] = held.borrow().as_slice().to_owned()[..] else {
            panic!("one hot key held: {:?}", held.borrow());
        };
        assert_eq!(modifiers, MOD_CONTROL | MOD_NOREPEAT);
        assert_eq!(key, KeySource::Scan(0x25));

        // What the window procedure does with a `WM_HOTKEY` of `id`.
        let table = &hotkeys.table;
        table.pressed(id);
        assert_eq!(pressed.get(), 0, "never inside the window procedure");
        owner.turn();
        assert_eq!(pressed.get(), 1);

        table.pressed(id);
        drop(registration);
        owner.turn();
        assert_eq!(pressed.get(), 1, "a press after the drop runs nothing");
        assert_eq!(*held.borrow(), [], "the drop unregistered");
    }

    #[test]
    fn a_chord_another_application_holds_is_taken() {
        let owner = TestOwner::new();
        let (hotkeys, held) = window_over(FakeHotkeys {
            refuse: Some(1409),
            ..FakeHotkeys::default()
        });
        let ctx = owner.ctx(None);
        let shortcuts = GlobalShortcuts::new(WindowsShortcuts::new(hotkeys), &ctx);
        let shortcut = Shortcut::try_new(Modifiers::ALT, Code::Space).expect("a chord");
        let answered: Rc<RefCell<Option<Result<ShortcutResult, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&answered);
        shortcuts
            .register(shortcut, || {})
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| answered.borrow().is_some());
        assert!(matches!(
            answered.borrow_mut().take(),
            Some(Ok(Err(ShortcutError::Taken)))
        ));
        assert_eq!(*held.borrow(), []);
    }
}
