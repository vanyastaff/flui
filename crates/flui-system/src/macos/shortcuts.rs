//! Global shortcuts over Carbon's hot keys (`RegisterEventHotKey`), which
//! reach the application while another one has the focus, need neither the
//! Accessibility permission nor an exemption from the sandbox, and report
//! the release as well as the press.
//!
//! The application event target's handler, installed with the runtime's
//! first binding, hears each hot key on the main run loop. macOS has no
//! shortcut settings an application binds through: a shortcut is bound to
//! the trigger it prefers, or not at all, and the [`ShortcutEvent::Bound`]
//! list names the ones bound. One whose chord another application holds,
//! that prefers none, or whose key has no key code (F21 to F24) is left out
//! of it. A trigger's key is the key code of its position on an ANSI
//! keyboard; the modifiers map Control, Option (`ALT`), Shift and Command
//! (`META`). A hot key number is never handed out twice in a runtime's
//! life, so an event still queued for a dropped binding finds no shortcut
//! rather than a later one.

use std::cell::{Cell, RefCell};

use flui_platform::keyboard::{Modifiers, NamedKey};
use rustc_hash::FxHashMap;

use crate::shortcuts::{
    BoundShortcut, Shortcut, ShortcutError, ShortcutEvent, ShortcutFeed, ShortcutId, Trigger,
    TriggerKey, function_key,
};

/// Carbon's modifier flags (`Events.h`).
mod flag {
    pub(super) const COMMAND: u32 = 1 << 8;
    pub(super) const SHIFT: u32 = 1 << 9;
    pub(super) const OPTION: u32 = 1 << 11;
    pub(super) const CONTROL: u32 = 1 << 12;
}

/// The key code (`kVK_ANSI_*`) of an ASCII letter or digit's position.
fn character_key(character: char) -> Option<u32> {
    let code = match character.to_ascii_lowercase() {
        'a' => 0x00,
        's' => 0x01,
        'd' => 0x02,
        'f' => 0x03,
        'h' => 0x04,
        'g' => 0x05,
        'z' => 0x06,
        'x' => 0x07,
        'c' => 0x08,
        'v' => 0x09,
        'b' => 0x0B,
        'q' => 0x0C,
        'w' => 0x0D,
        'e' => 0x0E,
        'r' => 0x0F,
        'y' => 0x10,
        't' => 0x11,
        '1' => 0x12,
        '2' => 0x13,
        '3' => 0x14,
        '4' => 0x15,
        '6' => 0x16,
        '5' => 0x17,
        '9' => 0x19,
        '7' => 0x1A,
        '8' => 0x1C,
        '0' => 0x1D,
        'o' => 0x1F,
        'u' => 0x20,
        'i' => 0x22,
        'p' => 0x23,
        'l' => 0x25,
        'j' => 0x26,
        'k' => 0x28,
        'n' => 0x2D,
        'm' => 0x2E,
        _ => return None,
    };
    Some(code)
}

/// The key code (`kVK_*`) and the name macOS shows of a named key a
/// trigger takes; `None` for F21 to F24, which have no key code.
fn named_key(key: NamedKey) -> Option<(u32, String)> {
    let (code, name) = match key {
        NamedKey::Enter => (0x24, "↩"),
        NamedKey::Escape => (0x35, "⎋"),
        NamedKey::Tab => (0x30, "⇥"),
        NamedKey::Backspace => (0x33, "⌫"),
        NamedKey::Insert => (0x72, "Help"),
        NamedKey::Delete => (0x75, "⌦"),
        NamedKey::Home => (0x73, "↖"),
        NamedKey::End => (0x77, "↘"),
        NamedKey::PageUp => (0x74, "⇞"),
        NamedKey::PageDown => (0x79, "⇟"),
        NamedKey::ArrowLeft => (0x7B, "←"),
        NamedKey::ArrowRight => (0x7C, "→"),
        NamedKey::ArrowDown => (0x7D, "↓"),
        NamedKey::ArrowUp => (0x7E, "↑"),
        _ => {
            let number = function_key(key)?;
            const CODES: [u32; 20] = [
                0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B,
                0x71, 0x6A, 0x40, 0x4F, 0x50, 0x5A,
            ];
            let code = *CODES.get(usize::from(number).checked_sub(1)?)?;
            return Some((code, format!("F{number}")));
        }
    };
    Some((code, name.to_owned()))
}

/// A trigger as `RegisterEventHotKey` takes it: its key code, its modifier
/// flags, and the chord as macOS shows it (`⌃⌥⇧⌘A`); `None` for a key with
/// no key code.
pub(super) fn hot_key(trigger: &Trigger) -> Option<(u32, u32, String)> {
    let mut flags = 0;
    let mut shown = String::new();
    for (modifier, carbon, symbol) in [
        (Modifiers::CONTROL, flag::CONTROL, '⌃'),
        (Modifiers::ALT, flag::OPTION, '⌥'),
        (Modifiers::SHIFT, flag::SHIFT, '⇧'),
        (Modifiers::META, flag::COMMAND, '⌘'),
    ] {
        if trigger.modifiers().contains(modifier) {
            flags |= carbon;
            shown.push(symbol);
        }
    }
    let code = match trigger.key() {
        TriggerKey::Character(character) => {
            shown.push(character.to_ascii_uppercase());
            character_key(character)?
        }
        TriggerKey::Named(named) => {
            let (code, name) = named_key(named)?;
            shown.push_str(&name);
            code
        }
    };
    Some((code, flags, shown))
}

/// Carbon's hot keys as the provider reaches them, on the main thread.
pub(super) trait HotKeySystem {
    /// Register hot key `id`; the failure is the `OSStatus`.
    fn register(&self, id: u32, code: u32, flags: u32) -> Result<(), i32>;

    /// Unregister hot key `id`.
    fn unregister(&self, id: u32);
}

/// Where each registered hot key's events go, by number, and the last
/// number handed out.
#[derive(Default)]
pub(crate) struct HotKeyTable {
    entries: RefCell<FxHashMap<u32, (ShortcutFeed, ShortcutId)>>,
    last: Cell<u32>,
}

impl HotKeyTable {
    /// `count` numbers no hot key of this runtime has had; `None`, and
    /// nothing handed out, when fewer are left.
    fn mint(&self, count: usize) -> Option<std::ops::RangeInclusive<u32>> {
        let first = self.last.get().checked_add(1)?;
        let last = self.last.get().checked_add(u32::try_from(count).ok()?)?;
        self.last.set(last);
        Some(first..=last)
    }

    /// Hot key `id` was pressed or released: its binding hears it, and
    /// whether the runtime has such a hot key. The table's borrow ends
    /// before the event is sent.
    pub(super) fn heard(&self, id: u32, pressed: bool) -> bool {
        let entry = self.entries.borrow().get(&id).cloned();
        let Some((feed, shortcut)) = entry else {
            return false;
        };
        feed.send(if pressed {
            ShortcutEvent::Pressed(shortcut)
        } else {
            ShortcutEvent::Released(shortcut)
        });
        true
    }

    fn insert(&self, id: u32, feed: ShortcutFeed, shortcut: ShortcutId) {
        let replaced = self.entries.borrow_mut().insert(id, (feed, shortcut));
        drop(replaced);
    }

    fn remove(&self, id: u32) {
        let removed = self.entries.borrow_mut().remove(&id);
        drop(removed);
    }
}

/// Register each of `shortcuts` that prefers a trigger with a key code, its
/// events sent to `feed`: the numbers registered and the shortcuts bound,
/// or why none could be.
pub(super) fn bind(
    table: &HotKeyTable,
    system: &dyn HotKeySystem,
    shortcuts: Vec<Shortcut>,
    feed: &ShortcutFeed,
) -> Result<(Vec<u32>, Vec<BoundShortcut>), ShortcutError> {
    let ids = table
        .mint(shortcuts.len())
        .ok_or(ShortcutError::Exhausted)?;
    let mut registered = Vec::new();
    let mut bound = Vec::new();
    for (id, shortcut) in ids.zip(shortcuts) {
        let Some((code, flags, shown)) = shortcut.preferred.as_ref().and_then(hot_key) else {
            continue;
        };
        // In the table first: an event the system reports at once finds it.
        table.insert(id, feed.clone(), shortcut.id.clone());
        match system.register(id, code, flags) {
            Ok(()) => {
                registered.push(id);
                bound.push(BoundShortcut::new(shortcut.id, Some(shown)));
            }
            // Another application, or this binding, holds the chord: this
            // shortcut is not bound.
            Err(_) => table.remove(id),
        }
    }
    Ok((registered, bound))
}

/// Unregister hot key `id` and forget its binding.
pub(super) fn unbind(table: &HotKeyTable, system: &dyn HotKeySystem, id: u32) {
    system.unregister(id);
    table.remove(id);
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::{HotKeys, MacShortcuts};

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "Carbon's hot keys and their event handler")]
mod appkit {
    use std::cell::{OnceCell, RefCell};
    use std::ffi::c_void;
    use std::rc::Rc;

    use flui_platform::SystemFailure;
    use objc2::MainThreadMarker;
    use rustc_hash::FxHashMap;

    use super::{HotKeySystem, HotKeyTable, bind, unbind};
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::shortcuts::{
        Shortcut, ShortcutBackend, ShortcutError, ShortcutEvent, ShortcutFeed, ShortcutHold,
    };

    type OsStatus = i32;
    type EventTargetRef = *mut c_void;
    type EventHandlerRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;
    type EventRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type EventHandlerUpp = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OsStatus;

    /// `EventHotKeyID`: the application's signature and the key's number.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct EventHotKeyId {
        signature: u32,
        id: u32,
    }

    /// `EventTypeSpec`.
    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    /// `'flui'`, the signature every hot key of the crate carries.
    const SIGNATURE: u32 = u32::from_be_bytes(*b"flui");
    /// `kEventClassKeyboard`.
    const KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
    /// `kEventHotKeyPressed`, `kEventHotKeyReleased`.
    const PRESSED: u32 = 5;
    const RELEASED: u32 = 6;
    /// `kEventParamDirectObject`, `typeEventHotKeyID`.
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
    const HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");
    /// `eventNotHandledErr`: the next handler hears the event.
    const NOT_HANDLED: OsStatus = -9874;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> EventTargetRef;
        fn InstallEventHandler(
            target: EventTargetRef,
            handler: EventHandlerUpp,
            count: usize,
            types: *const EventTypeSpec,
            user_data: *mut c_void,
            out_ref: *mut EventHandlerRef,
        ) -> OsStatus;
        fn RemoveEventHandler(handler: EventHandlerRef) -> OsStatus;
        fn RegisterEventHotKey(
            code: u32,
            modifiers: u32,
            id: EventHotKeyId,
            target: EventTargetRef,
            options: u32,
            out_ref: *mut EventHotKeyRef,
        ) -> OsStatus;
        fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OsStatus;
        fn GetEventKind(event: EventRef) -> u32;
        fn GetEventParameter(
            event: EventRef,
            name: u32,
            desired_type: u32,
            actual_type: *mut u32,
            size: usize,
            actual_size: *mut usize,
            data: *mut c_void,
        ) -> OsStatus;
    }

    /// The application event target's handler of the runtime's hot keys,
    /// whose user data is the runtime's [`HotKeyTable`].
    extern "C" fn hot_key_handler(
        call: EventHandlerCallRef,
        event: EventRef,
        table: *mut c_void,
    ) -> OsStatus {
        os_entry(
            "Carbon hot key handler",
            (call, event, table),
            AfterPanic::Answer(|_| NOT_HANDLED),
            || {
                let mut id = EventHotKeyId::default();
                // SAFETY: `event` is the event Carbon is dispatching; the
                // hot key id parameter is an `EventHotKeyID`, and `id`, of
                // that size, outlives the call.
                let status = unsafe {
                    GetEventParameter(
                        event,
                        DIRECT_OBJECT,
                        HOT_KEY_ID,
                        std::ptr::null_mut(),
                        std::mem::size_of::<EventHotKeyId>(),
                        std::ptr::null_mut(),
                        (&raw mut id).cast::<c_void>(),
                    )
                };
                // SAFETY: as above.
                let kind = unsafe { GetEventKind(event) };
                if status != 0 || id.signature != SIGNATURE || !matches!(kind, PRESSED | RELEASED) {
                    return NOT_HANDLED;
                }
                // SAFETY: the user data is the table the installed handler's
                // owner keeps alive until it removes the handler, and this
                // runs on the main thread, where the table is used.
                let table = unsafe { &*table.cast::<HotKeyTable>() };
                if table.heard(id.id, kind == PRESSED) {
                    0
                } else {
                    NOT_HANDLED
                }
            },
        )
    }

    /// The real hot key system: the handler on the application event
    /// target, and each registered key's reference.
    struct CarbonHotKeys {
        handler: EventHandlerRef,
        keys: RefCell<FxHashMap<u32, EventHotKeyRef>>,
        /// Kept while the handler is installed: its user data points here.
        _table: Rc<HotKeyTable>,
    }

    impl CarbonHotKeys {
        fn install(table: &Rc<HotKeyTable>) -> Result<Self, OsStatus> {
            let types = [
                EventTypeSpec {
                    event_class: KEYBOARD,
                    event_kind: PRESSED,
                },
                EventTypeSpec {
                    event_class: KEYBOARD,
                    event_kind: RELEASED,
                },
            ];
            let mut handler: EventHandlerRef = std::ptr::null_mut();
            // SAFETY: on the main thread; `types` outlives the call, and the
            // user data is the table this value keeps alive until it removes
            // the handler.
            let status = unsafe {
                InstallEventHandler(
                    GetApplicationEventTarget(),
                    hot_key_handler,
                    types.len(),
                    types.as_ptr(),
                    Rc::as_ptr(table).cast_mut().cast::<c_void>(),
                    &raw mut handler,
                )
            };
            if status != 0 {
                return Err(status);
            }
            Ok(Self {
                handler,
                keys: RefCell::default(),
                _table: Rc::clone(table),
            })
        }
    }

    impl Drop for CarbonHotKeys {
        fn drop(&mut self) {
            for (_, key) in self.keys.borrow_mut().drain() {
                // SAFETY: a key this value registered and did not unregister.
                let _ = unsafe { UnregisterEventHotKey(key) };
            }
            // SAFETY: the handler this value installed.
            let _ = unsafe { RemoveEventHandler(self.handler) };
        }
    }

    impl HotKeySystem for CarbonHotKeys {
        fn register(&self, id: u32, code: u32, flags: u32) -> Result<(), i32> {
            let mut key: EventHotKeyRef = std::ptr::null_mut();
            // SAFETY: on the main thread; the reference is written into
            // `key`, which outlives the call.
            let status = unsafe {
                RegisterEventHotKey(
                    code,
                    flags,
                    EventHotKeyId {
                        signature: SIGNATURE,
                        id,
                    },
                    GetApplicationEventTarget(),
                    0,
                    &raw mut key,
                )
            };
            if status != 0 {
                return Err(status);
            }
            let replaced = self.keys.borrow_mut().insert(id, key);
            debug_assert!(replaced.is_none(), "a hot key number is handed out once");
            Ok(())
        }

        fn unregister(&self, id: u32) {
            let key = self.keys.borrow_mut().remove(&id);
            if let Some(key) = key {
                // SAFETY: a key this value registered.
                let _ = unsafe { UnregisterEventHotKey(key) };
            }
        }
    }

    /// One UI runtime's hot keys: the table and the handler, installed with
    /// the first binding.
    #[derive(Default)]
    pub(crate) struct HotKeys {
        table: Rc<HotKeyTable>,
        system: OnceCell<Result<CarbonHotKeys, OsStatus>>,
    }

    impl HotKeys {
        fn system(&self) -> Result<&CarbonHotKeys, OsStatus> {
            self.system
                .get_or_init(|| CarbonHotKeys::install(&self.table))
                .as_ref()
                .map_err(|status| *status)
        }
    }

    /// Unregisters a binding's hot keys when dropped.
    struct Registered {
        hot_keys: Rc<HotKeys>,
        ids: Vec<u32>,
    }

    impl Drop for Registered {
        fn drop(&mut self) {
            if let Ok(system) = self.hot_keys.system() {
                for id in self.ids.drain(..) {
                    unbind(&self.hot_keys.table, system, id);
                }
            }
        }
    }

    /// [`ShortcutBackend`] over Carbon's hot keys.
    pub(crate) struct MacShortcuts {
        hot_keys: Rc<HotKeys>,
    }

    impl MacShortcuts {
        /// Made on the main thread, where Carbon dispatches the hot keys.
        pub(crate) fn new(_mtm: MainThreadMarker, hot_keys: Rc<HotKeys>) -> Self {
            Self { hot_keys }
        }
    }

    impl ShortcutBackend for MacShortcuts {
        fn bind(&self, shortcuts: Vec<Shortcut>, feed: ShortcutFeed) -> ShortcutHold {
            let bound = self
                .hot_keys
                .system()
                .map_err(|status| ShortcutError::Failed(SystemFailure::Framework(status.into())))
                .and_then(|system| bind(&self.hot_keys.table, system, shortcuts, &feed));
            match bound {
                Ok((ids, bound)) => {
                    feed.send(ShortcutEvent::Bound(bound));
                    ShortcutHold::new(Registered {
                        hot_keys: Rc::clone(&self.hot_keys),
                        ids,
                    })
                }
                Err(error) => {
                    feed.send(ShortcutEvent::Ended(error));
                    ShortcutHold::new(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use flui_platform::keyboard::{Key, Modifiers, NamedKey};

    use super::{HotKeySystem, HotKeyTable, bind, hot_key, unbind};
    use crate::shortcuts::{Shortcut, ShortcutFeed, ShortcutId, Trigger};

    #[test]
    fn a_trigger_is_its_key_code_and_flags_and_is_shown_as_macos_writes_it() {
        /// A key code, its flags and how macOS shows the chord, or none.
        type Expected = Option<(u32, u32, &'static str)>;
        let cases: [(Modifiers, Key, Expected); 6] = [
            (
                Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::META,
                Key::character("a"),
                Some((0x00, (1 << 12) | (1 << 11) | (1 << 9) | (1 << 8), "⌃⌥⇧⌘A")),
            ),
            (
                Modifiers::META,
                Key::character("9"),
                Some((0x19, 1 << 8, "⌘9")),
            ),
            (
                Modifiers::NONE,
                Key::Named(NamedKey::F1),
                Some((0x7A, 0, "F1")),
            ),
            (
                Modifiers::SHIFT,
                Key::Named(NamedKey::F20),
                Some((0x5A, 1 << 9, "⇧F20")),
            ),
            (
                Modifiers::NONE,
                Key::Named(NamedKey::ArrowUp),
                Some((0x7E, 0, "↑")),
            ),
            // F21 to F24 have no key code on a Mac keyboard.
            (Modifiers::NONE, Key::Named(NamedKey::F21), None),
        ];
        for (modifiers, key, expected) in cases {
            let trigger = Trigger::try_new(modifiers, &key).expect("a trigger");
            assert_eq!(
                hot_key(&trigger),
                expected.map(|(code, flags, shown)| (code, flags, shown.to_owned())),
                "{modifiers:?} {key:?}"
            );
        }
    }

    /// Hot keys whose chords another application may hold.
    #[derive(Default)]
    struct Keys {
        held_elsewhere: Vec<u32>,
        registered: RefCell<Vec<u32>>,
    }

    impl HotKeySystem for Keys {
        fn register(&self, id: u32, code: u32, _flags: u32) -> Result<(), i32> {
            if self.held_elsewhere.contains(&code) {
                // eventHotKeyExistsErr.
                return Err(-9878);
            }
            self.registered.borrow_mut().push(id);
            Ok(())
        }

        fn unregister(&self, id: u32) {
            self.registered
                .borrow_mut()
                .retain(|registered| *registered != id);
        }
    }

    fn shortcut(id: &str, key: Option<&str>) -> Shortcut {
        let shortcut = Shortcut::new(ShortcutId::try_new(id).expect("an id"), "does a thing");
        match key {
            Some(key) => shortcut.preferred(
                Trigger::try_new(Modifiers::META, &Key::character(key)).expect("a trigger"),
            ),
            None => shortcut,
        }
    }

    #[test]
    fn a_shortcut_is_bound_to_its_preferred_chord_or_left_out_and_numbers_are_never_reused() {
        let table = HotKeyTable::default();
        // `s`'s key code: another application holds ⌘S.
        let keys = Keys {
            held_elsewhere: vec![0x01],
            ..Keys::default()
        };
        let feed = ShortcutFeed::new();
        let (ids, bound) = bind(
            &table,
            &keys,
            vec![
                shortcut("record", Some("r")),
                shortcut("save", Some("s")),
                shortcut("unbound", None),
            ],
            &feed,
        )
        .expect("numbers are left");
        assert_eq!(ids, [1]);
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].trigger.as_deref(), Some("⌘R"));
        assert!(table.heard(1, true), "the bound key's press reaches it");
        assert!(
            !table.heard(2, true),
            "the left-out shortcut has no hot key"
        );

        unbind(&table, &keys, 1);
        assert!(keys.registered.borrow().is_empty());
        assert!(!table.heard(1, false), "a dropped binding hears nothing");
        let (again, _) = bind(&table, &keys, vec![shortcut("record", Some("r"))], &feed)
            .expect("numbers are left");
        assert_eq!(
            again,
            [4],
            "a number is handed out once in a runtime's life"
        );
    }
}
