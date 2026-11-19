//! Global shortcuts over `RegisterHotKey`, heard by a message-only window
//! of the UI runtime's own on the owner thread.
//!
//! The owner thread's loop dispatches every window of the thread, so the
//! window's `WM_HOTKEY` arrives without a hook in the host (ADR-0154 §9,
//! item 4). Windows has no shortcut settings an application binds through:
//! a shortcut is bound to the trigger it prefers, or not at all, and the
//! [`ShortcutEvent::Bound`] list names the ones bound. One whose chord
//! another application holds, or that prefers none, is left out of it. A
//! trigger's key is its virtual key: a letter's or digit's is its ASCII
//! upper case, which names the key that types it on the active layout, and
//! a named key's is fixed. `MOD_NOREPEAT` keeps a held chord one press, and
//! Windows reports no release, so a binding hears
//! [`ShortcutEvent::Pressed`] only.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use flui_platform::SystemFailure;
use flui_platform::input::keyboard::{Modifiers, NamedKey};
use rustc_hash::FxHashMap;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW, HWND_MESSAGE,
    RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE, WM_HOTKEY, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::w;

use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::shortcuts::{
    BoundShortcut, Shortcut, ShortcutBackend, ShortcutError, ShortcutEvent, ShortcutFeed,
    ShortcutHold, ShortcutId, Trigger, TriggerKey, function_key,
};

/// The highest id an application's hot key may have.
const MAX_HOTKEY_ID: i32 = 0xBFFF;

/// The virtual key and the name Windows shows of a named key a trigger
/// takes.
fn named_key(key: NamedKey) -> Option<(u32, String)> {
    let (virtual_key, name) = match key {
        NamedKey::Enter => (0x0D, "Enter"),
        NamedKey::Escape => (0x1B, "Esc"),
        NamedKey::Tab => (0x09, "Tab"),
        NamedKey::Backspace => (0x08, "Backspace"),
        NamedKey::Insert => (0x2D, "Ins"),
        NamedKey::Delete => (0x2E, "Del"),
        NamedKey::Home => (0x24, "Home"),
        NamedKey::End => (0x23, "End"),
        NamedKey::PageUp => (0x21, "PgUp"),
        NamedKey::PageDown => (0x22, "PgDn"),
        NamedKey::ArrowLeft => (0x25, "Left"),
        NamedKey::ArrowUp => (0x26, "Up"),
        NamedKey::ArrowRight => (0x27, "Right"),
        NamedKey::ArrowDown => (0x28, "Down"),
        _ => {
            let number = function_key(key)?;
            return Some((0x6F + u32::from(number), format!("F{number}")));
        }
    };
    Some((virtual_key, name.to_owned()))
}

/// A trigger as `RegisterHotKey` takes it: its modifiers with
/// `MOD_NOREPEAT`, its virtual key, and the chord as Windows shows it
/// (`Ctrl+Shift+A`).
pub(super) fn hotkey(trigger: &Trigger) -> (HOT_KEY_MODIFIERS, u32, String) {
    let mut modifiers = MOD_NOREPEAT;
    let mut shown = String::new();
    for (modifier, flag, name) in [
        (Modifiers::CONTROL, MOD_CONTROL, "Ctrl+"),
        (Modifiers::ALT, MOD_ALT, "Alt+"),
        (Modifiers::SHIFT, MOD_SHIFT, "Shift+"),
        (Modifiers::META, MOD_WIN, "Win+"),
    ] {
        if trigger.modifiers().contains(modifier) {
            modifiers |= flag;
            shown.push_str(name);
        }
    }
    let virtual_key = match trigger.key() {
        TriggerKey::Character(character) => {
            let upper = character.to_ascii_uppercase();
            shown.push(upper);
            u32::from(upper)
        }
        TriggerKey::Named(named) => {
            let (virtual_key, name) =
                named_key(named).expect("BUG: a trigger holds a key every system names");
            shown.push_str(&name);
            virtual_key
        }
    };
    (modifiers, virtual_key, shown)
}

/// The hot keys as the provider reaches them, on the owner thread.
pub(crate) trait HotkeySystem: 'static {
    /// Register hot key `id`; the failure is the Win32 error.
    fn register(&self, id: i32, modifiers: HOT_KEY_MODIFIERS, virtual_key: u32) -> Result<(), i32>;

    /// Unregister hot key `id`.
    fn unregister(&self, id: i32);
}

/// Where each registered hot key's presses go, by id, and the last id
/// handed out.
#[derive(Default)]
pub(crate) struct HotkeyTable {
    entries: RefCell<FxHashMap<i32, (ShortcutFeed, ShortcutId)>>,
    last: Cell<i32>,
}

impl HotkeyTable {
    /// `count` ids no hot key of this runtime has had: never handed out
    /// again, so a `WM_HOTKEY` still queued for a dropped binding finds no
    /// entry rather than a later one. `None`, and nothing handed out, when
    /// fewer than `count` are left of [`MAX_HOTKEY_ID`].
    fn mint(&self, count: usize) -> Option<std::ops::RangeInclusive<i32>> {
        let first = self.last.get().checked_add(1)?;
        let last = self.last.get().checked_add(i32::try_from(count).ok()?)?;
        (last <= MAX_HOTKEY_ID).then(|| {
            self.last.set(last);
            first..=last
        })
    }

    /// Hot key `id` was pressed: its binding hears it. The table's borrow
    /// ends before the press is sent.
    pub(super) fn pressed(&self, id: i32) {
        let entry = self.entries.borrow().get(&id).cloned();
        if let Some((feed, shortcut)) = entry {
            feed.send(ShortcutEvent::Pressed(shortcut));
        }
    }

    fn insert(&self, id: i32, feed: ShortcutFeed, shortcut: ShortcutId) {
        let replaced = self.entries.borrow_mut().insert(id, (feed, shortcut));
        drop(replaced);
    }

    fn remove(&self, id: i32) {
        let removed = self.entries.borrow_mut().remove(&id);
        drop(removed);
    }
}

/// Makes the hot key system of a runtime, the first time one is asked for.
type MakeSystem = Box<dyn Fn(&Rc<HotkeyTable>) -> Result<Box<dyn HotkeySystem>, i32>>;

/// One UI runtime's hot keys: the table and the system that registers
/// them, made with the first binding.
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

    /// Register each of `shortcuts` that prefers a trigger, its presses
    /// sent to `feed`: the ids registered and the shortcuts bound, or why
    /// none could be.
    fn bind(
        &self,
        shortcuts: Vec<Shortcut>,
        feed: &ShortcutFeed,
    ) -> Result<(Vec<i32>, Vec<BoundShortcut>), ShortcutError> {
        let system = self
            .system()
            .map_err(|code| ShortcutError::Failed(SystemFailure::Os(code)))?;
        let ids = self
            .table
            .mint(shortcuts.len())
            .ok_or(ShortcutError::Exhausted)?;
        let mut registered = Vec::new();
        let mut bound = Vec::new();
        for (id, shortcut) in ids.zip(shortcuts) {
            let Some(trigger) = &shortcut.preferred else {
                continue;
            };
            let (modifiers, virtual_key, shown) = hotkey(trigger);
            // In the table first: a press the system reports at once finds it.
            self.table.insert(id, feed.clone(), shortcut.id.clone());
            match system.register(id, modifiers, virtual_key) {
                Ok(()) => {
                    registered.push(id);
                    bound.push(BoundShortcut::new(shortcut.id, Some(shown)));
                }
                // Another application, or this binding, holds the chord:
                // this shortcut is not bound.
                Err(_) => self.table.remove(id),
            }
        }
        Ok((registered, bound))
    }

    fn unregister(&self, id: i32) {
        if let Ok(system) = self.system() {
            system.unregister(id);
        }
        self.table.remove(id);
    }
}

/// Unregisters a binding's hot keys when dropped.
struct Registered {
    hotkeys: Rc<HotkeyWindow>,
    ids: Vec<i32>,
}

impl Drop for Registered {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            self.hotkeys.unregister(id);
        }
    }
}

/// [`ShortcutBackend`] over `RegisterHotKey`.
pub(crate) struct WindowsShortcuts {
    hotkeys: Rc<HotkeyWindow>,
}

impl WindowsShortcuts {
    pub(crate) fn new(hotkeys: Rc<HotkeyWindow>) -> Self {
        Self { hotkeys }
    }
}

impl ShortcutBackend for WindowsShortcuts {
    fn bind(&self, shortcuts: Vec<Shortcut>, feed: ShortcutFeed) -> ShortcutHold {
        match self.hotkeys.bind(shortcuts, &feed) {
            Ok((ids, bound)) => {
                let hold = Registered {
                    hotkeys: Rc::clone(&self.hotkeys),
                    ids,
                };
                feed.send(ShortcutEvent::Bound(bound));
                ShortcutHold::new(hold)
            }
            Err(error) => {
                feed.send(ShortcutEvent::Ended(error));
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
    fn register(&self, id: i32, modifiers: HOT_KEY_MODIFIERS, virtual_key: u32) -> Result<(), i32> {
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

/// The hot key window's procedure: a `WM_HOTKEY` is handed to the table. A
/// panic in it is contained and the message answered 0.
unsafe extern "system" fn hotkey_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    os_entry(
        "the global shortcuts' window",
        (window, message, wparam, lparam),
        AfterPanic::Answer(|_| LRESULT(0)),
        || hotkey_message(window, message, wparam, lparam),
    )
}

fn hotkey_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_platform::capability::Abandoned;
    use flui_platform::input::keyboard::{Key, Modifiers, NamedKey};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };

    use super::{HotkeySystem, HotkeyTable, HotkeyWindow, WindowsShortcuts, hotkey};
    use crate::shortcuts::{
        BoundShortcut, GlobalShortcuts, Shortcut, ShortcutBinding, ShortcutError, ShortcutEvent,
        ShortcutId, Trigger,
    };
    use crate::testing::win32::{FakeHotkeys, Held, TestOwner};

    fn window_over(fake: FakeHotkeys) -> (Rc<HotkeyWindow>, Held) {
        let held = Rc::clone(&fake.held);
        let fake = RefCell::new(Some(fake));
        let window = HotkeyWindow::over(Box::new(move |_: &Rc<HotkeyTable>| {
            Ok(Box::new(fake.borrow_mut().take().expect("made once")) as Box<dyn HotkeySystem>)
        }));
        (Rc::new(window), held)
    }

    fn id(name: &str) -> ShortcutId {
        ShortcutId::try_new(name).expect("an id")
    }

    fn trigger(modifiers: Modifiers, key: &Key) -> Trigger {
        Trigger::try_new(modifiers, key).expect("a trigger")
    }

    /// The shortcut `name` preferring `modifiers` and `key`.
    fn shortcut(name: &str, modifiers: Modifiers, key: &Key) -> Shortcut {
        Shortcut::new(id(name), "Does a thing").preferred(trigger(modifiers, key))
    }

    /// The next event `binding` hears, after the owner's turns.
    fn next(owner: &TestOwner, binding: &ShortcutBinding) -> ShortcutEvent {
        let landed: Rc<RefCell<Option<Result<ShortcutEvent, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&landed);
        binding
            .next()
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| landed.borrow().is_some());
        let event = landed.borrow_mut().take();
        event.expect("landed").expect("not abandoned")
    }

    #[test]
    fn a_trigger_becomes_a_virtual_key_that_never_repeats_and_its_windows_name() {
        let ctrl_shift = Modifiers::CONTROL | Modifiers::SHIFT;
        let cases = [
            (
                trigger(ctrl_shift, &Key::character("k")),
                (MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, 0x4B, "Ctrl+Shift+K"),
            ),
            (
                trigger(Modifiers::META, &Key::character("7")),
                (MOD_WIN | MOD_NOREPEAT, 0x37, "Win+7"),
            ),
            (
                trigger(Modifiers::ALT, &Key::Named(NamedKey::F24)),
                (MOD_ALT | MOD_NOREPEAT, 0x87, "Alt+F24"),
            ),
            (
                trigger(Modifiers::NONE, &Key::Named(NamedKey::PageDown)),
                (MOD_NOREPEAT, 0x22, "PgDn"),
            ),
        ];
        for (trigger, (modifiers, virtual_key, shown)) in cases {
            assert_eq!(
                hotkey(&trigger),
                (modifiers, virtual_key, shown.to_owned()),
                "{trigger:?}"
            );
        }
    }

    #[test]
    fn a_binding_hears_what_was_bound_then_each_press_until_it_drops() {
        let owner = TestOwner::new();
        let (hotkeys, held) = window_over(FakeHotkeys::default());
        let ctx = owner.ctx(None);
        let shortcuts = GlobalShortcuts::new(WindowsShortcuts::new(Rc::clone(&hotkeys)), &ctx);
        let binding = shortcuts.bind(vec![
            shortcut("record", Modifiers::CONTROL, &Key::character("k")),
            // Windows has no settings to choose a trigger in.
            Shortcut::new(id("unchosen"), "Waits for a trigger"),
        ]);
        assert_eq!(
            next(&owner, &binding),
            ShortcutEvent::Bound(vec![BoundShortcut::new(
                id("record"),
                Some("Ctrl+K".to_owned())
            )])
        );
        let [(hotkey_id, modifiers, virtual_key)] = held.borrow().as_slice().to_owned()[..] else {
            panic!("one hot key held: {:?}", held.borrow());
        };
        assert_eq!((modifiers, virtual_key), (MOD_CONTROL | MOD_NOREPEAT, 0x4B));

        // What the window procedure does with a `WM_HOTKEY` of the id.
        hotkeys.table.pressed(hotkey_id);
        assert_eq!(next(&owner, &binding), ShortcutEvent::Pressed(id("record")));

        drop(binding);
        assert_eq!(*held.borrow(), [], "the drop unregistered");
        assert!(
            hotkeys.table.entries.borrow().is_empty(),
            "a press after the drop finds no binding"
        );
    }

    #[test]
    fn a_chord_another_application_holds_is_left_out() {
        let owner = TestOwner::new();
        let (hotkeys, held) = window_over(FakeHotkeys {
            refuse: Some(1409),
            ..FakeHotkeys::default()
        });
        let ctx = owner.ctx(None);
        let shortcuts = GlobalShortcuts::new(WindowsShortcuts::new(hotkeys), &ctx);
        let binding = shortcuts.bind(vec![shortcut(
            "launcher",
            Modifiers::ALT,
            &Key::Named(NamedKey::Enter),
        )]);
        assert_eq!(next(&owner, &binding), ShortcutEvent::Bound(Vec::new()));
        assert_eq!(*held.borrow(), []);
    }

    #[test]
    fn a_press_queued_for_a_dropped_binding_never_reaches_the_binding_after_it() {
        let owner = TestOwner::new();
        let (hotkeys, held) = window_over(FakeHotkeys::default());
        let ctx = owner.ctx(None);
        let shortcuts = GlobalShortcuts::new(WindowsShortcuts::new(Rc::clone(&hotkeys)), &ctx);
        let chord = || shortcut("same", Modifiers::CONTROL, &Key::character("a"));

        let first = shortcuts.bind(vec![chord()]);
        let first_id = held.borrow()[0].0;
        drop(first);
        // The second binds the same chord at once, before the queued
        // `WM_HOTKEY` of the first is dispatched.
        let second = shortcuts.bind(vec![chord()]);
        let second_id = held.borrow()[0].0;
        assert_ne!(first_id, second_id, "an id is never handed out twice");
        assert!(matches!(next(&owner, &second), ShortcutEvent::Bound(_)));

        // The late press of the dropped binding reaches nobody; the
        // second's press is the first event the second hears after `Bound`.
        hotkeys.table.pressed(first_id);
        hotkeys.table.pressed(second_id);
        assert_eq!(next(&owner, &second), ShortcutEvent::Pressed(id("same")));
        assert_eq!(second.dropped(), 0);
        let landed: Rc<RefCell<Option<Result<ShortcutEvent, Abandoned>>>> = Rc::default();
        let into = Rc::clone(&landed);
        second
            .next()
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn();
        assert!(landed.borrow().is_none(), "one press, not two");
    }

    #[test]
    fn a_runtime_that_numbered_every_hot_key_ends_the_next_binding_for_good() {
        let owner = TestOwner::new();
        let (hotkeys, held) = window_over(FakeHotkeys::default());
        hotkeys.table.last.set(super::MAX_HOTKEY_ID - 1);
        let ctx = owner.ctx(None);
        let shortcuts = GlobalShortcuts::new(WindowsShortcuts::new(hotkeys), &ctx);
        let two = vec![
            shortcut("one", Modifiers::CONTROL, &Key::character("y")),
            shortcut("two", Modifiers::CONTROL, &Key::character("z")),
        ];
        let binding = shortcuts.bind(two);
        assert_eq!(
            next(&owner, &binding),
            ShortcutEvent::Ended(ShortcutError::Exhausted),
            "one number is left, the binding needs two: none is handed out"
        );
        assert_eq!(*held.borrow(), []);
        let one = shortcuts.bind(vec![shortcut(
            "one",
            Modifiers::CONTROL,
            &Key::character("y"),
        )]);
        assert!(matches!(next(&owner, &one), ShortcutEvent::Bound(list) if list.len() == 1));
    }
}
