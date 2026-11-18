//! Global shortcuts over Carbon's hot keys (`RegisterEventHotKey`), which
//! reach the application while another one has the focus, need neither the
//! Accessibility permission nor an exemption from the sandbox, and report
//! the release as well as the press.
//!
//! Each UI runtime installs a handler on the application event target with
//! its first binding, and hears its hot keys on the main run loop. macOS has
//! no shortcut settings an application binds through: a shortcut is bound
//! to the trigger it prefers, or not at all, and the
//! [`ShortcutEvent::Bound`] list names the ones bound. One whose chord
//! another application holds (each is registered exclusive), that prefers
//! none, or whose key has no key code (F21 to F24) is left out of it. A
//! letter or digit is the key that types it on the keyboard layout current
//! when the binding is made, or its ANSI position where that layout types
//! it on no key (a non-Latin layout); the modifiers map Control, Option
//! (`ALT`), Shift and Command (`META`).
//!
//! A hot key's number is unique in the process: Carbon delivers every hot
//! key to every handler on the application target, so two runtimes with
//! numbers of their own would each take the other's events. The number is
//! never handed out again, so an event still queued for a dropped binding
//! finds no shortcut rather than a later one, and a handler passes on an
//! event whose number it does not hold, for the next handler to hear.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use flui_platform::keyboard::{Modifiers, NamedKey};
use rustc_hash::FxHashMap;

use crate::shortcuts::{
    BoundShortcut, Shortcut, ShortcutEvent, ShortcutFeed, ShortcutId, Trigger, TriggerKey,
    function_key,
};

/// Carbon's modifier flags (`Events.h`).
mod flag {
    pub(super) const COMMAND: u32 = 1 << 8;
    pub(super) const SHIFT: u32 = 1 << 9;
    pub(super) const OPTION: u32 = 1 << 11;
    pub(super) const CONTROL: u32 = 1 << 12;
}

/// The key code (`kVK_ANSI_*`) of an ASCII letter or digit's position on an
/// ANSI keyboard.
fn ansi_key(character: char) -> Option<u32> {
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

/// The highest key code a keyboard layout maps.
const LAST_KEY_CODE: u16 = 0x7F;

/// The key code that types `character` (ASCII, lower case) with no
/// modifier on the layout whose keys `typed` reads, or its ANSI position
/// when no key of that layout types it.
pub(super) fn key_code_for(character: char, typed: impl Fn(u16) -> Option<char>) -> Option<u32> {
    (0..=LAST_KEY_CODE)
        .find(|code| typed(*code).is_some_and(|typed| typed.to_ascii_lowercase() == character))
        .map(u32::from)
        .or_else(|| ansi_key(character))
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
            const CODES: [u32; 20] = [
                0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B,
                0x71, 0x6A, 0x40, 0x4F, 0x50, 0x5A,
            ];
            let number = function_key(key)?;
            let code = *CODES.get(usize::from(number).checked_sub(1)?)?;
            return Some((code, format!("F{number}")));
        }
    };
    Some((code, name.to_owned()))
}

/// A trigger as `RegisterEventHotKey` takes it: its key code, its modifier
/// flags, and the chord as macOS shows it (`⌃⌥⇧⌘A`); `None` for a key with
/// no key code. `key_code` names the key of a letter or digit.
pub(super) fn hot_key(
    trigger: &Trigger,
    key_code: impl Fn(char) -> Option<u32>,
) -> Option<(u32, u32, String)> {
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
            key_code(character)?
        }
        TriggerKey::Named(named) => {
            let (code, name) = named_key(named)?;
            shown.push_str(&name);
            code
        }
    };
    Some((code, flags, shown))
}

/// The next hot key number of the process: one domain, since every handler
/// on the application target hears every hot key. Only `fetch_add` reads it,
/// a monotonic counter (ADR-0097 §2) that 64 bits keep from wrapping.
static NEXT_HOT_KEY: AtomicU64 = AtomicU64::new(1);

/// A hot key's number in the process, never handed out twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct HotKeyNumber(u64);

/// `'flui'`, which a number's high half is folded into as Carbon's
/// signature.
const SIGNATURE: u32 = u32::from_be_bytes(*b"flui");

impl HotKeyNumber {
    fn mint() -> Self {
        Self(NEXT_HOT_KEY.fetch_add(1, Ordering::Relaxed))
    }

    /// The number as Carbon's `EventHotKeyID`: the signature, then the id.
    pub(super) fn to_carbon(self) -> (u32, u32) {
        let high = u32::try_from(self.0 >> 32).expect("BUG: the high half of a u64 fits a u32");
        let low = u32::try_from(self.0 & u64::from(u32::MAX)).expect("BUG: masked to 32 bits");
        (SIGNATURE ^ high, low)
    }

    /// The number an `EventHotKeyID` names, whoever registered it.
    pub(super) fn from_carbon(signature: u32, id: u32) -> Self {
        Self((u64::from(signature ^ SIGNATURE) << 32) | u64::from(id))
    }
}

/// Carbon's hot keys as the provider reaches them, on the main thread.
pub(super) trait HotKeySystem {
    /// Register hot key `number`, exclusive; the failure is the `OSStatus`.
    fn register(&self, number: HotKeyNumber, code: u32, flags: u32) -> Result<(), i32>;

    /// Unregister hot key `number`.
    fn unregister(&self, number: HotKeyNumber);

    /// The key code that types an ASCII letter or digit now.
    fn key_code(&self, character: char) -> Option<u32>;
}

/// Where each of a runtime's hot keys' events go, by number.
#[derive(Default)]
pub(crate) struct HotKeyTable {
    entries: RefCell<FxHashMap<HotKeyNumber, (ShortcutFeed, ShortcutId)>>,
}

impl HotKeyTable {
    /// Hot key `number` was pressed or released: its binding hears it.
    /// Returns whether the runtime holds that hot key; the table's borrow
    /// ends before the event is sent.
    pub(super) fn heard(&self, number: HotKeyNumber, pressed: bool) -> bool {
        let entry = self.entries.borrow().get(&number).cloned();
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

    fn insert(&self, number: HotKeyNumber, feed: ShortcutFeed, shortcut: ShortcutId) {
        let replaced = self.entries.borrow_mut().insert(number, (feed, shortcut));
        drop(replaced);
    }

    fn remove(&self, number: HotKeyNumber) {
        let removed = self.entries.borrow_mut().remove(&number);
        drop(removed);
    }
}

/// Register each of `shortcuts` that prefers a trigger with a key code, its
/// events sent to `feed`: the numbers registered and the shortcuts bound.
pub(super) fn bind(
    table: &HotKeyTable,
    system: &dyn HotKeySystem,
    shortcuts: Vec<Shortcut>,
    feed: &ShortcutFeed,
) -> (Vec<HotKeyNumber>, Vec<BoundShortcut>) {
    let mut registered = Vec::new();
    let mut bound = Vec::new();
    for shortcut in shortcuts {
        let Some((code, flags, shown)) = shortcut
            .preferred
            .as_ref()
            .and_then(|trigger| hot_key(trigger, |character| system.key_code(character)))
        else {
            continue;
        };
        let number = HotKeyNumber::mint();
        // In the table first: an event the system reports at once finds it.
        table.insert(number, feed.clone(), shortcut.id.clone());
        match system.register(number, code, flags) {
            Ok(()) => {
                registered.push(number);
                bound.push(BoundShortcut::new(shortcut.id, Some(shown)));
            }
            // Another application, or this binding, holds the chord: this
            // shortcut is not bound.
            Err(_) => table.remove(number),
        }
    }
    (registered, bound)
}

/// Unregister hot key `number` and forget its binding.
pub(super) fn unbind(table: &HotKeyTable, system: &dyn HotKeySystem, number: HotKeyNumber) {
    system.unregister(number);
    table.remove(number);
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::{HotKeys, MacShortcuts};

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "Carbon's hot keys, their event handler and the keyboard layout"
)]
mod appkit {
    use std::cell::{OnceCell, RefCell};
    use std::ffi::c_void;
    use std::rc::Rc;

    use objc2::MainThreadMarker;
    use rustc_hash::FxHashMap;

    use super::{
        HotKeyNumber, HotKeySystem, HotKeyTable, LAST_KEY_CODE, ansi_key, bind, key_code_for,
        unbind,
    };
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

    /// `EventHotKeyID`: a signature and an id.
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
    /// `kEventHotKeyExclusive`: the registration fails when another
    /// application holds the chord.
    const EXCLUSIVE: u32 = 1 << 0;
    /// `kUCKeyActionDown`, `kUCKeyTranslateNoDeadKeysMask`.
    const KEY_DOWN: u16 = 0;
    const NO_DEAD_KEYS: u32 = 1 << 0;

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
        fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
        fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *mut c_void;
        static kTISPropertyUnicodeKeyLayoutData: *const c_void;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            layout: *const c_void,
            code: u16,
            action: u16,
            modifiers: u32,
            keyboard: u32,
            options: u32,
            dead_keys: *mut u32,
            capacity: usize,
            length: *mut usize,
            text: *mut u16,
        ) -> OsStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFRelease(object: *const c_void);
    }

    /// The keyboard layout current now, while held.
    struct CurrentLayout {
        source: *mut c_void,
        layout: *const c_void,
    }

    impl CurrentLayout {
        /// The current layout, when it has Unicode key data (an input
        /// method may have none).
        fn read() -> Option<Self> {
            // SAFETY: on the main thread; the copy is released by `Drop`.
            let source = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
            if source.is_null() {
                return None;
            }
            // SAFETY: the property key is HIToolbox's constant; the data it
            // answers is owned by `source`, which this value holds.
            let data =
                unsafe { TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) };
            let layout = if data.is_null() {
                std::ptr::null()
            } else {
                // SAFETY: `data` is a live `CFData` owned by `source`.
                unsafe { CFDataGetBytePtr(data) }.cast::<c_void>()
            };
            let current = Self { source, layout };
            (!current.layout.is_null()).then_some(current)
        }

        /// The character key `code` types with no modifier, when it types one.
        fn typed(&self, code: u16) -> Option<char> {
            let mut dead_keys = 0_u32;
            let mut text = [0_u16; 4];
            let mut length = 0_usize;
            // SAFETY: `layout` is the held source's `UCKeyboardLayout`; the
            // buffers outlive the call, which writes at most `text.len()`
            // units and their count.
            let status = unsafe {
                UCKeyTranslate(
                    self.layout,
                    code,
                    KEY_DOWN,
                    0,
                    u32::from(LMGetKbdType()),
                    NO_DEAD_KEYS,
                    &raw mut dead_keys,
                    text.len(),
                    &raw mut length,
                    text.as_mut_ptr(),
                )
            };
            if status != 0 || length != 1 {
                return None;
            }
            char::from_u32(u32::from(text[0]))
        }
    }

    impl Drop for CurrentLayout {
        fn drop(&mut self) {
            // SAFETY: the copy `read` made, released once.
            unsafe { CFRelease(self.source) };
        }
    }

    /// The application event target's handler of a runtime's hot keys,
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
                if status != 0 || !matches!(kind, PRESSED | RELEASED) {
                    return NOT_HANDLED;
                }
                // SAFETY: the user data is the table the installed handler's
                // owner keeps alive until it removes the handler, and this
                // runs on the main thread, where the table is used.
                let table = unsafe { &*table.cast::<HotKeyTable>() };
                let number = HotKeyNumber::from_carbon(id.signature, id.id);
                if table.heard(number, kind == PRESSED) {
                    0
                } else {
                    NOT_HANDLED
                }
            },
        )
    }

    /// The real hot key system: the runtime's handler on the application
    /// event target, and each registered key's reference.
    struct CarbonHotKeys {
        handler: EventHandlerRef,
        keys: RefCell<FxHashMap<HotKeyNumber, EventHotKeyRef>>,
        /// Read when a binding is made, so a letter is the key that types
        /// it on the layout of that moment.
        layout: RefCell<Option<CurrentLayout>>,
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
                layout: RefCell::new(None),
                _table: Rc::clone(table),
            })
        }
    }

    impl Drop for CarbonHotKeys {
        fn drop(&mut self) {
            let keys = std::mem::take(&mut *self.keys.borrow_mut());
            for key in keys.into_values() {
                // SAFETY: a key this value registered and did not unregister.
                let _ = unsafe { UnregisterEventHotKey(key) };
            }
            // SAFETY: the handler this value installed.
            let _ = unsafe { RemoveEventHandler(self.handler) };
        }
    }

    impl HotKeySystem for CarbonHotKeys {
        fn register(&self, number: HotKeyNumber, code: u32, flags: u32) -> Result<(), i32> {
            let (signature, id) = number.to_carbon();
            let mut key: EventHotKeyRef = std::ptr::null_mut();
            // SAFETY: on the main thread; the reference is written into
            // `key`, which outlives the call.
            let status = unsafe {
                RegisterEventHotKey(
                    code,
                    flags,
                    EventHotKeyId { signature, id },
                    GetApplicationEventTarget(),
                    EXCLUSIVE,
                    &raw mut key,
                )
            };
            if status != 0 {
                return Err(status);
            }
            let replaced = self.keys.borrow_mut().insert(number, key);
            debug_assert!(replaced.is_none(), "a hot key number is handed out once");
            Ok(())
        }

        fn unregister(&self, number: HotKeyNumber) {
            let key = self.keys.borrow_mut().remove(&number);
            if let Some(key) = key {
                // SAFETY: a key this value registered.
                let _ = unsafe { UnregisterEventHotKey(key) };
            }
        }

        fn key_code(&self, character: char) -> Option<u32> {
            let layout = self.layout.borrow();
            match layout.as_ref() {
                Some(layout) => key_code_for(character, |code| {
                    (code <= LAST_KEY_CODE)
                        .then(|| layout.typed(code))
                        .flatten()
                }),
                None => ansi_key(character),
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
        numbers: Vec<HotKeyNumber>,
    }

    impl Drop for Registered {
        fn drop(&mut self) {
            if let Ok(system) = self.hot_keys.system() {
                for number in self.numbers.drain(..) {
                    unbind(&self.hot_keys.table, system, number);
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
            let system = match self.hot_keys.system() {
                Ok(system) => system,
                Err(status) => {
                    feed.send(ShortcutEvent::Ended(ShortcutError::Failed(
                        flui_platform::SystemFailure::Framework(status.into()),
                    )));
                    return ShortcutHold::new(());
                }
            };
            // The layout of this moment: a letter is the key that types it.
            let replaced = system.layout.replace(CurrentLayout::read());
            drop(replaced);
            let (numbers, bound) = bind(&self.hot_keys.table, system, shortcuts, &feed);
            feed.send(ShortcutEvent::Bound(bound));
            ShortcutHold::new(Registered {
                hot_keys: Rc::clone(&self.hot_keys),
                numbers,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use flui_platform::keyboard::{Key, Modifiers, NamedKey};

    use super::{
        HotKeyNumber, HotKeySystem, HotKeyTable, ansi_key, bind, hot_key, key_code_for, unbind,
    };
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
                hot_key(&trigger, ansi_key),
                expected.map(|(code, flags, shown)| (code, flags, shown.to_owned())),
                "{modifiers:?} {key:?}"
            );
        }
    }

    /// French AZERTY's unshifted keys at the ANSI positions of Q, A, W, Z
    /// and 1: `a`, `q`, `z`, `w` and `&`.
    fn azerty(code: u16) -> Option<char> {
        match code {
            0x0C => Some('a'),
            0x00 => Some('q'),
            0x0D => Some('z'),
            0x06 => Some('w'),
            0x12 => Some('&'),
            _ => None,
        }
    }

    #[test]
    fn a_letter_is_the_key_that_types_it_on_the_current_layout() {
        assert_eq!(key_code_for('a', azerty), Some(0x0C), "AZERTY types a at Q");
        assert_eq!(key_code_for('q', azerty), Some(0x00));
        assert_eq!(key_code_for('w', azerty), Some(0x06));
        assert_eq!(
            key_code_for('1', azerty),
            ansi_key('1'),
            "a digit the layout types only shifted keeps its ANSI position"
        );
        let cyrillic = |_: u16| Some('ф');
        assert_eq!(
            key_code_for('a', cyrillic),
            ansi_key('a'),
            "a layout that types no Latin letter binds the ANSI position"
        );
    }

    #[test]
    fn a_number_names_one_hot_key_in_the_process_and_survives_carbon_unchanged() {
        let first = HotKeyNumber::mint();
        let second = HotKeyNumber::mint();
        assert_ne!(first, second);
        for number in [first, HotKeyNumber(u64::MAX), HotKeyNumber(1 << 32)] {
            let (signature, id) = number.to_carbon();
            assert_eq!(HotKeyNumber::from_carbon(signature, id), number);
        }
        assert_eq!(
            HotKeyNumber(7).to_carbon(),
            (u32::from_be_bytes(*b"flui"), 7)
        );
    }

    /// Hot keys whose chords another application may hold, on a layout
    /// that types letters at their ANSI positions.
    #[derive(Default)]
    struct Keys {
        held_elsewhere: Vec<u32>,
        registered: RefCell<Vec<HotKeyNumber>>,
    }

    impl HotKeySystem for Keys {
        fn register(&self, number: HotKeyNumber, code: u32, _flags: u32) -> Result<(), i32> {
            if self.held_elsewhere.contains(&code) {
                // eventHotKeyExistsErr, for an exclusive registration.
                return Err(-9878);
            }
            self.registered.borrow_mut().push(number);
            Ok(())
        }

        fn unregister(&self, number: HotKeyNumber) {
            self.registered
                .borrow_mut()
                .retain(|registered| *registered != number);
        }

        fn key_code(&self, character: char) -> Option<u32> {
            ansi_key(character)
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
    fn a_shortcut_is_bound_to_its_preferred_chord_or_left_out_and_a_number_is_never_reused() {
        let table = HotKeyTable::default();
        // `s`'s key code: another application holds ⌘S.
        let keys = Keys {
            held_elsewhere: vec![0x01],
            ..Keys::default()
        };
        let feed = ShortcutFeed::new();
        let (numbers, bound) = bind(
            &table,
            &keys,
            vec![
                shortcut("record", Some("r")),
                shortcut("save", Some("s")),
                shortcut("unbound", None),
            ],
            &feed,
        );
        assert_eq!(numbers.len(), 1);
        assert_eq!(bound.len(), 1);
        assert_eq!(bound[0].trigger.as_deref(), Some("⌘R"));
        let recorded = numbers[0];
        assert!(
            table.heard(recorded, true),
            "the bound key's press reaches it"
        );

        unbind(&table, &keys, recorded);
        assert!(keys.registered.borrow().is_empty());
        assert!(
            !table.heard(recorded, false),
            "a dropped binding hears nothing"
        );
        let (again, _) = bind(&table, &keys, vec![shortcut("record", Some("r"))], &feed);
        assert_ne!(again, [recorded], "a number is handed out once");
    }

    #[test]
    fn two_runtimes_never_take_each_others_hot_keys() {
        let (first, second) = (HotKeyTable::default(), HotKeyTable::default());
        let keys = Keys::default();
        let feed = ShortcutFeed::new();
        let (in_first, _) = bind(&first, &keys, vec![shortcut("one", Some("1"))], &feed);
        let (in_second, _) = bind(&second, &keys, vec![shortcut("two", Some("2"))], &feed);
        assert!(
            !second.heard(in_first[0], true),
            "the second runtime passes it on"
        );
        assert!(first.heard(in_first[0], true));
        assert!(
            !first.heard(in_second[0], true),
            "the first runtime passes it on"
        );
        assert!(second.heard(in_second[0], true));
    }
}
