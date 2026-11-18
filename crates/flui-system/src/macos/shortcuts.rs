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
//! none, or whose key has no key code (F21 to F24) is left out of it. The
//! modifiers map Control, Option (`ALT`), Shift and Command (`META`).
//!
//! A letter or digit is the key of the keyboard's main block that types it
//! on the current layout while the chord's Shift and Command are held
//! ([`key_code_for`]), so a chord is pressed where its user reads it: on
//! AZERTY `⌘A` is the key beside Tab, and on a layout that changes under
//! Command (Dvorak – QWERTY ⌘) `⌘S` is the key QWERTY has it on. The
//! runtime hears the layout change and moves every shortcut whose key is
//! another one now ([`layout_changed`]); one whose new chord another
//! application holds leaves the binding, which hears
//! [`ShortcutEvent::Changed`] with the shortcuts still bound.
//!
//! A hot key's number is unique in the process: Carbon delivers every hot
//! key to every handler on the application target, so two runtimes with
//! numbers of their own would each take the other's events. The number is
//! never handed out again, so an event still queued for a dropped binding
//! finds no shortcut rather than a later one, and a handler passes on an
//! event whose number it does not hold, for the next handler to hear. A
//! process that handed out every number binds no more shortcuts
//! ([`ShortcutError::Exhausted`]).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A keyboard layout, as the provider asks it what a key types.
pub(super) trait KeyLayout {
    /// The character the key `code` types while the modifiers `flags`
    /// (Carbon's) are held, when it types exactly one.
    fn typed(&self, code: u16, flags: u32) -> Option<char>;
}

/// The last key code of a keyboard's main block, which starts at 0: the
/// letters, the digit row and the punctuation around them, `kVK_ANSI_A` to
/// `kVK_ANSI_Grave`. The keypad, whose keys type the digits on every layout
/// and which a laptop does not have, and the function and navigation keys
/// lie above it.
const LAST_MAIN_BLOCK_KEY: u16 = 0x32;

/// The modifiers of a chord a layout is asked under: Shift, and Command,
/// under which some layouts type other letters (Dvorak – QWERTY ⌘). Under
/// Control or Option a key types a control or an alternate character, not
/// the letter its chord is read by, so they are left out.
const LAYOUT_FLAGS: u32 = flag::COMMAND | flag::SHIFT;

/// The key code of the main-block key that types `character` (an ASCII
/// letter or digit, lower case) on `layout` while the chord's `flags` are
/// held, or its ANSI position when no such key types it: a non-Latin
/// layout, a digit the layout types only shifted (AZERTY, Czech) asked for
/// without Shift, a layout with no key data (`None`).
pub(super) fn key_code_for(
    character: char,
    flags: u32,
    layout: Option<&impl KeyLayout>,
) -> Option<u32> {
    let held = flags & LAYOUT_FLAGS;
    layout
        .and_then(|layout| {
            (0..=LAST_MAIN_BLOCK_KEY).find(|code| {
                layout
                    .typed(*code, held)
                    .is_some_and(|typed| typed.to_ascii_lowercase() == character)
            })
        })
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

/// A trigger as `RegisterEventHotKey` takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HotKey {
    /// The key code (`kVK_*`).
    pub(super) code: u32,
    /// Carbon's modifier flags.
    pub(super) flags: u32,
    /// The chord as macOS shows it (`⌃⌥⇧⌘A`).
    pub(super) shown: String,
}

/// `trigger` as a hot key on `layout`; `None` for a key with no key code.
pub(super) fn hot_key(trigger: &Trigger, layout: Option<&impl KeyLayout>) -> Option<HotKey> {
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
            key_code_for(character, flags, layout)?
        }
        TriggerKey::Named(named) => {
            let (code, name) = named_key(named)?;
            shown.push_str(&name);
            code
        }
    };
    Some(HotKey { code, flags, shown })
}

/// A hot key's number in the process, never handed out twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct HotKeyNumber(u64);

/// `'flui'`, which a number's high half is folded into as Carbon's
/// signature.
const SIGNATURE: u32 = u32::from_be_bytes(*b"flui");

impl HotKeyNumber {
    /// The number after `last`, the last one handed out, or `None` once
    /// every number was: `last` then stays where it is, so no number is
    /// ever handed out again.
    fn mint(last: &AtomicU64) -> Option<Self> {
        last.try_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            last.checked_add(1)
        })
        .ok()
        .map(|previous| Self(previous + 1))
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

/// Carbon's hot keys and the keyboard's layout as the provider reaches
/// them, on the main thread.
pub(super) trait HotKeySystem {
    /// The layout this system reads.
    type Layout: KeyLayout;

    /// Register hot key `number`, exclusive; the failure is the `OSStatus`.
    fn register(&self, number: HotKeyNumber, code: u32, flags: u32) -> Result<(), i32>;

    /// Unregister hot key `number`.
    fn unregister(&self, number: HotKeyNumber);

    /// The keyboard layout current now, held for one pass over the
    /// shortcuts; `None` when it has no key data (an input method), and a
    /// letter is then its ANSI position.
    fn layout(&self) -> Option<Self::Layout>;
}

/// One shortcut of a binding that has a key code.
struct BoundKey {
    id: ShortcutId,
    key: TriggerKey,
    /// Its hot key on the layout it was last resolved on.
    hot_key: HotKey,
    /// The number it is registered under; `None` while another application,
    /// or another shortcut of this one, holds its chord.
    number: Option<HotKeyNumber>,
}

/// A binding's shortcuts while it is held: what the layout change resolves
/// again, and where it tells the binding what is left.
pub(super) struct Binding {
    feed: ShortcutFeed,
    keys: RefCell<Vec<BoundKey>>,
}

/// The shortcuts of `keys` that are registered, as the binding hears them.
fn bound_shortcuts(keys: &[BoundKey]) -> Vec<BoundShortcut> {
    keys.iter()
        .filter(|key| key.number.is_some())
        .map(|key| BoundShortcut::new(key.id.clone(), Some(key.hot_key.shown.clone())))
        .collect::<Vec<BoundShortcut>>()
}

/// One UI runtime's hot keys: where each one's events go, by number, and
/// the bindings held.
#[derive(Default)]
pub(crate) struct HotKeyTable {
    entries: RefCell<FxHashMap<HotKeyNumber, (ShortcutFeed, ShortcutId)>>,
    bindings: RefCell<Vec<Rc<Binding>>>,
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

    /// The bindings held now, apart from the table's borrow.
    fn held(&self) -> Vec<Rc<Binding>> {
        self.bindings.borrow().clone()
    }
}

/// Register `key` under a number of its own, its events sent to `feed`. A
/// chord another application, or another shortcut of this one, holds leaves
/// it unregistered.
///
/// # Errors
///
/// [`ShortcutError::Exhausted`] when the process has no number left;
/// nothing was registered.
fn register<S: HotKeySystem>(
    table: &HotKeyTable,
    system: &S,
    numbers: &AtomicU64,
    feed: &ShortcutFeed,
    key: &mut BoundKey,
) -> Result<(), ShortcutError> {
    let number = HotKeyNumber::mint(numbers).ok_or(ShortcutError::Exhausted)?;
    // In the table first: an event the system reports at once finds it.
    table.insert(number, feed.clone(), key.id.clone());
    match system.register(number, key.hot_key.code, key.hot_key.flags) {
        Ok(()) => key.number = Some(number),
        Err(_) => table.remove(number),
    }
    Ok(())
}

/// Unregister `key`, if it is registered, and forget its number.
fn release<S: HotKeySystem>(table: &HotKeyTable, system: &S, key: &mut BoundKey) {
    if let Some(number) = key.number.take() {
        system.unregister(number);
        table.remove(number);
    }
}

/// Register each of `shortcuts` that prefers a trigger with a key code, on
/// the layout current now, and tell `feed` which were bound. The binding is
/// held until [`unbind`], and resolved again by [`layout_changed`].
///
/// A process with no number left binds nothing: `feed` hears
/// [`ShortcutError::Exhausted`] as its end, and there is no binding to
/// hold.
pub(super) fn bind<S: HotKeySystem>(
    table: &HotKeyTable,
    system: &S,
    numbers: &AtomicU64,
    shortcuts: Vec<Shortcut>,
    feed: &ShortcutFeed,
) -> Option<Rc<Binding>> {
    let layout = system.layout();
    let mut keys = shortcuts
        .into_iter()
        .filter_map(|shortcut| {
            let trigger = shortcut.preferred?;
            Some(BoundKey {
                id: shortcut.id,
                key: trigger.key(),
                hot_key: hot_key(&trigger, layout.as_ref())?,
                number: None,
            })
        })
        .collect::<Vec<BoundKey>>();
    drop(layout);
    let registered = keys
        .iter_mut()
        .try_for_each(|key| register(table, system, numbers, feed, key));
    if let Err(exhausted) = registered {
        for key in &mut keys {
            release(table, system, key);
        }
        feed.send(ShortcutEvent::Ended(exhausted));
        return None;
    }
    let binding = Rc::new(Binding {
        feed: feed.clone(),
        keys: RefCell::new(keys),
    });
    table.bindings.borrow_mut().push(Rc::clone(&binding));
    let bound = bound_shortcuts(&binding.keys.borrow());
    feed.send(ShortcutEvent::Bound(bound));
    Some(binding)
}

/// Unregister `binding`'s hot keys and forget it.
pub(super) fn unbind<S: HotKeySystem>(table: &HotKeyTable, system: &S, binding: &Rc<Binding>) {
    table
        .bindings
        .borrow_mut()
        .retain(|held| !Rc::ptr_eq(held, binding));
    for key in binding.keys.borrow_mut().iter_mut() {
        release(table, system, key);
    }
}

/// The keyboard layout changed: every shortcut of a held binding whose
/// letter or digit is another key now moves to that key, so a chord goes on
/// firing where its user reads it and never on the key its old code is now.
///
/// Every key that moves is given up before any is taken: two shortcuts
/// whose keys trade places (`A` and `Q` between QWERTY and AZERTY) would
/// otherwise each find the other's chord still held. A shortcut whose new
/// chord is held leaves its binding, and one that was left out and whose
/// new chord is free joins it; a binding whose shortcuts changed that way
/// hears [`ShortcutEvent::Changed`] with the ones bound now. A binding that
/// needs a number the process no longer has ends with
/// [`ShortcutError::Exhausted`], none of its shortcuts registered.
pub(super) fn layout_changed<S: HotKeySystem>(
    table: &HotKeyTable,
    system: &S,
    numbers: &AtomicU64,
) {
    /// What a binding gave up: the places of the keys that moved, and
    /// which of its shortcuts were bound before.
    struct Moved {
        binding: Rc<Binding>,
        places: Vec<usize>,
        was_bound: Vec<bool>,
    }

    let layout = system.layout();
    let moved = table
        .held()
        .into_iter()
        .map(|binding| {
            let mut keys = binding.keys.borrow_mut();
            let was_bound = keys
                .iter()
                .map(|key| key.number.is_some())
                .collect::<Vec<bool>>();
            let mut places = Vec::new();
            for (place, key) in keys.iter_mut().enumerate() {
                let TriggerKey::Character(character) = key.key else {
                    continue;
                };
                let code = key_code_for(character, key.hot_key.flags, layout.as_ref());
                if let Some(code) = code.filter(|code| *code != key.hot_key.code) {
                    release(table, system, key);
                    key.hot_key.code = code;
                    places.push(place);
                }
            }
            drop(keys);
            Moved {
                binding,
                places,
                was_bound,
            }
        })
        .collect::<Vec<Moved>>();
    drop(layout);

    for Moved {
        binding,
        places,
        was_bound,
    } in moved
    {
        // What the binding hears, made under the borrow and sent after it.
        let event = {
            let mut keys = binding.keys.borrow_mut();
            let registered = places.into_iter().try_for_each(|place| {
                register(table, system, numbers, &binding.feed, &mut keys[place])
            });
            match registered {
                Ok(()) => keys
                    .iter()
                    .map(|key| key.number.is_some())
                    .ne(was_bound)
                    .then(|| ShortcutEvent::Changed(bound_shortcuts(&keys))),
                Err(exhausted) => Some(ShortcutEvent::Ended(exhausted)),
            }
        };
        if matches!(event, Some(ShortcutEvent::Ended(_))) {
            unbind(table, system, &binding);
        }
        if let Some(event) = event {
            binding.feed.send(event);
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use appkit::{HotKeys, MacShortcuts};

#[cfg(target_os = "macos")]
#[expect(
    unsafe_code,
    reason = "Carbon's hot keys, their event handler, the keyboard layout and its change"
)]
mod appkit {
    use std::cell::{OnceCell, RefCell};
    use std::ffi::c_void;
    use std::rc::Rc;
    use std::sync::atomic::AtomicU64;

    use objc2::MainThreadMarker;
    use rustc_hash::FxHashMap;

    use super::{
        Binding, HotKeyNumber, HotKeySystem, HotKeyTable, KeyLayout, bind, layout_changed, unbind,
    };
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::shortcuts::{
        Shortcut, ShortcutBackend, ShortcutError, ShortcutEvent, ShortcutFeed, ShortcutHold,
    };

    /// The last hot key number handed out in the process, one domain since
    /// every handler on the application target hears every hot key. Only
    /// `HotKeyNumber::mint` advances it, and it stops at the last number.
    static LAST_HOT_KEY: AtomicU64 = AtomicU64::new(0);

    type OsStatus = i32;
    type EventTargetRef = *mut c_void;
    type EventHandlerRef = *mut c_void;
    type EventHandlerCallRef = *mut c_void;
    type EventRef = *mut c_void;
    type EventHotKeyRef = *mut c_void;
    type EventHandlerUpp = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OsStatus;
    /// `CFNotificationCallback`: the center, the observer, the name, the
    /// object and the user info.
    type NotificationCallback =
        extern "C" fn(*mut c_void, *mut c_void, *const c_void, *const c_void, *const c_void);

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
    /// `CFNotificationSuspensionBehaviorDeliverImmediately`: a change is
    /// heard while the application is in the background, which is when a
    /// global shortcut matters.
    const DELIVER_IMMEDIATELY: isize = 4;

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
        /// The distributed notification's name, a `CFStringRef`.
        static kTISNotifySelectedKeyboardInputSourceChanged: *const c_void;
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
        fn CFNotificationCenterGetDistributedCenter() -> *mut c_void;
        fn CFNotificationCenterAddObserver(
            center: *mut c_void,
            observer: *const c_void,
            callback: NotificationCallback,
            name: *const c_void,
            object: *const c_void,
            suspension_behavior: isize,
        );
        fn CFNotificationCenterRemoveObserver(
            center: *mut c_void,
            observer: *const c_void,
            name: *const c_void,
            object: *const c_void,
        );
    }

    /// The keyboard layout current when it was read, while held.
    struct CurrentLayout {
        source: *mut c_void,
        layout: *const c_void,
        /// The keyboard's type (`LMGetKbdType`), which says how an ISO or a
        /// JIS keyboard's extra keys map.
        keyboard: u32,
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
            // SAFETY: takes nothing and answers the type of the keyboard
            // Carbon last heard from; on the main thread.
            let keyboard = u32::from(unsafe { LMGetKbdType() });
            let current = Self {
                source,
                layout,
                keyboard,
            };
            (!current.layout.is_null()).then_some(current)
        }
    }

    impl KeyLayout for CurrentLayout {
        fn typed(&self, code: u16, flags: u32) -> Option<char> {
            let mut dead_keys = 0_u32;
            let mut text = [0_u16; 4];
            let mut length = 0_usize;
            // SAFETY: `layout` is the held source's `UCKeyboardLayout`; the
            // buffers outlive the call, which writes at most `text.len()`
            // units and their count. The modifier state is Carbon's flags
            // shifted down to their byte, as `UCKeyTranslate` takes them.
            let status = unsafe {
                UCKeyTranslate(
                    self.layout,
                    code,
                    KEY_DOWN,
                    (flags >> 8) & 0xFF,
                    self.keyboard,
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

    /// The distributed notification center's callback for the selected
    /// keyboard input source, whose observer is the runtime's [`HotKeys`].
    extern "C" fn input_source_changed(
        center: *mut c_void,
        observer: *mut c_void,
        _name: *const c_void,
        _object: *const c_void,
        _user_info: *const c_void,
    ) {
        os_entry(
            "keyboard input source change",
            (center, observer),
            AfterPanic::Answer(|_| ()),
            || {
                // The center delivers on the main run loop; anywhere else
                // the runtime's hot keys are not this thread's to touch.
                if MainThreadMarker::new().is_none() {
                    return;
                }
                // SAFETY: the observer is the `HotKeys` whose
                // `LayoutObserver` added it and removes it before the
                // `HotKeys` is freed, and this is the main thread, where it
                // is used.
                let hot_keys = unsafe { &*observer.cast::<HotKeys>() };
                hot_keys.layout_changed();
            },
        );
    }

    /// The real hot key system: the runtime's handler on the application
    /// event target, and each registered key's reference.
    struct CarbonHotKeys {
        handler: EventHandlerRef,
        keys: RefCell<FxHashMap<HotKeyNumber, EventHotKeyRef>>,
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
        type Layout = CurrentLayout;

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

        fn layout(&self) -> Option<CurrentLayout> {
            CurrentLayout::read()
        }
    }

    /// A runtime's observer of the selected keyboard input source, removed
    /// when dropped.
    struct LayoutObserver {
        /// The observer the center was given: the `HotKeys` that keeps this
        /// value.
        hot_keys: *const HotKeys,
    }

    impl LayoutObserver {
        /// Observe for `hot_keys`, which keeps the observer it is given.
        fn add(hot_keys: &Rc<HotKeys>) -> Self {
            let observer = Rc::as_ptr(hot_keys);
            // SAFETY: on the main thread. The observer is a `HotKeys` in an
            // `Rc`, so it does not move, and it keeps this value, whose
            // `Drop` removes the observer before the `HotKeys` is freed. The
            // name is HIToolbox's constant string.
            unsafe {
                CFNotificationCenterAddObserver(
                    CFNotificationCenterGetDistributedCenter(),
                    observer.cast::<c_void>(),
                    input_source_changed,
                    kTISNotifySelectedKeyboardInputSourceChanged,
                    std::ptr::null(),
                    DELIVER_IMMEDIATELY,
                );
            }
            Self { hot_keys: observer }
        }
    }

    impl Drop for LayoutObserver {
        fn drop(&mut self) {
            // SAFETY: removes the observer `add` registered, under the name
            // it registered it for.
            unsafe {
                CFNotificationCenterRemoveObserver(
                    CFNotificationCenterGetDistributedCenter(),
                    self.hot_keys.cast::<c_void>(),
                    kTISNotifySelectedKeyboardInputSourceChanged,
                    std::ptr::null(),
                );
            }
        }
    }

    /// One UI runtime's hot keys: the table, and the handler and the layout
    /// observer, installed with the first binding.
    #[derive(Default)]
    pub(crate) struct HotKeys {
        table: Rc<HotKeyTable>,
        /// Before the system it registers through, so it is removed first.
        observer: OnceCell<LayoutObserver>,
        system: OnceCell<Result<CarbonHotKeys, OsStatus>>,
    }

    impl HotKeys {
        /// The hot key system, installed on first use; with it the runtime
        /// starts hearing the layout change.
        fn system(self: &Rc<Self>) -> Result<&CarbonHotKeys, OsStatus> {
            let system = self
                .system
                .get_or_init(|| CarbonHotKeys::install(&self.table))
                .as_ref()
                .map_err(|status| *status)?;
            self.observer.get_or_init(|| LayoutObserver::add(self));
            Ok(system)
        }

        /// The hot key system, when a binding installed it.
        fn installed(&self) -> Option<&CarbonHotKeys> {
            self.system.get().and_then(|system| system.as_ref().ok())
        }

        /// The selected keyboard input source changed: every held binding
        /// is resolved on the layout current now.
        fn layout_changed(&self) {
            if let Some(system) = self.installed() {
                layout_changed(&self.table, system, &LAST_HOT_KEY);
            }
        }
    }

    /// Unregisters a binding's hot keys when dropped.
    struct Registered {
        hot_keys: Rc<HotKeys>,
        binding: Rc<Binding>,
    }

    impl Drop for Registered {
        fn drop(&mut self) {
            if let Some(system) = self.hot_keys.installed() {
                unbind(&self.hot_keys.table, system, &self.binding);
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
            match bind(
                &self.hot_keys.table,
                system,
                &LAST_HOT_KEY,
                shortcuts,
                &feed,
            ) {
                Some(binding) => ShortcutHold::new(Registered {
                    hot_keys: Rc::clone(&self.hot_keys),
                    binding,
                }),
                None => ShortcutHold::new(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::atomic::AtomicU64;

    use flui_platform::capability::Abandoned;
    use flui_platform::keyboard::{Key, Modifiers, NamedKey};

    use super::{
        HotKey, HotKeyNumber, HotKeySystem, HotKeyTable, KeyLayout, ansi_key, bind, flag, hot_key,
        key_code_for, layout_changed, unbind,
    };
    use crate::shortcuts::{
        BoundShortcut, GlobalShortcuts, Shortcut, ShortcutBackend, ShortcutBinding, ShortcutError,
        ShortcutEvent, ShortcutFeed, ShortcutHold, ShortcutId, Trigger,
    };
    use crate::testing::owner::TestOwner;

    /// The keyboard layouts the model reads, each as its keys type.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Layout {
        /// U.S.: every letter and digit at its ANSI position.
        Qwerty,
        /// French: `a` and `q`, `z` and `w` trade places, `m` is beside
        /// `l`, and the digit row types its digits only with Shift.
        Azerty,
        /// Dvorak – QWERTY ⌘: Dvorak, and QWERTY while Command is held.
        DvorakQwertyCommand,
        /// A layout that types no Latin letter.
        Cyrillic,
    }

    /// The letter or digit at `code` on a U.S. keyboard.
    fn qwerty(code: u16) -> Option<char> {
        ('a'..='z')
            .chain('0'..='9')
            .find(|character| ansi_key(*character) == Some(u32::from(code)))
    }

    /// The digit a keypad key types, on every layout and under every
    /// modifier.
    fn keypad(code: u16) -> Option<char> {
        const DIGITS: [(u16, char); 10] = [
            (0x52, '0'),
            (0x53, '1'),
            (0x54, '2'),
            (0x55, '3'),
            (0x56, '4'),
            (0x57, '5'),
            (0x58, '6'),
            (0x59, '7'),
            (0x5B, '8'),
            (0x5C, '9'),
        ];
        DIGITS
            .iter()
            .find(|(key, _)| *key == code)
            .map(|(_, digit)| *digit)
    }

    impl KeyLayout for Layout {
        fn typed(&self, code: u16, flags: u32) -> Option<char> {
            /// `kVK_ANSI_Semicolon`.
            const SEMICOLON: u16 = 0x29;
            if let Some(digit) = keypad(code) {
                return Some(digit);
            }
            let shift = flags & flag::SHIFT != 0;
            let cased = |letter: char| {
                if shift {
                    letter.to_ascii_uppercase()
                } else {
                    letter
                }
            };
            // A digit key under Shift on a U.S. keyboard types punctuation.
            let us = |code: u16| match qwerty(code) {
                Some(digit) if digit.is_ascii_digit() && shift => Some('!'),
                typed => typed.map(cased),
            };
            match self {
                Self::Qwerty => us(code),
                Self::Azerty => match (code, qwerty(code)) {
                    (SEMICOLON, _) => Some(cased('m')),
                    (_, Some(digit)) if digit.is_ascii_digit() => {
                        Some(if shift { digit } else { '&' })
                    }
                    (_, Some('a')) => Some(cased('q')),
                    (_, Some('q')) => Some(cased('a')),
                    (_, Some('z')) => Some(cased('w')),
                    (_, Some('w')) => Some(cased('z')),
                    (_, Some('m')) => Some(','),
                    (_, typed) => typed.map(cased),
                },
                Self::DvorakQwertyCommand if flags & flag::COMMAND != 0 => us(code),
                Self::DvorakQwertyCommand => {
                    const QWERTY: &str = "qwertyuiopasdfghjklzxcvbnm";
                    const DVORAK: &str = "',.pyfgcrlaoeuidhtn;qjkxbm";
                    if code == SEMICOLON {
                        return Some(cased('s'));
                    }
                    match qwerty(code) {
                        Some(digit) if digit.is_ascii_digit() => us(code),
                        Some(letter) => QWERTY
                            .find(letter)
                            .and_then(|place| DVORAK.chars().nth(place))
                            .map(cased),
                        None => None,
                    }
                }
                Self::Cyrillic => qwerty(code).map(|_| 'ф'),
            }
        }
    }

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
                hot_key(&trigger, Some(&Layout::Qwerty)),
                expected.map(|(code, flags, shown)| HotKey {
                    code,
                    flags,
                    shown: shown.to_owned()
                }),
                "{modifiers:?} {key:?}"
            );
        }
    }

    #[test]
    fn a_letter_or_digit_is_the_main_block_key_that_types_it_under_the_chords_shift_and_command() {
        const COMMAND: u32 = flag::COMMAND;
        const SHIFT_COMMAND: u32 = flag::SHIFT | flag::COMMAND;
        let cases: [(Option<Layout>, char, u32, u32, &str); 15] = [
            (
                Some(Layout::Azerty),
                'a',
                COMMAND,
                0x0C,
                "AZERTY types a at Q",
            ),
            (Some(Layout::Azerty), 'q', COMMAND, 0x00, "and q at A"),
            (Some(Layout::Azerty), 'w', COMMAND, 0x06, "w at Z"),
            (Some(Layout::Azerty), 'm', COMMAND, 0x29, "m beside l"),
            (
                Some(Layout::Azerty),
                '1',
                COMMAND,
                0x12,
                "a digit typed only with Shift keeps its ANSI key, never the keypad's",
            ),
            (
                Some(Layout::Azerty),
                '1',
                SHIFT_COMMAND,
                0x12,
                "with Shift in the chord the digit row types it",
            ),
            (Some(Layout::Qwerty), '1', COMMAND, 0x12, "the digit row"),
            (
                Some(Layout::Qwerty),
                '1',
                SHIFT_COMMAND,
                0x12,
                "Shift and 1 type punctuation: the ANSI key",
            ),
            (
                Some(Layout::DvorakQwertyCommand),
                's',
                COMMAND,
                0x01,
                "under Command the layout is QWERTY",
            ),
            (
                Some(Layout::DvorakQwertyCommand),
                's',
                flag::CONTROL | flag::OPTION,
                0x29,
                "and without it Dvorak, whatever else is held",
            ),
            (
                Some(Layout::DvorakQwertyCommand),
                's',
                flag::SHIFT,
                0x29,
                "a capital is its letter's key",
            ),
            (
                Some(Layout::Azerty),
                'a',
                flag::CONTROL | flag::OPTION,
                0x0C,
                "Control and Option do not change which key is A",
            ),
            (
                Some(Layout::Cyrillic),
                'a',
                COMMAND,
                0x00,
                "a layout that types no Latin letter binds the ANSI position",
            ),
            (
                Some(Layout::Cyrillic),
                '7',
                COMMAND,
                0x1A,
                "and no keypad key for a digit",
            ),
            (None, 'a', COMMAND, 0x00, "a layout with no key data"),
        ];
        for (layout, character, flags, code, why) in cases {
            assert_eq!(
                key_code_for(character, flags, layout.as_ref()),
                Some(code),
                "{layout:?} {character} {flags:#x}: {why}"
            );
        }
    }

    #[test]
    fn a_number_names_one_hot_key_in_the_process_and_survives_carbon_unchanged() {
        let last = AtomicU64::new(0);
        let first = HotKeyNumber::mint(&last).expect("a number");
        let second = HotKeyNumber::mint(&last).expect("another");
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

    /// Carbon's hot keys as the model keeps them: a chord is registered
    /// once, here or by another application, on the layout of the moment.
    struct Keys {
        layout: Cell<Option<Layout>>,
        /// The chords another application holds: a key code and flags.
        held_elsewhere: RefCell<Vec<(u32, u32)>>,
        registered: RefCell<Vec<(HotKeyNumber, u32, u32)>>,
    }

    impl Keys {
        fn on(layout: Layout) -> Rc<Self> {
            Rc::new(Self {
                layout: Cell::new(Some(layout)),
                held_elsewhere: RefCell::default(),
                registered: RefCell::default(),
            })
        }

        /// The key code hot key `number` is registered at.
        fn code_of(&self, number: HotKeyNumber) -> Option<u32> {
            self.registered
                .borrow()
                .iter()
                .find(|(registered, _, _)| *registered == number)
                .map(|(_, code, _)| *code)
        }
    }

    impl HotKeySystem for Keys {
        type Layout = Layout;

        fn register(&self, number: HotKeyNumber, code: u32, flags: u32) -> Result<(), i32> {
            let chord = (code, flags);
            let held = self.held_elsewhere.borrow().contains(&chord)
                || self
                    .registered
                    .borrow()
                    .iter()
                    .any(|(_, code, flags)| (*code, *flags) == chord);
            if held {
                // eventHotKeyExistsErr.
                return Err(-9878);
            }
            self.registered.borrow_mut().push((number, code, flags));
            Ok(())
        }

        fn unregister(&self, number: HotKeyNumber) {
            self.registered
                .borrow_mut()
                .retain(|(registered, _, _)| *registered != number);
        }

        fn layout(&self) -> Option<Layout> {
            self.layout.get()
        }
    }

    /// One UI runtime's shortcuts over the model's keys: what
    /// `MacShortcuts` is over Carbon's.
    #[derive(Clone)]
    struct Runtime {
        table: Rc<HotKeyTable>,
        keys: Rc<Keys>,
        numbers: Rc<AtomicU64>,
    }

    impl Runtime {
        fn on(keys: &Rc<Keys>, numbers: &Rc<AtomicU64>) -> Self {
            Self {
                table: Rc::default(),
                keys: Rc::clone(keys),
                numbers: Rc::clone(numbers),
            }
        }

        /// The number and key code `shortcut` is registered under.
        fn registered(&self, shortcut: &str) -> Option<(HotKeyNumber, u32)> {
            let number = self
                .table
                .entries
                .borrow()
                .iter()
                .find(|(_, (_, id))| id.as_str() == shortcut)
                .map(|(number, _)| *number)?;
            Some((number, self.keys.code_of(number)?))
        }

        fn layout_changed(&self, layout: Layout) {
            self.keys.layout.set(Some(layout));
            layout_changed(&self.table, &*self.keys, &self.numbers);
        }
    }

    /// Unbinds its binding when dropped.
    struct Held {
        runtime: Runtime,
        binding: Rc<super::Binding>,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            unbind(&self.runtime.table, &*self.runtime.keys, &self.binding);
        }
    }

    impl ShortcutBackend for Runtime {
        fn bind(&self, shortcuts: Vec<Shortcut>, feed: ShortcutFeed) -> ShortcutHold {
            match bind(&self.table, &*self.keys, &self.numbers, shortcuts, &feed) {
                Some(binding) => ShortcutHold::new(Held {
                    runtime: self.clone(),
                    binding,
                }),
                None => ShortcutHold::new(()),
            }
        }
    }

    fn id(name: &str) -> ShortcutId {
        ShortcutId::try_new(name).expect("an id")
    }

    /// The shortcut `name`, preferring Command and the key `name` names.
    fn shortcut(name: &str) -> Shortcut {
        let key = match name {
            "f5" => Key::Named(NamedKey::F5),
            letter => Key::character(letter),
        };
        Shortcut::new(id(name), "does a thing")
            .preferred(Trigger::try_new(Modifiers::META, &key).expect("a trigger"))
    }

    /// `names` as the binding hears them bound: each with its chord.
    fn bound(names: &[&str]) -> Vec<BoundShortcut> {
        names
            .iter()
            .map(|name| {
                let chord = format!("⌘{}", name.to_ascii_uppercase());
                BoundShortcut::new(id(name), Some(chord))
            })
            .collect::<Vec<BoundShortcut>>()
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
    fn a_shortcut_is_bound_to_its_preferred_chord_or_left_out_and_a_number_is_never_reused() {
        let owner = TestOwner::new();
        let keys = Keys::on(Layout::Qwerty);
        // Another application holds ⌘S.
        keys.held_elsewhere.borrow_mut().push((0x01, flag::COMMAND));
        let runtime = Runtime::on(&keys, &Rc::default());
        let shortcuts = GlobalShortcuts::new(runtime.clone(), &owner.ctx(None));
        let unbound = Shortcut::new(id("unbound"), "prefers no trigger");
        let binding = shortcuts.bind(vec![shortcut("r"), shortcut("s"), unbound]);
        assert_eq!(next(&owner, &binding), ShortcutEvent::Bound(bound(&["r"])));
        assert_eq!(keys.registered.borrow().len(), 1);
        let (recorded, code) = runtime.registered("r").expect("⌘R is registered");
        assert_eq!(code, 0x0F);
        assert!(runtime.table.heard(recorded, true));
        assert_eq!(next(&owner, &binding), ShortcutEvent::Pressed(id("r")));

        drop(binding);
        assert!(keys.registered.borrow().is_empty());
        assert!(
            !runtime.table.heard(recorded, false),
            "a dropped binding hears nothing"
        );
        let again = shortcuts.bind(vec![shortcut("r")]);
        assert_eq!(next(&owner, &again), ShortcutEvent::Bound(bound(&["r"])));
        let (renumbered, _) = runtime.registered("r").expect("⌘R is registered again");
        assert_ne!(renumbered, recorded, "a number is handed out once");
    }

    #[test]
    fn two_runtimes_never_take_each_others_hot_keys() {
        let owner = TestOwner::new();
        let keys = Keys::on(Layout::Qwerty);
        let numbers = Rc::default();
        let (first, second) = (Runtime::on(&keys, &numbers), Runtime::on(&keys, &numbers));
        let in_first =
            GlobalShortcuts::new(first.clone(), &owner.ctx(None)).bind(vec![shortcut("1")]);
        let in_second =
            GlobalShortcuts::new(second.clone(), &owner.ctx(None)).bind(vec![shortcut("2")]);
        let (one, _) = first.registered("1").expect("⌘1 is registered");
        let (two, _) = second.registered("2").expect("⌘2 is registered");
        assert!(
            !second.table.heard(one, true),
            "the second runtime passes it on"
        );
        assert!(first.table.heard(one, true));
        assert!(
            !first.table.heard(two, true),
            "the first runtime passes it on"
        );
        assert!(second.table.heard(two, true));
        drop((in_first, in_second));
    }

    #[test]
    fn a_layout_switch_moves_each_shortcut_to_the_key_that_types_it_now() {
        let owner = TestOwner::new();
        let keys = Keys::on(Layout::Qwerty);
        let runtime = Runtime::on(&keys, &Rc::default());
        let shortcuts = GlobalShortcuts::new(runtime.clone(), &owner.ctx(None));
        let all = ["a", "q", "w", "f5", "m"];
        let binding = shortcuts.bind(all.iter().copied().map(shortcut).collect::<Vec<Shortcut>>());
        assert_eq!(next(&owner, &binding), ShortcutEvent::Bound(bound(&all)));
        let code = |name: &str| runtime.registered(name).map(|(_, code)| code);
        assert_eq!(
            all.map(code),
            [Some(0x00), Some(0x0C), Some(0x0D), Some(0x60), Some(0x2E)]
        );
        let (old_a, _) = runtime.registered("a").expect("⌘A is registered");
        let f5 = runtime.registered("f5");

        // A and Q trade keys, which neither could take while the other
        // still held it.
        runtime.layout_changed(Layout::Azerty);
        assert_eq!(
            all.map(code),
            [Some(0x0C), Some(0x00), Some(0x06), Some(0x60), Some(0x29)]
        );
        assert_eq!(runtime.registered("f5"), f5, "a named key stays as it is");
        assert!(
            !runtime.table.heard(old_a, true),
            "the key that types q now is not ⌘A's"
        );
        let (new_a, _) = runtime.registered("a").expect("⌘A moved");
        assert!(runtime.table.heard(new_a, true));
        assert_eq!(
            next(&owner, &binding),
            ShortcutEvent::Pressed(id("a")),
            "every shortcut is still bound, so the binding heard no change"
        );

        // Back on QWERTY another application holds the key ⌘A returns to.
        keys.held_elsewhere.borrow_mut().push((0x00, flag::COMMAND));
        runtime.layout_changed(Layout::Qwerty);
        assert_eq!(runtime.registered("a"), None);
        assert_eq!(
            next(&owner, &binding),
            ShortcutEvent::Changed(bound(&["q", "w", "f5", "m"]))
        );
        // A layout that leaves every key where it is changes nothing.
        runtime.layout_changed(Layout::Qwerty);
        // On AZERTY that key is ⌘Q's: ⌘A is bound again, at its own key,
        // and ⌘Q leaves.
        runtime.layout_changed(Layout::Azerty);
        assert_eq!(code("a"), Some(0x0C));
        assert_eq!(code("q"), None);
        assert_eq!(
            next(&owner, &binding),
            ShortcutEvent::Changed(bound(&["a", "w", "f5", "m"]))
        );

        drop(binding);
        assert!(keys.registered.borrow().is_empty());
        runtime.layout_changed(Layout::Qwerty);
        assert!(
            keys.registered.borrow().is_empty(),
            "a dropped binding is not registered again"
        );
    }

    #[test]
    fn a_process_out_of_hot_key_numbers_binds_nothing_more() {
        let owner = TestOwner::new();
        let keys = Keys::on(Layout::Qwerty);
        // One number is left.
        let numbers = Rc::new(AtomicU64::new(u64::MAX - 1));
        let runtime = Runtime::on(&keys, &numbers);
        let shortcuts = GlobalShortcuts::new(runtime.clone(), &owner.ctx(None));
        let exhausted = ShortcutEvent::Ended(ShortcutError::Exhausted);

        let binding = shortcuts.bind(vec![shortcut("r"), shortcut("s")]);
        assert_eq!(next(&owner, &binding), exhausted);
        assert!(
            keys.registered.borrow().is_empty(),
            "the shortcut that took the last number is not left registered"
        );
        assert!(runtime.table.entries.borrow().is_empty());
        let later = shortcuts.bind(vec![shortcut("r")]);
        assert_eq!(
            next(&owner, &later),
            exhausted,
            "no number is ever handed out again"
        );
        assert!(keys.registered.borrow().is_empty());
    }
}
