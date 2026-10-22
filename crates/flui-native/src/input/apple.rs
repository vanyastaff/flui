//! The Apple hosts' input: AppKit's events at the window's content view
//! (`macos`) and UIKit's touches and Pencil (`ios`), each copied into plain
//! readings for the pointer assembly.
//!
//! - [`keys`]: the AppKit keyboard tables; built on every target and public,
//!   hidden from the documentation, for the backend conformance table in
//!   `tests/`;
//! - `macos`: the content view's class, the responder its window's input
//!   arrives at, and the translation of its `NSEvent`s (`events`);
//! - `ios`: the translation of a view's `UITouch`es.

#[doc(hidden)]
pub mod keys {
    //! macOS (AppKit) keyboard translation decisions.
    //!
    //! Everything the AppKit backend decides about a keystroke from its virtual
    //! keycode — which W3C `code` names the physical key, and which named `Key`
    //! intercepts the `NSEvent.characters` text path — lives here as pure
    //! functions over the raw `NSEvent.keyCode` value.
    //!
    //! Like [`super::keys`] (the Win32 twin this module's shape follows), it is
    //! deliberately free of `cfg` gates and platform types so the tables compile
    //! — and their tests execute — on any host, even though the AppKit caller
    //! (`macos::events`) itself is only ever clippy-linted in CI,
    //! never linked or run.
    //!
    //! # References
    //!
    //! macOS virtual keycodes are position-based (they name the physical key,
    //! not the character it types) and are defined once, as the `kVK_*` constants
    //! in Apple's Carbon `HIToolbox/Events.h` — still the canonical source; the
    //! [`kvk`] module transcribes it. The `Code` each keycode maps to follows the
    //! W3C uievents-code registry (<https://www.w3.org/TR/uievents-code/>) as
    //! implemented by Firefox (`NativeKeyToDOMCodeName.h`, the `CODE_MAP_MAC`
    //! rows) and the winit macOS backend (winit 0.30
    //! `platform_impl/macos/event.rs`, `scancode_to_physicalkey`). Where those
    //! two disagree the table follows Apple's header and Firefox, and the
    //! divergence is commented at the arm.
    //!
    //! # What never reaches these tables
    //!
    //! AppKit reports modifier keys through `flagsChanged:`, not
    //! `keyDown:`/`keyUp:` — the modifier rows below (`kVK_Shift` …
    //! `kVK_Function`) are complete for the table's own sake, but the backend
    //! does not receive plain key events for them, and it does not translate
    //! `flagsChanged:` today; modifier *state* rides every event's
    //! `modifierFlags` instead.

    #![forbid(unsafe_code)]

    use flui_platform::keyboard::{Code, Key, NamedKey};

    /// Apple's `kVK_*` virtual keycodes, from Carbon `HIToolbox/Events.h`.
    ///
    /// `ANSI_*` names mean "the key in this position on a US (ANSI) keyboard" —
    /// the keycode itself is layout-independent. Raw `u16` (the width of
    /// `NSEvent.keyCode`) rather than a binding crate's type so this module
    /// compiles on every host.
    #[expect(missing_docs)]
    pub mod kvk {
        pub const ANSI_A: u16 = 0x00;
        pub const ANSI_S: u16 = 0x01;
        pub const ANSI_D: u16 = 0x02;
        pub const ANSI_F: u16 = 0x03;
        pub const ANSI_H: u16 = 0x04;
        pub const ANSI_G: u16 = 0x05;
        pub const ANSI_Z: u16 = 0x06;
        pub const ANSI_X: u16 = 0x07;
        pub const ANSI_C: u16 = 0x08;
        pub const ANSI_V: u16 = 0x09;
        pub const ISO_SECTION: u16 = 0x0A;
        pub const ANSI_B: u16 = 0x0B;
        pub const ANSI_Q: u16 = 0x0C;
        pub const ANSI_W: u16 = 0x0D;
        pub const ANSI_E: u16 = 0x0E;
        pub const ANSI_R: u16 = 0x0F;
        pub const ANSI_Y: u16 = 0x10;
        pub const ANSI_T: u16 = 0x11;
        pub const ANSI_1: u16 = 0x12;
        pub const ANSI_2: u16 = 0x13;
        pub const ANSI_3: u16 = 0x14;
        pub const ANSI_4: u16 = 0x15;
        pub const ANSI_6: u16 = 0x16;
        pub const ANSI_5: u16 = 0x17;
        pub const ANSI_EQUAL: u16 = 0x18;
        pub const ANSI_9: u16 = 0x19;
        pub const ANSI_7: u16 = 0x1A;
        pub const ANSI_MINUS: u16 = 0x1B;
        pub const ANSI_8: u16 = 0x1C;
        pub const ANSI_0: u16 = 0x1D;
        pub const ANSI_RIGHT_BRACKET: u16 = 0x1E;
        pub const ANSI_O: u16 = 0x1F;
        pub const ANSI_U: u16 = 0x20;
        pub const ANSI_LEFT_BRACKET: u16 = 0x21;
        pub const ANSI_I: u16 = 0x22;
        pub const ANSI_P: u16 = 0x23;
        pub const RETURN: u16 = 0x24;
        pub const ANSI_L: u16 = 0x25;
        pub const ANSI_J: u16 = 0x26;
        pub const ANSI_QUOTE: u16 = 0x27;
        pub const ANSI_K: u16 = 0x28;
        pub const ANSI_SEMICOLON: u16 = 0x29;
        pub const ANSI_BACKSLASH: u16 = 0x2A;
        pub const ANSI_COMMA: u16 = 0x2B;
        pub const ANSI_SLASH: u16 = 0x2C;
        pub const ANSI_N: u16 = 0x2D;
        pub const ANSI_M: u16 = 0x2E;
        pub const ANSI_PERIOD: u16 = 0x2F;
        pub const TAB: u16 = 0x30;
        pub const SPACE: u16 = 0x31;
        pub const ANSI_GRAVE: u16 = 0x32;
        /// `kVK_Delete` is the key labeled "delete" on Apple keyboards — the
        /// BACKSPACE position; forward delete is [`FORWARD_DELETE`].
        pub const DELETE: u16 = 0x33;
        pub const ESCAPE: u16 = 0x35;
        pub const RIGHT_COMMAND: u16 = 0x36;
        pub const COMMAND: u16 = 0x37;
        pub const SHIFT: u16 = 0x38;
        pub const CAPS_LOCK: u16 = 0x39;
        pub const OPTION: u16 = 0x3A;
        pub const CONTROL: u16 = 0x3B;
        pub const RIGHT_SHIFT: u16 = 0x3C;
        pub const RIGHT_OPTION: u16 = 0x3D;
        pub const RIGHT_CONTROL: u16 = 0x3E;
        pub const FUNCTION: u16 = 0x3F;
        pub const F17: u16 = 0x40;
        pub const ANSI_KEYPAD_DECIMAL: u16 = 0x41;
        pub const ANSI_KEYPAD_MULTIPLY: u16 = 0x43;
        pub const ANSI_KEYPAD_PLUS: u16 = 0x45;
        /// The numpad "clear" key — the NumLock position of a PC numpad.
        pub const ANSI_KEYPAD_CLEAR: u16 = 0x47;
        pub const VOLUME_UP: u16 = 0x48;
        pub const VOLUME_DOWN: u16 = 0x49;
        pub const MUTE: u16 = 0x4A;
        pub const ANSI_KEYPAD_DIVIDE: u16 = 0x4B;
        pub const ANSI_KEYPAD_ENTER: u16 = 0x4C;
        pub const ANSI_KEYPAD_MINUS: u16 = 0x4E;
        pub const F18: u16 = 0x4F;
        pub const F19: u16 = 0x50;
        pub const ANSI_KEYPAD_EQUALS: u16 = 0x51;
        pub const ANSI_KEYPAD_0: u16 = 0x52;
        pub const ANSI_KEYPAD_1: u16 = 0x53;
        pub const ANSI_KEYPAD_2: u16 = 0x54;
        pub const ANSI_KEYPAD_3: u16 = 0x55;
        pub const ANSI_KEYPAD_4: u16 = 0x56;
        pub const ANSI_KEYPAD_5: u16 = 0x57;
        pub const ANSI_KEYPAD_6: u16 = 0x58;
        pub const ANSI_KEYPAD_7: u16 = 0x59;
        pub const F20: u16 = 0x5A;
        pub const ANSI_KEYPAD_8: u16 = 0x5B;
        pub const ANSI_KEYPAD_9: u16 = 0x5C;
        pub const JIS_YEN: u16 = 0x5D;
        pub const JIS_UNDERSCORE: u16 = 0x5E;
        pub const JIS_KEYPAD_COMMA: u16 = 0x5F;
        pub const F5: u16 = 0x60;
        pub const F6: u16 = 0x61;
        pub const F7: u16 = 0x62;
        pub const F3: u16 = 0x63;
        pub const F8: u16 = 0x64;
        pub const F9: u16 = 0x65;
        pub const JIS_EISU: u16 = 0x66;
        pub const F11: u16 = 0x67;
        pub const JIS_KANA: u16 = 0x68;
        pub const F13: u16 = 0x69;
        pub const F16: u16 = 0x6A;
        pub const F14: u16 = 0x6B;
        pub const F10: u16 = 0x6D;
        /// The PC-keyboard Menu key. `Events.h` defines no constant for `0x6E`;
        /// Firefox and Chromium both map it to `ContextMenu`.
        pub const PC_MENU: u16 = 0x6E;
        pub const F12: u16 = 0x6F;
        pub const F15: u16 = 0x71;
        /// The Help key on Apple pro keyboards — the Insert position of a PC
        /// keyboard, and `Insert` is what the winit backend and Chromium call
        /// it (Firefox says `Help`); this table sides with the winit wire so
        /// FLUI's two macOS backends agree.
        pub const HELP: u16 = 0x72;
        pub const HOME: u16 = 0x73;
        pub const PAGE_UP: u16 = 0x74;
        pub const FORWARD_DELETE: u16 = 0x75;
        pub const F4: u16 = 0x76;
        pub const END: u16 = 0x77;
        pub const F2: u16 = 0x78;
        pub const PAGE_DOWN: u16 = 0x79;
        pub const F1: u16 = 0x7A;
        pub const LEFT_ARROW: u16 = 0x7B;
        pub const RIGHT_ARROW: u16 = 0x7C;
        pub const DOWN_ARROW: u16 = 0x7D;
        pub const UP_ARROW: u16 = 0x7E;
    }

    /// Map an `NSEvent.keyCode` (a `kVK_*` virtual keycode) to the W3C `code`
    /// value naming the physical key.
    ///
    /// Two arms deliberately diverge from the winit macOS backend's table, both
    /// toward Apple's `Events.h` and Firefox:
    ///
    /// - **Volume keys.** `Events.h` says `kVK_VolumeUp`/`Down`/`Mute` are
    ///   `0x48`/`0x49`/`0x4A`; winit maps `0x49 → VolumeUp` and `0x4A →
    ///   VolumeDown` (dropping Mute), one code off, with a `TODO` in its own
    ///   source questioning the choice.
    /// - **The ISO section key** (`0x0A`, between Left Shift and Z on ISO
    ///   boards). The registry and Firefox name that position `IntlBackslash`;
    ///   winit maps it to `Backquote` because German QWERTZ prints `^` there.
    pub fn keycode_to_code(key_code: u16) -> Code {
        match key_code {
            kvk::ANSI_A => Code::KeyA,
            kvk::ANSI_S => Code::KeyS,
            kvk::ANSI_D => Code::KeyD,
            kvk::ANSI_F => Code::KeyF,
            kvk::ANSI_H => Code::KeyH,
            kvk::ANSI_G => Code::KeyG,
            kvk::ANSI_Z => Code::KeyZ,
            kvk::ANSI_X => Code::KeyX,
            kvk::ANSI_C => Code::KeyC,
            kvk::ANSI_V => Code::KeyV,
            kvk::ISO_SECTION => Code::IntlBackslash,
            kvk::ANSI_B => Code::KeyB,
            kvk::ANSI_Q => Code::KeyQ,
            kvk::ANSI_W => Code::KeyW,
            kvk::ANSI_E => Code::KeyE,
            kvk::ANSI_R => Code::KeyR,
            kvk::ANSI_Y => Code::KeyY,
            kvk::ANSI_T => Code::KeyT,
            kvk::ANSI_1 => Code::Digit1,
            kvk::ANSI_2 => Code::Digit2,
            kvk::ANSI_3 => Code::Digit3,
            kvk::ANSI_4 => Code::Digit4,
            kvk::ANSI_6 => Code::Digit6,
            kvk::ANSI_5 => Code::Digit5,
            kvk::ANSI_EQUAL => Code::Equal,
            kvk::ANSI_9 => Code::Digit9,
            kvk::ANSI_7 => Code::Digit7,
            kvk::ANSI_MINUS => Code::Minus,
            kvk::ANSI_8 => Code::Digit8,
            kvk::ANSI_0 => Code::Digit0,
            kvk::ANSI_RIGHT_BRACKET => Code::BracketRight,
            kvk::ANSI_O => Code::KeyO,
            kvk::ANSI_U => Code::KeyU,
            kvk::ANSI_LEFT_BRACKET => Code::BracketLeft,
            kvk::ANSI_I => Code::KeyI,
            kvk::ANSI_P => Code::KeyP,
            kvk::RETURN => Code::Enter,
            kvk::ANSI_L => Code::KeyL,
            kvk::ANSI_J => Code::KeyJ,
            kvk::ANSI_QUOTE => Code::Quote,
            kvk::ANSI_K => Code::KeyK,
            kvk::ANSI_SEMICOLON => Code::Semicolon,
            kvk::ANSI_BACKSLASH => Code::Backslash,
            kvk::ANSI_COMMA => Code::Comma,
            kvk::ANSI_SLASH => Code::Slash,
            kvk::ANSI_N => Code::KeyN,
            kvk::ANSI_M => Code::KeyM,
            kvk::ANSI_PERIOD => Code::Period,
            kvk::TAB => Code::Tab,
            kvk::SPACE => Code::Space,
            kvk::ANSI_GRAVE => Code::Backquote,
            kvk::DELETE => Code::Backspace,
            kvk::ESCAPE => Code::Escape,
            kvk::RIGHT_COMMAND => Code::MetaRight,
            kvk::COMMAND => Code::MetaLeft,
            kvk::SHIFT => Code::ShiftLeft,
            kvk::CAPS_LOCK => Code::CapsLock,
            kvk::OPTION => Code::AltLeft,
            kvk::CONTROL => Code::ControlLeft,
            kvk::RIGHT_SHIFT => Code::ShiftRight,
            kvk::RIGHT_OPTION => Code::AltRight,
            kvk::RIGHT_CONTROL => Code::ControlRight,
            kvk::FUNCTION => Code::Fn,
            kvk::F17 => Code::F17,
            kvk::ANSI_KEYPAD_DECIMAL => Code::NumpadDecimal,
            kvk::ANSI_KEYPAD_MULTIPLY => Code::NumpadMultiply,
            kvk::ANSI_KEYPAD_PLUS => Code::NumpadAdd,
            // The registry names the numpad-clear position NumLock (it IS the
            // NumLock key of a PC numpad).
            kvk::ANSI_KEYPAD_CLEAR => Code::NumLock,
            kvk::VOLUME_UP => Code::AudioVolumeUp,
            kvk::VOLUME_DOWN => Code::AudioVolumeDown,
            kvk::MUTE => Code::AudioVolumeMute,
            kvk::ANSI_KEYPAD_DIVIDE => Code::NumpadDivide,
            kvk::ANSI_KEYPAD_ENTER => Code::NumpadEnter,
            kvk::ANSI_KEYPAD_MINUS => Code::NumpadSubtract,
            kvk::F18 => Code::F18,
            kvk::F19 => Code::F19,
            kvk::ANSI_KEYPAD_EQUALS => Code::NumpadEqual,
            kvk::ANSI_KEYPAD_0 => Code::Numpad0,
            kvk::ANSI_KEYPAD_1 => Code::Numpad1,
            kvk::ANSI_KEYPAD_2 => Code::Numpad2,
            kvk::ANSI_KEYPAD_3 => Code::Numpad3,
            kvk::ANSI_KEYPAD_4 => Code::Numpad4,
            kvk::ANSI_KEYPAD_5 => Code::Numpad5,
            kvk::ANSI_KEYPAD_6 => Code::Numpad6,
            kvk::ANSI_KEYPAD_7 => Code::Numpad7,
            kvk::F20 => Code::F20,
            kvk::ANSI_KEYPAD_8 => Code::Numpad8,
            kvk::ANSI_KEYPAD_9 => Code::Numpad9,
            kvk::JIS_YEN => Code::IntlYen,
            kvk::JIS_UNDERSCORE => Code::IntlRo,
            kvk::JIS_KEYPAD_COMMA => Code::NumpadComma,
            kvk::F5 => Code::F5,
            kvk::F6 => Code::F6,
            kvk::F7 => Code::F7,
            kvk::F3 => Code::F3,
            kvk::F8 => Code::F8,
            kvk::F9 => Code::F9,
            kvk::JIS_EISU => Code::Lang2,
            kvk::F11 => Code::F11,
            kvk::JIS_KANA => Code::Lang1,
            kvk::F13 => Code::F13,
            kvk::F16 => Code::F16,
            kvk::F14 => Code::F14,
            kvk::F10 => Code::F10,
            kvk::PC_MENU => Code::ContextMenu,
            kvk::F12 => Code::F12,
            kvk::F15 => Code::F15,
            kvk::HELP => Code::Insert,
            kvk::HOME => Code::Home,
            kvk::PAGE_UP => Code::PageUp,
            kvk::FORWARD_DELETE => Code::Delete,
            kvk::F4 => Code::F4,
            kvk::END => Code::End,
            kvk::F2 => Code::F2,
            kvk::PAGE_DOWN => Code::PageDown,
            kvk::F1 => Code::F1,
            kvk::LEFT_ARROW => Code::ArrowLeft,
            kvk::RIGHT_ARROW => Code::ArrowRight,
            kvk::DOWN_ARROW => Code::ArrowDown,
            kvk::UP_ARROW => Code::ArrowUp,
            _ => Code::Unidentified,
        }
    }

    /// The named `Key` a keycode resolves to *before* the backend consults
    /// `NSEvent.characters` — `None` means "let the characters text decide".
    ///
    /// This intercept exists because for these keys `characters` is not typeable
    /// text: editing/navigation/function keys deliver control characters or
    /// Unicode private-use-area codepoints (`NSUpArrowFunctionKey` is `U+F700`,
    /// `NSF13FunctionKey` is `U+F70A`, …), which would otherwise ship as
    /// garbage `Key::Character` values. Letters, digits, and punctuation return
    /// `None` on purpose: their `characters` string is the layout- and
    /// modifier-aware translation (the macOS analogue of the Win32 `WM_CHAR`
    /// merge in [`super::keys`]) and must keep winning.
    pub fn keycode_to_key(key_code: u16) -> Option<Key> {
        let named = match key_code {
            kvk::LEFT_ARROW => NamedKey::ArrowLeft,
            kvk::RIGHT_ARROW => NamedKey::ArrowRight,
            kvk::DOWN_ARROW => NamedKey::ArrowDown,
            kvk::UP_ARROW => NamedKey::ArrowUp,

            kvk::F1 => NamedKey::F1,
            kvk::F2 => NamedKey::F2,
            kvk::F3 => NamedKey::F3,
            kvk::F4 => NamedKey::F4,
            kvk::F5 => NamedKey::F5,
            kvk::F6 => NamedKey::F6,
            kvk::F7 => NamedKey::F7,
            kvk::F8 => NamedKey::F8,
            kvk::F9 => NamedKey::F9,
            kvk::F10 => NamedKey::F10,
            kvk::F11 => NamedKey::F11,
            kvk::F12 => NamedKey::F12,
            kvk::F13 => NamedKey::F13,
            kvk::F14 => NamedKey::F14,
            kvk::F15 => NamedKey::F15,
            kvk::F16 => NamedKey::F16,
            kvk::F17 => NamedKey::F17,
            kvk::F18 => NamedKey::F18,
            kvk::F19 => NamedKey::F19,
            kvk::F20 => NamedKey::F20,

            // Both Enter keys report the same logical key; the numpad one is
            // distinguished by its `code`/`location`.
            kvk::RETURN | kvk::ANSI_KEYPAD_ENTER => NamedKey::Enter,
            kvk::TAB => NamedKey::Tab,
            kvk::DELETE => NamedKey::Backspace,
            kvk::ESCAPE => NamedKey::Escape,
            kvk::FORWARD_DELETE => NamedKey::Delete,
            kvk::HOME => NamedKey::Home,
            kvk::END => NamedKey::End,
            kvk::PAGE_UP => NamedKey::PageUp,
            kvk::PAGE_DOWN => NamedKey::PageDown,
            // Insert, matching this key's `Code` (see [`kvk::HELP`]).
            kvk::HELP => NamedKey::Insert,
            kvk::PC_MENU => NamedKey::ContextMenu,
            // Keypad Clear delivers the PUA scalar `NSClearLineFunctionKey`
            // (U+F739, AppKit `NSEvent.h`) as its `characters`; its W3C `key`
            // value is `Clear` (Firefox maps `kVK_ANSI_KeypadClear` so), even
            // though its physical `code` is the registry's NumLock position.
            kvk::ANSI_KEYPAD_CLEAR => NamedKey::Clear,

            // Modifiers arrive via `flagsChanged:`, not `keyDown:` — mapped for
            // table completeness (see the module doc).
            kvk::SHIFT | kvk::RIGHT_SHIFT => NamedKey::Shift,
            kvk::CONTROL | kvk::RIGHT_CONTROL => NamedKey::Control,
            kvk::OPTION | kvk::RIGHT_OPTION => NamedKey::Alt,
            kvk::COMMAND | kvk::RIGHT_COMMAND => NamedKey::Meta,
            kvk::CAPS_LOCK => NamedKey::CapsLock,
            kvk::FUNCTION => NamedKey::Fn,

            kvk::VOLUME_UP => NamedKey::AudioVolumeUp,
            kvk::VOLUME_DOWN => NamedKey::AudioVolumeDown,
            kvk::MUTE => NamedKey::AudioVolumeMute,

            kvk::JIS_EISU => NamedKey::Eisu,
            kvk::JIS_KANA => NamedKey::KanaMode,

            // Space types text but its `characters` IS the space — still, keep
            // the historical intercept so the key never depends on the
            // characters path.
            kvk::SPACE => return Some(Key::character(" ")),

            _ => return None,
        };
        Some(Key::Named(named))
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod macos {
    //! macOS NSView implementation for input events
    //!
    //! Creates a custom NSView subclass (FLUIContentView) that receives keyboard,
    //! mouse, and scroll events through the NSResponder chain.
    //!
    //! # Architecture
    //!
    //! ```text
    //! NSEvent (OS)
    //!     ↓
    //! NSApplication.sendEvent:
    //!     ↓
    //! NSWindow.sendEvent:
    //!     ↓
    //! FLUIContentView (first responder)
    //!     ↓
    //! keyDown:/mouseDown:/etc.
    //!     ↓
    //! MacInputState::convert()
    //!     ↓
    //! WindowCallbacks::dispatch_input
    //! ```
    //!
    //! # Migration note
    //!
    //! The class is built with objc2's imperative `ClassBuilder` (the successor to
    //! `objc` 0.2's `ClassDecl`) rather than the declarative `define_class!` macro,
    //! because the `NSTextInputClient` methods are registered by a *separate*
    //! function in [`crate::text::apple::macos`] — the macro wants all methods in one
    //! block, and the split is what keeps the IME conformance's eleven callbacks
    //! beside the state they read.

    // AppKit's classes are reached through Objective-C messages, which have
    // no safe wrapper; a handful of the accessors used are deprecated in the
    // multi-scene era, and each use says so where it is made.
    #![expect(unsafe_code)]
    #![expect(deprecated)]

    use std::cell::{Cell, RefCell};
    use std::ffi::CStr;
    use std::sync::Weak;

    use flui_platform::WindowAppearance;
    use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Sel};
    use objc2::{ClassType, msg_send};
    use objc2_app_kit::{
        NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSResponder,
        NSView as NSViewClass,
    };
    use objc2_foundation::{NSArray, NSRect, NSRectEdge};

    use crate::owner::handlers::WindowCallbacks;
    use crate::text::apple::macos::{TextInputState, add_text_input_methods};
    use events::MacInputState;

    // ============================================================================
    // FLUIContentView Creation
    // ============================================================================

    /// The ivar name holding the boxed [`ViewContext`].
    const CONTEXT_IVAR: &CStr = c"context_ptr";

    /// Create a content view for receiving input events
    ///
    /// This view becomes the NSWindow's contentView and first responder,
    /// receiving all keyboard, mouse, and scroll events.
    pub fn create_content_view(
        frame: NSRect,
        scale_factor: f64,
        callbacks: Weak<WindowCallbacks>,
    ) -> *mut AnyObject {
        // SAFETY: FLUIContentView is registered before alloc/init; the boxed
        // ViewContext pointer is stored in the view's ivar and released in
        // `dealloc`, so it lives exactly as long as the view.
        unsafe {
            let class = get_or_create_view_class();
            let view: *mut AnyObject = msg_send![class, alloc];
            let view: *mut AnyObject = msg_send![view, initWithFrame: frame];

            // Store context (scale factor + callbacks + composition state)
            let context = Box::into_raw(Box::new(ViewContext {
                scale_factor,
                callbacks,
                text_input: RefCell::new(TextInputState::default()),
                input: RefCell::new(MacInputState::default()),
                appearance: Cell::new(effective_appearance(as_view(&*view))),
            }))
            .cast::<std::ffi::c_void>();

            // SAFETY: `view` is a live FLUIContentView just allocated above, so it
            // carries `CONTEXT_IVAR`; the value is a pointer this function owns.
            let ivar = class
                .instance_variable(CONTEXT_IVAR)
                .expect("BUG: FLUIContentView declares the context ivar");
            let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(&*view);
            *slot = context;

            view
        }
    }

    /// Context stored in NSView ivar
    pub(crate) struct ViewContext {
        scale_factor: f64,
        pub(super) callbacks: Weak<WindowCallbacks>,

        /// The `NSTextInputClient` composition state this view is driven through
        /// (see [`crate::text::apple::macos`]). A `RefCell` because the AppKit callbacks and
        /// the routed [`crate::text::apple::macos::MacOSTextInput`] bodies both reach it
        /// through a shared `&ViewContext`, and every one of them either runs on the
        /// main thread (AppKit's delivery contract) or under the owner-lane guard.
        pub(crate) text_input: RefCell<TextInputState>,
        input: RefCell<MacInputState>,
        /// The appearance last reported to `on_appearance_changed`; AppKit also
        /// re-resolves the effective appearance for changes that keep the theme,
        /// such as the accent colour.
        appearance: Cell<WindowAppearance>,
    }

    // ============================================================================
    // NSView Class Definition
    // ============================================================================

    /// Handle an input NSEvent arriving at a FLUIContentView method.
    ///
    /// Converts the event and dispatches it through the per-window callbacks.
    ///
    /// `pub(crate)` because `text::apple::macos`'s `doCommandBySelector:` is the
    /// second caller: the key event an input method declined is handed back here to
    /// take the ordinary keyboard path.
    pub(crate) extern "C-unwind" fn handle_input_event(
        this: &AnyObject,
        _sel: Sel,
        event: *mut AnyObject,
    ) {
        // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
        // live objects); `event` is a valid NSEvent* for the duration of the call;
        // `bounds` is a plain NSRect getter.
        unsafe {
            if let Some(ctx) = get_context(this) {
                let bounds: NSRect = msg_send![this, bounds];
                let inputs = ctx
                    .input
                    .borrow_mut()
                    .convert(event.cast::<std::ffi::c_void>(), bounds.size.height);
                for input in inputs {
                    dispatch_input_event(ctx, input);
                }
            }
        }
    }

    /// `keyDown:` — the input-context route while a text input is attached, the
    /// keyboard route otherwise.
    ///
    /// The gate is load-bearing and its default is the load-bearing part: with
    /// `ime_allowed == false` — the state of every window until a presentation
    /// attaches a text input and enables composition — this is byte-identical to
    /// wiring `keyDown:` straight to [`handle_input_event`], which is what it did
    /// before the `NSTextInputClient` conformance existed. Only while a text input
    /// IS attached does the event take AppKit's input method instead, through
    /// `interpretKeyEvents:`: that call comes back into this view's
    /// `NSTextInputClient` methods (`setMarkedText:` while composing, `insertText:`
    /// on commit), so a key delivered to both routes would reach the application
    /// twice from one physical press — an `ImeEvent::Commit` *and* a
    /// `Key::Character`. `doCommandBySelector:` is the way back for the keys the
    /// input method declines.
    ///
    /// `pending_key_event` is set for exactly the duration of the
    /// `interpretKeyEvents:` call, which is the only window in which the input
    /// context can call `doCommandBySelector:` back for this event.
    extern "C-unwind" fn key_down(this: &AnyObject, _sel: Sel, event: *mut AnyObject) {
        // SAFETY: `this` is a live FLUIContentView; `event` is a valid NSEvent* for
        // the duration of the call, and `interpretKeyEvents:` retains it for the
        // span of its own dispatch. The array is created by an NSArray class
        // constructor and autoreleased.
        unsafe {
            let Some(ctx) = get_context(this) else {
                return;
            };
            // The gate, and the whole of the change this callback makes to a window
            // that never attaches a text input: while no input context is attached
            // the key takes the keyboard path below, byte for byte as `keyDown:`
            // did before `NSTextInputClient` existed. Only an attached text input
            // makes AppKit's input method a *second* producer for the same press,
            // and a press must reach the application exactly once — either as a
            // composition/commit through the input context, or as a key event
            // through the keyboard conversion, never both (see the module doc of
            // `text::apple::macos`).
            let ime_allowed = ctx.text_input.borrow().ime_allowed;
            if !ime_allowed {
                handle_input_event(this, objc2::sel!(keyDown:), event);
                return;
            }

            // Saved and restored rather than set and cleared: an input method can
            // route a *second* `keyDown:` through the view while this one is inside
            // `interpretKeyEvents:` (the character palette does), and a plain clear
            // would leave the outer event's `doCommandBySelector:` reading zero and
            // dropping a command that belongs to this press.
            let previous_pending_key_event = ctx.text_input.borrow().pending_key_event;
            ctx.text_input.borrow_mut().pending_key_event = event as usize;
            let events: *mut AnyObject = msg_send![
                objc2::class!(NSArray),
                arrayWithObject: event
            ];
            let _: () = msg_send![this, interpretKeyEvents: events];
            ctx.text_input.borrow_mut().pending_key_event = previous_pending_key_event;
        }
    }

    /// `keyUp:` — the keyboard route, except while a composition is in flight.
    ///
    /// Gated on the *composition* rather than on `ime_allowed`, which is the
    /// distinction winit draws in its own `keyUp:` arm: it queues the release only
    /// from its `Ground` and `Disabled` states, so an active preedit suppresses it.
    /// The reason is the same one [`key_down`] exists for, on the opposite edge —
    /// the press was consumed by the input method, so a release reported while the
    /// composition is still open describes a key the application never saw go down.
    ///
    /// A commit returns the state to `Ground` before the release arrives (winit
    /// resets it in `keyDown:`, and [`TextInputState::clear_marked_text`] empties
    /// the composition here), so an ordinary typed character still reports its
    /// release normally. What is suppressed is exactly the release that lands
    /// inside an open composition.
    extern "C-unwind" fn key_up(this: &AnyObject, _sel: Sel, event: *mut AnyObject) {
        // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
        // live objects); `event` is a valid NSEvent* for the duration of the call.
        let reported = unsafe {
            get_context(this).is_none_or(|ctx| ctx.text_input.borrow().reports_key_release())
        };
        if !reported {
            return;
        }
        handle_input_event(this, objc2::sel!(keyUp:), event);
    }

    /// Dispatch a hover status change through the per-window callbacks.
    fn dispatch_hover_change(this: &AnyObject, is_hovered: bool) {
        // SAFETY: `this` is a live FLUIContentView (AppKit only invokes methods on
        // live objects); `get_context`'s ivar contract holds for views created by
        // `create_content_view`.
        unsafe {
            if let Some(ctx) = get_context(this)
                && let Some(callbacks) = ctx.callbacks.upgrade()
            {
                callbacks.dispatch_hover_status_change(is_hovered);
            }
        }
    }

    /// mouseEntered: — report hover gained, then forward the pointer event.
    extern "C-unwind" fn mouse_entered(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
        dispatch_hover_change(this, true);
        handle_input_event(this, sel, event);
    }

    /// mouseExited: — report hover lost, then forward the pointer event.
    extern "C-unwind" fn mouse_exited(this: &AnyObject, sel: Sel, event: *mut AnyObject) {
        dispatch_hover_change(this, false);
        handle_input_event(this, sel, event);
    }

    /// drawRect: — the platform asks for content; forward as a frame request.
    ///
    /// `request_redraw()` marks the view dirty via `setNeedsDisplay:`; AppKit then
    /// calls this method on the next display pass, which is where the
    /// per-window `on_request_frame` contract fires (the macOS analogue of the
    /// Windows backend's WM_PAINT dispatch).
    ///
    /// The frame runs inside this callback, so any redraw the frame asks for is
    /// asked for *while AppKit is displaying this view* — the one moment a
    /// `setNeedsDisplay:` is discarded. [`DisplayPassGuard`] marks this thread for
    /// the duration so `request_redraw` can tell that case apart from a request
    /// made at any other moment and defer only the former.
    ///
    /// [`DisplayPassGuard`]: crate::window::apple::macos::display_pass::DisplayPassGuard
    extern "C-unwind" fn draw_rect(this: &AnyObject, _sel: Sel, dirty_rect: NSRect) {
        // SAFETY: `this` is a live FLUIContentView; the super `drawRect:` message
        // is the documented NSView teardown of the dirty region.
        unsafe {
            let _display_pass =
                crate::window::apple::macos::display_pass::DisplayPassGuard::enter();
            if let Some(callbacks) = get_context(this).and_then(|ctx| ctx.callbacks.upgrade()) {
                tracing::trace!("FLUIContentView drawRect: dispatching frame request");
                callbacks.dispatch_request_frame();
            } else {
                // The window's callbacks are gone (torn down, or the view outlived
                // its window). AppKit still owns this display pass, so the super
                // call below runs either way — but a missing callback here is why a
                // frame that AppKit asked for produced no frame request, which is
                // otherwise indistinguishable from AppKit never asking at all.
                tracing::trace!(
                    "FLUIContentView drawRect: no live callbacks; frame request dropped"
                );
            }

            let _: () = msg_send![super(this, NSViewClass::class()), drawRect: dirty_rect];
        }
    }

    /// The light/dark half of `object`'s effective appearance: the system theme,
    /// unless the app or a window overrides it.
    pub(crate) fn effective_appearance(
        object: &(impl NSAppearanceCustomization + objc2::Message),
    ) -> WindowAppearance {
        // SAFETY: AppKit exports both names as constant strings that live for the
        // process; reading the extern statics only copies those references.
        let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        let names = NSArray::from_slice(&[aqua, dark]);
        match object
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&names)
        {
            Some(name) if &*name == dark => WindowAppearance::Dark,
            _ => WindowAppearance::Light,
        }
    }

    /// `this`, a FLUIContentView, as the NSView it subclasses.
    fn as_view(this: &AnyObject) -> &NSViewClass {
        this.downcast_ref::<NSViewClass>()
            .expect("BUG: FLUIContentView subclasses NSView")
    }

    /// `viewDidChangeEffectiveAppearance` — AppKit re-resolved this view's
    /// appearance: the system theme flipped, or something that keeps it did.
    extern "C-unwind" fn view_did_change_effective_appearance(this: &AnyObject, _sel: Sel) {
        // SAFETY: `this` is a live FLUIContentView, messaged by AppKit on the main
        // thread; the super call is NSView's own (empty) implementation.
        unsafe {
            let _: () = msg_send![
                super(this, NSViewClass::class()),
                viewDidChangeEffectiveAppearance
            ];
            let Some(ctx) = get_context(this) else {
                return;
            };
            let appearance = effective_appearance(as_view(this));
            if ctx.appearance.replace(appearance) != appearance
                && let Some(callbacks) = ctx.callbacks.upgrade()
            {
                callbacks.dispatch_appearance_changed();
            }
        }
    }

    /// Get or create the FLUIContentView class
    fn get_or_create_view_class() -> &'static AnyClass {
        use std::sync::Once;
        static INIT: Once = Once::new();

        INIT.call_once(|| {
            let superclass = NSViewClass::class();
            let mut builder = objc2::runtime::ClassBuilder::new(c"FLUIContentView", superclass)
                .expect("FLUIContentView must be registered exactly once (guarded by Once)");

            // Add ivar to store context (a raw `*mut c_void` boxed ViewContext).
            builder.add_ivar::<*mut std::ffi::c_void>(CONTEXT_IVAR);

            // =================================================================
            // NSResponder Methods (Input Events)
            // =================================================================

            // acceptsFirstResponder - Allow view to become first responder
            extern "C-unwind" fn accepts_first_responder(_this: &AnyObject, _sel: Sel) -> Bool {
                Bool::YES
            }

            // becomeFirstResponder
            extern "C-unwind" fn become_first_responder(_this: &AnyObject, _sel: Sel) -> Bool {
                tracing::debug!("FLUIContentView became first responder");
                Bool::YES
            }

            // resignFirstResponder
            extern "C-unwind" fn resign_first_responder(_this: &AnyObject, _sel: Sel) -> Bool {
                tracing::debug!("FLUIContentView resigned first responder");
                Bool::YES
            }

            // flagsChanged: — modifier keys (Shift, Control, Alt, Command).
            // Modifier state is carried on every converted event, so flag-only
            // transitions are observed but not dispatched separately.
            extern "C-unwind" fn flags_changed(
                _this: &AnyObject,
                _sel: Sel,
                _event: *mut AnyObject,
            ) {
                tracing::trace!("Modifier flags changed");
            }

            // =================================================================
            // View Lifecycle
            // =================================================================

            extern "C-unwind" fn dealloc(this: &AnyObject, _sel: Sel) {
                // SAFETY: the ivar holds either null or a Box<ViewContext> leaked
                // in `create_content_view`; reclaiming it here (exactly once, on
                // dealloc) is the matching release. The super dealloc message is
                // the mandatory NSObject teardown.
                unsafe {
                    let class = AnyClass::get(c"FLUIContentView")
                        .expect("BUG: dealloc only runs on a registered FLUIContentView");
                    let ivar = class
                        .instance_variable(CONTEXT_IVAR)
                        .expect("BUG: FLUIContentView declares the context ivar");
                    let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(this);
                    let context_ptr = *slot;
                    if !context_ptr.is_null() {
                        drop(Box::from_raw(context_ptr.cast::<ViewContext>()));
                    }

                    let _: () = msg_send![super(this, NSViewClass::class()), dealloc];
                }
            }

            // =================================================================
            // View Drawing (Optional)
            // =================================================================

            extern "C-unwind" fn is_opaque(_this: &AnyObject, _sel: Sel) -> Bool {
                Bool::YES // Our view is fully opaque
            }

            extern "C-unwind" fn accepts_touch_events(_this: &AnyObject, _sel: Sel) -> Bool {
                Bool::YES // Accept touch events for future trackpad gestures
            }

            // =================================================================
            // Add Methods to Class
            // =================================================================

            // SAFETY: every registered function pointer matches the Objective-C
            // method signature of its selector (`&AnyObject, Sel` plus the declared
            // argument/return types), as required by `ClassBuilder::add_method`.
            unsafe {
                // First responder
                builder.add_method(
                    objc2::sel!(acceptsFirstResponder),
                    accepts_first_responder as extern "C-unwind" fn(_, _) -> Bool,
                );
                builder.add_method(
                    objc2::sel!(becomeFirstResponder),
                    become_first_responder as extern "C-unwind" fn(_, _) -> Bool,
                );
                builder.add_method(
                    objc2::sel!(resignFirstResponder),
                    resign_first_responder as extern "C-unwind" fn(_, _) -> Bool,
                );

                // Keyboard events
                builder.add_method(
                    objc2::sel!(keyDown:),
                    key_down as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(keyUp:),
                    key_up as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(flagsChanged:),
                    flags_changed as extern "C-unwind" fn(_, _, *mut AnyObject),
                );

                // Left mouse
                builder.add_method(
                    objc2::sel!(mouseDown:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(mouseUp:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(mouseMoved:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(mouseDragged:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );

                // Right mouse
                builder.add_method(
                    objc2::sel!(rightMouseDown:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(rightMouseUp:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(rightMouseDragged:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );

                // Other mouse
                builder.add_method(
                    objc2::sel!(otherMouseDown:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(otherMouseUp:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(otherMouseDragged:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );

                // Mouse enter/exit (hover status + pointer event)
                builder.add_method(
                    objc2::sel!(mouseEntered:),
                    mouse_entered as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(mouseExited:),
                    mouse_exited as extern "C-unwind" fn(_, _, *mut AnyObject),
                );

                // Scroll
                builder.add_method(
                    objc2::sel!(scrollWheel:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(magnifyWithEvent:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(rotateWithEvent:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(pressureChangeWithEvent:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(beginGestureWithEvent:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );
                builder.add_method(
                    objc2::sel!(endGestureWithEvent:),
                    handle_input_event as extern "C-unwind" fn(_, _, *mut AnyObject),
                );

                // Drawing — drawRect: drives the on_request_frame contract
                builder.add_method(
                    objc2::sel!(drawRect:),
                    draw_rect as extern "C-unwind" fn(_, _, NSRect),
                );

                // Lifecycle
                builder.add_method(objc2::sel!(dealloc), dealloc as extern "C-unwind" fn(_, _));
                builder.add_method(
                    objc2::sel!(viewDidChangeEffectiveAppearance),
                    view_did_change_effective_appearance as extern "C-unwind" fn(_, _),
                );

                // Properties
                builder.add_method(
                    objc2::sel!(isOpaque),
                    is_opaque as extern "C-unwind" fn(_, _) -> Bool,
                );
                builder.add_method(
                    objc2::sel!(acceptsTouchEvents),
                    accepts_touch_events as extern "C-unwind" fn(_, _) -> Bool,
                );

                // The NSTextInputClient conformance and its callbacks —
                // `interpretKeyEvents:` reaches this view's composition state, and
                // the input method reaches it back, only through these.
                add_text_input_methods(&mut builder);
            }

            builder.register();
        });

        AnyClass::get(c"FLUIContentView").expect("FLUIContentView was registered by the Once block")
    }

    // ============================================================================
    // Helper Functions
    // ============================================================================

    /// Get view context from ivar
    ///
    /// # Safety
    ///
    /// `view` must be a live FLUIContentView whose `context_ptr` ivar is either
    /// null or points to a `ViewContext` owned by that view.
    pub(crate) unsafe fn get_context(view: &AnyObject) -> Option<&ViewContext> {
        // SAFETY: per the function contract the ivar is null or a valid
        // Box<ViewContext> pointer owned by the view; the returned shared
        // reference cannot outlive the view method invocation that holds `view`.
        unsafe {
            let class = AnyClass::get(c"FLUIContentView")?;
            let ivar = class.instance_variable(CONTEXT_IVAR)?;
            let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(view);
            let context_ptr = *slot;
            if context_ptr.is_null() {
                return None;
            }
            Some(&*(context_ptr as *const ViewContext))
        }
    }

    /// The `context_ptr` of `view`, or `None` unless `view` is really one of ours.
    ///
    /// The class check is the whole point of this function. [`get_context`] may
    /// index the `context_ptr` ivar directly only because its callers are
    /// `FLUIContentView` methods — AppKit invokes a method on an object that
    /// declares it. A view handed in from elsewhere carries no such guarantee: an
    /// `NSVisualEffectView` (the window's content view while Liquid Glass is
    /// applied) has no `context_ptr` ivar at all, and reading it would be undefined
    /// rather than returning null. So every read of that ivar
    /// from a view the crate did not itself receive as `self` goes through here,
    /// where the class is checked first.
    ///
    /// # Safety
    ///
    /// `view` must be null or a live `NSView*`.
    unsafe fn content_view_context_ptr(view: *mut AnyObject) -> Option<*mut ViewContext> {
        // SAFETY: per the function contract `view` is null or a live NSView;
        // `isKindOfClass:` is asked before the ivar is touched, and a view that
        // answers yes to it is one `create_content_view` built, whose
        // `context_ptr` is either null or a `ViewContext` it owns.
        unsafe {
            if view.is_null() {
                return None;
            }
            let is_content_view: Bool = msg_send![view, isKindOfClass: get_or_create_view_class()];
            if is_content_view != Bool::YES {
                return None;
            }
            let ivar = get_or_create_view_class().instance_variable(CONTEXT_IVAR)?;
            let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(&*view);
            let context_ptr = *slot;
            if context_ptr.is_null() {
                return None;
            }
            Some(context_ptr.cast::<ViewContext>())
        }
    }

    /// Reach the [`ViewContext`] of a view fetched from somewhere other than a
    /// `FLUIContentView` method, scoped to `body`.
    ///
    /// Answers `None` for anything that is not a view this crate built, so a caller
    /// that obtained its view from anywhere else needs no check of its
    /// own. The context is handed to `body` rather than returned, so the borrow
    /// cannot outlive the call that established the view was alive.
    ///
    /// # Safety
    ///
    /// `view` must be null or a live `NSView*`, and `body` must not hold on to what
    /// it is given past its own return (it may not: the reference is scoped to it).
    pub(crate) unsafe fn with_view_context<R>(
        view: *mut AnyObject,
        body: impl FnOnce(&ViewContext) -> R,
    ) -> Option<R> {
        // SAFETY: `content_view_context_ptr` establishes that the pointer is a live
        // `ViewContext` owned by `view`; the reference is scoped to `body`, so it
        // cannot outlive the call that established the view was alive.
        unsafe { content_view_context_ptr(view).map(|ptr| body(&*ptr)) }
    }

    /// [`with_view_context`], handing the context out mutably.
    ///
    /// # Safety
    ///
    /// Same contract as [`with_view_context`]: `view` must be null or a live
    /// `NSView*`, and `body` must not hold on to what it is given past its own
    /// return.
    pub(super) unsafe fn with_view_context_mut<R>(
        view: *mut AnyObject,
        body: impl FnOnce(&mut ViewContext) -> R,
    ) -> Option<R> {
        // SAFETY: as `with_view_context`, plus: the `&mut` is sound because the
        // pointer is the view's own ivar and AppKit delivers these callbacks
        // serially on the main thread, so no other borrow of this context is live.
        unsafe { content_view_context_ptr(view).map(|ptr| body(&mut *ptr)) }
    }

    /// Dispatch input event to the window's callbacks
    ///
    /// `pub(crate)` for [`crate::text::apple::macos`], whose IME events take this same
    /// throat so they are ordered against pointer and keyboard events.
    pub(crate) fn dispatch_input_event(ctx: &ViewContext, input: flui_platform::PlatformInput) {
        if let Some(callbacks) = ctx.callbacks.upgrade() {
            let _result = callbacks.dispatch_input(input);
        } else {
            tracing::trace!("Input event received after window callbacks were dropped");
        }
    }

    // ============================================================================
    // Public API
    // ============================================================================

    /// Update view scale factor (called when window moves to different display)
    ///
    /// A no-op on any view that is not a `FLUIContentView`.
    pub fn update_view_scale_factor(view: *mut AnyObject, new_scale_factor: f64) {
        // SAFETY: `view` is null or a live NSView* (callers pass the window's
        // FLUI view); `with_view_context_mut` checks the class before touching
        // the ivar and scopes the borrow to the closure, and the mutation is
        // confined to the main thread (AppKit calls).
        let updated = unsafe {
            with_view_context_mut(view, |context| {
                context.scale_factor = new_scale_factor;
            })
            .is_some()
        };
        if updated {
            tracing::debug!("Updated view scale factor to {}", new_scale_factor);
        }
    }

    /// `NSTrackingArea` option bits (cocoa 0.26 does not bind NSTrackingArea).
    ///
    /// Raw values per AppKit's `NSTrackingAreaOptions`.
    mod tracking_area_options {
        pub const MOUSE_ENTERED_AND_EXITED: usize = 0x01;
        pub const MOUSE_MOVED: usize = 0x02;
        pub const ACTIVE_IN_KEY_WINDOW: usize = 0x20;
        pub const IN_VISIBLE_RECT: usize = 0x200;
    }

    /// Enable mouse tracking for mouse moved events
    pub fn enable_mouse_tracking(view: *mut AnyObject) {
        // SAFETY: `view` is a live NSView; NSTrackingArea is looked up via the
        // runtime (the class always exists in AppKit), and `addTrackingArea:`
        // retains the tracking area, so releasing our reference is handled by
        // the view's lifetime.
        unsafe {
            // Get view bounds
            let bounds: NSRect = msg_send![view, bounds];

            // Create tracking area options
            let options: usize = tracking_area_options::MOUSE_MOVED
                | tracking_area_options::ACTIVE_IN_KEY_WINDOW
                | tracking_area_options::MOUSE_ENTERED_AND_EXITED
                | tracking_area_options::IN_VISIBLE_RECT;

            // Create tracking area
            let cls = AnyClass::get(c"NSTrackingArea")
                .expect("BUG: NSTrackingArea is an AppKit class this backend always has");
            let tracking_area: *mut AnyObject = msg_send![cls, alloc];
            let tracking_area: *mut AnyObject = msg_send![tracking_area,
                initWithRect: bounds
                options: options
                owner: view
                userInfo: std::ptr::null_mut::<AnyObject>()
            ];

            // Add to view
            let _: () = msg_send![view, addTrackingArea: tracking_area];

            tracing::debug!("Enabled mouse tracking for view");
        }
    }

    /// Unused-import guard: `NSResponder`/`AnyProtocol`/`NSRectEdge` are named by
    /// the conformance and superclass chain objc2's typed AppKit API relies on.
    #[allow(dead_code)]
    fn _typed_appkit_is_used(_r: Option<&NSResponder>, _p: Option<&AnyProtocol>, _e: NSRectEdge) {}

    /// The input position of the macOS table: the loop answers it.
    impl crate::host::Input<crate::owner::apple::MacOSLoop> for crate::owner::apple::MacOSLoop {
        fn keyboard_layout(_: &Self) -> String {
            // The host does not read AppKit's input source: the layout is unknown.
            String::new()
        }

        fn on_keyboard_layout_change(_: &Self, callback: Box<dyn FnMut() + Send>) {
            // No layout is read, so no change is ever reported.
            drop(callback);
        }
    }

    mod events {
        //! AppKit input translated into the owned pointer vocabulary.
        //!
        //! Each `NSEvent` is copied into a plain [`NativeReading`] and assembled by
        //! the shared [`pointer_assembly`](crate::input::assembly): mouse
        //! chords belong to the receiving view; trackpad magnify and rotate components
        //! share one cumulative session. Two-finger scroll remains scroll, retaining
        //! the native finger/momentum phase and delta precision.

        use std::num::{NonZeroU8, NonZeroU64};
        use std::time::Duration;

        use flui_platform::EventTime;
        use flui_platform::keyboard::{Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey};
        use flui_platform::pointer::{
            PanZoomEvent, PointerButton, PointerEvent, PointerId, PointerInfo, PointerKind,
            PointerPosition, PointerRole, ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision,
            ScrollUnit,
        };
        use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventPhase, NSEventType};
        use objc2_foundation::NSProcessInfo;

        use crate::input::assembly::{
            ClockChoice, ContactRegistry, DecodeError, GestureComponent, GestureDelta,
            GesturePhase, GestureReading, NativeCancel, NativeClock, NativeContact, NativeReading,
            NativeSurface, NativeTime, RawSensors, ReadingAction, ReadingTime, ScrollPhases,
            ScrollReading, decode,
        };
        use crate::input::scroll::{WheelAmount, wheel_step};
        use flui_platform::PlatformInput;

        /// The view's cursor.
        const MOUSE: NativeContact = NativeContact(0);
        /// The view's trackpad gesture session.
        const GESTURE: NativeContact = NativeContact(1);

        /// Owner-local AppKit input state, committed before callbacks run.
        pub(super) struct MacInputState {
            contacts: ContactRegistry,
            scrolling: ScrollPhases,
        }

        impl Default for MacInputState {
            fn default() -> Self {
                // The uptime and the epoch are read back to back: AppKit timestamps
                // and `Instant` both count the same monotonic uptime.
                let uptime = NSProcessInfo::processInfo().systemUptime();
                let clock = NativeClock::new(
                    EventTime::from_nanos(crate::input::event_timestamp_ns()),
                    Duration::try_from_secs_f64(uptime).unwrap_or_default(),
                    None,
                );
                Self {
                    contacts: ContactRegistry::new(clock).with_reserved_contact(MOUSE),
                    scrolling: ScrollPhases::default(),
                }
            }
        }

        impl MacInputState {
            /// # Safety
            /// The pointer must refer to the live NSEvent delivered to this view.
            pub(super) unsafe fn convert(
                &mut self,
                event: *mut std::ffi::c_void,
                view_height: f64,
            ) -> Vec<PlatformInput> {
                if event.is_null() {
                    return Vec::new();
                }
                // SAFETY: the caller supplies the live event for the duration of this call.
                let event = unsafe { &*event.cast::<NSEvent>() };
                self.convert_typed(event, view_height)
            }

            fn convert_typed(&mut self, event: &NSEvent, view_height: f64) -> Vec<PlatformInput> {
                let reading_time =
                    ReadingTime::coarse(NativeTime::UptimeSeconds(event.timestamp()));
                let time = self
                    .contacts
                    .clock()
                    .stamp(reading_time, ClockChoice::Coarse)
                    .unwrap_or_else(|| EventTime::from_nanos(crate::input::event_timestamp_ns()));
                let event_type = event.r#type();
                if event_type == NSEventType::KeyDown || event_type == NSEventType::KeyUp {
                    let key_code = event.keyCode();
                    let code = crate::input::apple::keys::keycode_to_code(key_code);
                    return vec![PlatformInput::Keyboard(
                        KeyEvent::new(
                            if event_type == NSEventType::KeyDown {
                                KeyState::Down
                            } else {
                                KeyState::Up
                            },
                            extract_key(event, key_code),
                            code,
                            time,
                        )
                        .with_location(crate::input::keys::location_for_code(code))
                        .with_modifiers(extract_modifiers(event))
                        .with_repeat(
                            if event_type == NSEventType::KeyDown && event.isARepeat() {
                                KeyRepeat::AutoRepeat
                            } else {
                                KeyRepeat::First
                            },
                        ),
                    )];
                }
                let modifiers = extract_modifiers(event);
                let location = event.locationInWindow();
                let (x, y) = (location.x, view_height - location.y);
                let placed = |reading: NativeReading| {
                    if x.is_finite() && y.is_finite() {
                        reading.at(x, y, 1.0)
                    } else {
                        reading
                    }
                };
                let mouse = |action| {
                    placed(
                        NativeReading::new(MOUSE, PointerKind::Mouse, action, reading_time)
                            .with_primary(true)
                            .with_modifiers(modifiers),
                    )
                };
                let reading = if event_type == NSEventType::MouseEntered {
                    mouse(ReadingAction::Enter)
                } else if event_type == NSEventType::MouseExited {
                    mouse(ReadingAction::Leave)
                } else if event_type == NSEventType::MouseCancelled {
                    mouse(ReadingAction::Cancel(NativeCancel::Platform))
                } else if matches!(
                    event_type,
                    NSEventType::LeftMouseDown
                        | NSEventType::RightMouseDown
                        | NSEventType::OtherMouseDown
                ) {
                    let Some(button) = mouse_button(event.buttonNumber()) else {
                        return Vec::new();
                    };
                    let mut press = mouse(ReadingAction::Press(button));
                    if let Ok(count) = u8::try_from(event.clickCount())
                        && let Some(count) = NonZeroU8::new(count)
                    {
                        press = press.with_click_count(count);
                    }
                    press
                } else if matches!(
                    event_type,
                    NSEventType::LeftMouseUp
                        | NSEventType::RightMouseUp
                        | NSEventType::OtherMouseUp
                ) {
                    let Some(button) = mouse_button(event.buttonNumber()) else {
                        return Vec::new();
                    };
                    mouse(ReadingAction::Release(button))
                } else if matches!(
                    event_type,
                    NSEventType::MouseMoved
                        | NSEventType::LeftMouseDragged
                        | NSEventType::RightMouseDragged
                        | NSEventType::OtherMouseDragged
                        | NSEventType::Pressure
                ) {
                    // AppKit pressure stages have separate normalized curves. Keep the
                    // first-stage reading; summing stages would invent a pressure range.
                    let pressure = (event_type == NSEventType::Pressure && event.stage() == 1)
                        .then(|| f64::from(event.pressure()))
                        .filter(|pressure| (0.0..=1.0).contains(pressure));
                    mouse(ReadingAction::Move).with_sensors(RawSensors {
                        pressure,
                        ..RawSensors::default()
                    })
                } else if event_type == NSEventType::ScrollWheel {
                    return self.scroll(event, time, x, y, modifiers);
                } else if let Some((component, delta)) = gesture_component(event, event_type) {
                    let phase = if event_type == NSEventType::BeginGesture {
                        NSEventPhase::Began
                    } else if event_type == NSEventType::EndGesture {
                        NSEventPhase::Ended
                    } else {
                        event.phase()
                    };
                    return self
                        .gesture(component, phase, delta, (x, y), reading_time, modifiers)
                        .into_iter()
                        .map(|event| PlatformInput::Pointer(PointerEvent::PanZoom(event)))
                        .collect();
                } else {
                    return Vec::new();
                };
                decode(&mut self.contacts, &reading)
                    .unwrap_or_else(DecodeError::into_inputs)
                    .into_vec()
            }

            fn scroll(
                &mut self,
                event: &NSEvent,
                time: EventTime,
                x: f64,
                y: f64,
                modifiers: Modifiers,
            ) -> Vec<PlatformInput> {
                let Ok(position) =
                    PointerPosition::try_new(flui_foundation::geometry::Point::new(x, y))
                else {
                    return Vec::new();
                };
                let pointer = self
                    .contacts
                    .pointer(NativeSurface::default(), None, MOUSE)
                    .unwrap_or_else(mouse_info);
                let precise = event.hasPreciseScrollingDeltas();
                let unit = if precise {
                    ScrollUnit::Pixels
                } else {
                    ScrollUnit::Lines
                };
                // AppKit's deltas move the content; a step the one wheel rule
                // refuses is zero.
                let delta = wheel_step(
                    unit,
                    WheelAmount::content(event.scrollingDeltaX()),
                    WheelAmount::content(event.scrollingDeltaY()),
                )
                .unwrap_or_else(|_| ScrollDelta::zero(unit));
                let mut scroll = ScrollEvent::new(pointer, time, position, delta)
                    .with_modifiers(modifiers)
                    .with_precision(if precise {
                        ScrollPrecision::Precise
                    } else {
                        ScrollPrecision::Notched
                    });
                if let Some(phase) = scroll_phase(
                    &mut self.scrolling,
                    event.phase(),
                    event.momentumPhase(),
                    precise,
                ) {
                    scroll = scroll.with_phase(phase);
                }
                vec![PlatformInput::Pointer(PointerEvent::Scroll(scroll))]
            }

            /// One trackpad gesture component at view point `(x, y)`; a non-finite
            /// point keeps the gesture's last one.
            fn gesture(
                &mut self,
                component: GestureComponent,
                phase: NSEventPhase,
                delta: GestureDelta,
                (x, y): (f64, f64),
                time: ReadingTime,
                modifiers: Modifiers,
            ) -> Vec<PanZoomEvent> {
                let phase = if phase.contains(NSEventPhase::Cancelled) {
                    GesturePhase::Cancelled
                } else if phase.contains(NSEventPhase::Began) {
                    GesturePhase::Began
                } else if phase.contains(NSEventPhase::Ended) {
                    GesturePhase::Ended
                } else {
                    GesturePhase::Changed
                };
                let mut reading = NativeReading::new(
                    GESTURE,
                    PointerKind::Trackpad,
                    ReadingAction::Gesture(GestureReading {
                        component,
                        phase,
                        delta,
                    }),
                    time,
                )
                .with_primary(true)
                .with_modifiers(modifiers);
                if x.is_finite() && y.is_finite() {
                    reading = reading.at(x, y, 1.0);
                }
                decode(&mut self.contacts, &reading)
                    .unwrap_or_else(DecodeError::into_inputs)
                    .into_iter()
                    .filter_map(|input| match input {
                        PlatformInput::Pointer(PointerEvent::PanZoom(event)) => Some(event),
                        _ => None,
                    })
                    .collect()
            }
        }

        /// The gesture component an AppKit gesture event reports, with its change.
        fn gesture_component(
            event: &NSEvent,
            event_type: NSEventType,
        ) -> Option<(GestureComponent, GestureDelta)> {
            if event_type == NSEventType::BeginGesture || event_type == NSEventType::EndGesture {
                Some((GestureComponent::Session, GestureDelta::None))
            } else if event_type == NSEventType::Magnify {
                Some((
                    GestureComponent::Scale,
                    GestureDelta::Scale(1.0 + event.magnification()),
                ))
            } else if event_type == NSEventType::Rotate {
                // AppKit reports counterclockwise degrees; owned rotation is clockwise radians.
                Some((
                    GestureComponent::Rotation,
                    GestureDelta::Rotation(-f64::from(event.rotation()).to_radians()),
                ))
            } else {
                None
            }
        }

        fn mouse_info() -> PointerInfo {
            PointerInfo::new(PointerId::new(NonZeroU64::MIN), PointerKind::Mouse)
                .with_role(PointerRole::Primary)
        }

        fn mouse_button(number: isize) -> Option<PointerButton> {
            // Native numbers are zero-based; FLUI reserves six for the pen eraser tool.
            let number = match number {
                0..=4 => number + 1,
                5..=30 => number + 2,
                _ => return None,
            };
            PointerButton::try_from(u8::try_from(number).ok()?).ok()
        }

        /// An AppKit phase as the shared grammar's plain phase; `None` for
        /// `MayBegin` and no phase.
        fn native_phase(phase: NSEventPhase) -> Option<GesturePhase> {
            if phase.contains(NSEventPhase::Cancelled) {
                Some(GesturePhase::Cancelled)
            } else if phase.contains(NSEventPhase::Ended) {
                Some(GesturePhase::Ended)
            } else if phase.contains(NSEventPhase::Began) {
                Some(GesturePhase::Began)
            } else if phase.contains(NSEventPhase::Changed)
                || phase.contains(NSEventPhase::Stationary)
            {
                Some(GesturePhase::Changed)
            } else {
                None
            }
        }

        /// The owned phase of an AppKit scroll through the shared grammar
        /// (ADR-0217 §4): a two-finger pan is a phased scroll, momentum is
        /// `Momentum*`.
        fn scroll_phase(
            phases: &mut ScrollPhases,
            phase: NSEventPhase,
            momentum: NSEventPhase,
            precise: bool,
        ) -> Option<ScrollPhase> {
            let reading = ScrollReading {
                fingers: native_phase(phase),
                momentum: native_phase(momentum),
                precise,
            };
            phases.phase(NativeSurface::default(), None, reading)
        }

        fn extract_key(event: &NSEvent, key_code: u16) -> Key {
            // Check for special keys via key code first.
            if let Some(special_key) = crate::input::apple::keys::keycode_to_key(key_code) {
                return special_key;
            }

            // `characters` is `None` only when the event has no character mapping;
            // objc2 models the NSString as retained, so there is no null to test.
            let Some(chars) = event.characters() else {
                return Key::Named(NamedKey::Unidentified);
            };

            let chars_str = chars.to_string();
            if chars_str.is_empty() {
                return Key::Named(NamedKey::Unidentified);
            }
            Key::character(chars_str)
        }

        /// Extract modifiers from NSEvent
        ///
        /// # Safety
        ///
        /// `ns_event` must be a valid, live `NSEvent*`.
        fn extract_modifiers(event: &NSEvent) -> Modifiers {
            let flags = event.modifierFlags();

            let mut modifiers = Modifiers::NONE;
            if flags.contains(NSEventModifierFlags::Shift) {
                modifiers |= Modifiers::SHIFT;
            }
            if flags.contains(NSEventModifierFlags::Control) {
                modifiers |= Modifiers::CONTROL;
            }
            if flags.contains(NSEventModifierFlags::Option) {
                modifiers |= Modifiers::ALT;
            }
            if flags.contains(NSEventModifierFlags::Command) {
                modifiers |= Modifiers::META; // Command = Meta
            }
            modifiers
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn native_scroll_phases_preserve_momentum_and_unphased_wheels() {
                for (phase, momentum, expected) in [
                    (NSEventPhase::None, NSEventPhase::None, None),
                    (NSEventPhase::MayBegin, NSEventPhase::None, None),
                    (
                        NSEventPhase::Began,
                        NSEventPhase::None,
                        Some(ScrollPhase::Began),
                    ),
                    (
                        NSEventPhase::Changed,
                        NSEventPhase::None,
                        Some(ScrollPhase::Changed),
                    ),
                    (
                        NSEventPhase::Ended,
                        NSEventPhase::None,
                        Some(ScrollPhase::Ended),
                    ),
                    (
                        NSEventPhase::Cancelled,
                        NSEventPhase::None,
                        Some(ScrollPhase::Cancelled),
                    ),
                    (
                        NSEventPhase::None,
                        NSEventPhase::Began,
                        Some(ScrollPhase::MomentumBegan),
                    ),
                    (
                        NSEventPhase::None,
                        NSEventPhase::Changed,
                        Some(ScrollPhase::MomentumChanged),
                    ),
                    (
                        NSEventPhase::Ended,
                        NSEventPhase::Ended,
                        Some(ScrollPhase::MomentumEnded),
                    ),
                ] {
                    assert_eq!(
                        scroll_phase(&mut ScrollPhases::default(), phase, momentum, true),
                        expected
                    );
                }
            }

            #[test]
            fn native_pinch_and_rotation_share_one_cumulative_gesture() {
                use GestureComponent::{Rotation, Scale, Session};
                use flui_platform::pointer::PanZoomPhase;
                let mut state = MacInputState::default();
                let at = (10.25, 20.5);
                let time = ReadingTime::coarse(NativeTime::UptimeSeconds(1.0));
                let modifiers = Modifiers::NONE;
                let mut gesture = |component, phase, delta| {
                    state.gesture(component, phase, delta, at, time, modifiers)
                };
                assert_eq!(
                    gesture(Scale, NSEventPhase::Changed, GestureDelta::Scale(1.1)),
                    []
                );
                let first = gesture(Scale, NSEventPhase::Began, GestureDelta::Scale(1.1));
                assert_eq!(first.len(), 2);
                assert_eq!(first[0].phase, PanZoomPhase::Start);
                assert_eq!(
                    first[0].position.get(),
                    flui_foundation::geometry::Point::new(10.25, 20.5)
                );
                let id = first[0].pointer().id;
                let joined = gesture(
                    Rotation,
                    NSEventPhase::Began,
                    GestureDelta::Rotation(-std::f64::consts::FRAC_PI_6),
                );
                assert_eq!(joined.len(), 1);
                let PanZoomPhase::Update(transform) = joined[0].phase else {
                    panic!("update")
                };
                assert_eq!(joined[0].pointer().id, id);
                assert_eq!(transform.scale(), 1.1);
                assert_eq!(transform.rotation(), -std::f64::consts::FRAC_PI_6);
                let ended_component = gesture(Scale, NSEventPhase::Ended, GestureDelta::Scale(1.2));
                assert_eq!(ended_component.len(), 1);
                let last = gesture(
                    Rotation,
                    NSEventPhase::Ended,
                    GestureDelta::Rotation(-std::f64::consts::FRAC_PI_6),
                );
                assert_eq!(last.len(), 2);
                let PanZoomPhase::Update(transform) = last[0].phase else {
                    panic!("update")
                };
                assert!((transform.scale() - 1.32).abs() < 1e-12);
                assert_eq!(transform.rotation(), -std::f64::consts::FRAC_PI_3);
                assert_eq!(last[1].phase, PanZoomPhase::End);
                assert_eq!(last[1].pointer().id, id);
                let next = gesture(Scale, NSEventPhase::Began, GestureDelta::Scale(1.0));
                assert_ne!(next[0].pointer().id, id);
                let cancelled = gesture(
                    Scale,
                    NSEventPhase::Cancelled,
                    GestureDelta::Scale(f64::NAN),
                );
                assert_eq!(cancelled.len(), 1);
                assert_eq!(cancelled[0].phase, PanZoomPhase::Cancelled);
                assert_eq!(
                    gesture(Scale, NSEventPhase::Changed, GestureDelta::Scale(1.1)),
                    []
                );
                let outer = gesture(Session, NSEventPhase::Began, GestureDelta::None);
                assert_eq!(outer.len(), 1);
                let outer_id = outer[0].pointer().id;
                gesture(Scale, NSEventPhase::Began, GestureDelta::Scale(1.1));
                assert_eq!(
                    gesture(Scale, NSEventPhase::Ended, GestureDelta::Scale(1.0)),
                    []
                );
                let rotation = gesture(Rotation, NSEventPhase::Began, GestureDelta::Rotation(0.1));
                assert_eq!(rotation.len(), 1);
                assert_eq!(rotation[0].pointer().id, outer_id);
                gesture(Rotation, NSEventPhase::Ended, GestureDelta::None);
                let end = gesture(Session, NSEventPhase::Ended, GestureDelta::None);
                assert_eq!(end.len(), 1);
                assert_eq!(end[0].phase, PanZoomPhase::End);
                assert_eq!(end[0].pointer().id, outer_id);
            }
        }
    }
}

#[cfg(target_os = "ios")]
pub(crate) mod ios {
    //! UIKit touch and Pencil readings in the owned input vocabulary.
    //!
    //! Each `UITouch` is copied into a plain [`NativeReading`] and assembled by the
    //! shared [`pointer_assembly`](crate::input::assembly); touch identity
    //! and primary admission belong to the receiving view. UIKit reports force
    //! only for Pencil or a force-capable trait environment; unsupported force
    //! remains absent. Native timestamps keep their uptime on the process
    //! timeline.

    // A gesture recognizer's state is read through an Objective-C message,
    // which has no safe wrapper.
    #![expect(unsafe_code)]

    use std::time::Duration;

    use flui_platform::EventTime;
    use flui_platform::pointer::{PenOrientation, PenTool, PointerButton, PointerKind};
    use objc2::{msg_send, sel};
    use objc2_foundation::NSObjectProtocol;
    use objc2_foundation::NSProcessInfo;
    use objc2_ui_kit::{
        UIForceTouchCapability, UIGestureRecognizerState, UIHoverGestureRecognizer, UITouch,
        UITouchType, UITraitEnvironment, UIView,
    };

    use crate::input::assembly::{
        ContactRegistry, DecodeError, NativeCancel, NativeClock, NativeContact, NativeReading,
        NativeTime, PenAngles, RawSensors, ReadingAction, ReadingTime, decode,
    };
    use flui_platform::PlatformInput;

    /// Which native callback delivered a batch.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum TouchPhase {
        Down,
        Move,
        Up,
        Cancel,
    }

    /// The primary Pencil: its hover and its contact are one contact, which keeps
    /// the reserved identity one.
    const PENCIL: NativeContact = NativeContact(0);
    /// The indirect pointer (a trackpad or mouse cursor): one contact for every
    /// `UITouch` UIKit makes for it.
    const CURSOR: NativeContact = NativeContact(1);

    /// Contacts admitted by one UIKit view; no address arithmetic mints identities.
    pub(crate) struct TouchInputState {
        contacts: ContactRegistry,
        /// The touch the primary Pencil's contact is, while its tip is down.
        pencil_touch: Option<usize>,
    }

    impl Default for TouchInputState {
        fn default() -> Self {
            // The uptime and the epoch are read back to back: UIKit timestamps and
            // `Instant` both count the same monotonic uptime.
            let uptime = NSProcessInfo::processInfo().systemUptime();
            let clock = NativeClock::new(
                EventTime::from_nanos(crate::input::event_timestamp_ns()),
                Duration::try_from_secs_f64(uptime).unwrap_or_default(),
                None,
            );
            Self {
                contacts: ContactRegistry::new(clock).with_reserved_contact(PENCIL),
                pencil_touch: None,
            }
        }
    }

    impl TouchInputState {
        /// The recognizer is restricted to Stylus at registration; a mouse cannot
        /// acquire Pencil sensors through this path.
        pub(crate) fn hover(
            &mut self,
            recognizer: &UIHoverGestureRecognizer,
            view: &UIView,
        ) -> Vec<PlatformInput> {
            if self.pencil_touch.is_some() {
                // Contact samples own this Pencil while its tip is down.
                return Vec::new();
            }
            // SAFETY: UIKit invokes this target with a live gesture recognizer;
            // `state` is its documented enum-valued getter (omitted by the bindings).
            let state: UIGestureRecognizerState = unsafe { msg_send![recognizer, state] };
            let time =
                ReadingTime::epoch(EventTime::from_nanos(crate::input::event_timestamp_ns()));
            let pencil = |action| {
                NativeReading::new(
                    PENCIL,
                    PointerKind::Pen { tool: PenTool::Tip },
                    action,
                    time,
                )
                .with_primary(true)
            };
            if state == UIGestureRecognizerState::Ended
                || state == UIGestureRecognizerState::Cancelled
                || state == UIGestureRecognizerState::Failed
            {
                return self.assemble(&pencil(ReadingAction::Leave));
            }
            if state != UIGestureRecognizerState::Began
                && state != UIGestureRecognizerState::Changed
            {
                return Vec::new();
            }
            let location = recognizer.locationInView(Some(view));
            let mut sensors = RawSensors::default();
            // Pencil hover angles were added after UIHoverGestureRecognizer itself.
            if recognizer.respondsToSelector(sel!(altitudeAngle))
                && recognizer.respondsToSelector(sel!(azimuthAngleInView:))
            {
                sensors.angles = pen_angles(
                    recognizer.altitudeAngle(),
                    recognizer.azimuthAngleInView(Some(view)),
                );
            }
            let moved = pencil(ReadingAction::Move)
                .at(location.x, location.y, 1.0)
                .with_sensors(sensors);
            let mut events = Vec::new();
            if state == UIGestureRecognizerState::Began {
                events.extend(self.assemble(&NativeReading {
                    action: ReadingAction::Enter,
                    ..moved
                }));
            }
            events.extend(self.assemble(&moved));
            events
        }

        pub(crate) fn convert(&mut self, touch: &UITouch, phase: TouchPhase) -> Vec<PlatformInput> {
            let address = std::ptr::from_ref(touch) as usize;
            let kind = pointer_kind(touch);
            let contact = if kind == PointerKind::Mouse {
                CURSOR
            } else if self.pencil_touch == Some(address)
                || (phase == TouchPhase::Down
                    && matches!(kind, PointerKind::Pen { .. })
                    && self.pencil_touch.is_none())
            {
                self.pencil_touch = Some(address);
                PENCIL
            } else {
                NativeContact(address as u64)
            };
            if contact == PENCIL && matches!(phase, TouchPhase::Up | TouchPhase::Cancel) {
                self.pencil_touch = None;
            }
            let action = match phase {
                TouchPhase::Down => ReadingAction::Press(PointerButton::PRIMARY),
                TouchPhase::Move => ReadingAction::Move,
                TouchPhase::Up => ReadingAction::Release(PointerButton::PRIMARY),
                TouchPhase::Cancel => ReadingAction::Cancel(NativeCancel::Platform),
            };
            let view = touch.view();
            let location = touch.locationInView(view.as_deref());
            let is_pen = matches!(kind, PointerKind::Pen { .. });
            let has_force = is_pen
                || view.as_ref().is_some_and(|view| {
                    view.traitCollection().forceTouchCapability()
                        == UIForceTouchCapability::Available
                });
            let mut sensors = touch_sensors(
                has_force,
                touch.force(),
                touch.maximumPossibleForce(),
                touch.majorRadius(),
            );
            if is_pen {
                sensors.angles = pen_angles(
                    touch.altitudeAngle(),
                    touch.azimuthAngleInView(view.as_deref()),
                );
            }
            let reading = NativeReading::new(
                contact,
                kind,
                action,
                ReadingTime::coarse(NativeTime::UptimeSeconds(touch.timestamp())),
            )
            .at(location.x, location.y, 1.0)
            .with_sensors(sensors);
            let reading = if contact == PENCIL || contact == CURSOR {
                reading.with_primary(true)
            } else {
                reading
            };
            self.assemble(&reading)
        }

        fn assemble(&mut self, reading: &NativeReading) -> Vec<PlatformInput> {
            decode(&mut self.contacts, reading)
                .unwrap_or_else(DecodeError::into_inputs)
                .into_vec()
        }
    }

    fn pointer_kind(touch: &UITouch) -> PointerKind {
        let kind = touch.r#type();
        if kind == UITouchType::Stylus {
            PointerKind::Pen { tool: PenTool::Tip }
        } else if kind == UITouchType::IndirectPointer {
            PointerKind::Mouse
        } else {
            PointerKind::Touch
        }
    }

    /// Pencil angles, kept only when UIKit reports a valid pair.
    fn pen_angles(altitude: f64, azimuth: f64) -> Option<PenAngles> {
        PenOrientation::try_new(altitude, azimuth)
            .is_ok()
            .then_some(PenAngles::Spherical {
                altitude: Some(altitude),
                azimuth: Some(azimuth),
            })
    }

    /// A touch's force, normalized by its maximum when the environment reports
    /// force, and its contact diameter in points when UIKit measures a radius.
    fn touch_sensors(has_force: bool, force: f64, maximum_force: f64, radius: f64) -> RawSensors {
        let normalized_force = force / maximum_force;
        RawSensors {
            pressure: (has_force
                && maximum_force.is_finite()
                && maximum_force > 0.0
                && (0.0..=1.0).contains(&normalized_force))
            .then_some(normalized_force),
            contact_size: (radius > 0.0 && radius.is_finite())
                .then_some((radius * 2.0, radius * 2.0)),
            ..RawSensors::default()
        }
    }

    /// The input position of the iOS table: the loop answers it.
    impl crate::host::Input<crate::owner::apple::IOSLoop> for crate::owner::apple::IOSLoop {
        fn keyboard_layout(_: &Self) -> String {
            // The host does not read the active keyboard: the layout is unknown.
            String::new()
        }

        fn on_keyboard_layout_change(_: &Self, callback: Box<dyn FnMut() + Send>) {
            // No layout is read, so no change is ever reported.
            drop(callback);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use flui_platform::pointer::{PointerEvent, Pressure};

        /// The Down a Pencil-less touch with `sensors` assembles into.
        fn down(sensors: RawSensors) -> flui_platform::pointer::PointerSample {
            let mut contacts = ContactRegistry::new(NativeClock::epoch_only());
            let reading = NativeReading::new(
                NativeContact(9),
                PointerKind::Touch,
                ReadingAction::Press(PointerButton::PRIMARY),
                ReadingTime::epoch(EventTime::from_nanos(10)),
            )
            .at(1.25, 2.5, 1.0)
            .with_sensors(sensors);
            decode(&mut contacts, &reading)
                .expect("valid touch")
                .iter()
                .find_map(|input| match input.as_pointer() {
                    Some(PointerEvent::Down(press)) => Some(press.sample),
                    _ => None,
                })
                .expect("down")
        }

        #[test]
        fn native_touch_readings_keep_sensor_presence_and_pen_angles() {
            for (has_force, force, maximum, expected) in [
                (false, 1.0, 1.0, None),
                (true, 0.0, 4.0, Some(0.0)),
                (true, 2.0, 4.0, Some(0.5)),
                (true, 2.0, 0.0, None),
                (true, f64::INFINITY, 4.0, None),
            ] {
                let sample = down(touch_sensors(has_force, force, maximum, 3.25));
                assert_eq!(sample.pressure.map(Pressure::get), expected);
                let extent = sample.contact_size.expect("reported radius");
                assert_eq!((extent.get().width, extent.get().height), (6.5, 6.5));
                assert_eq!(sample.orientation, None);
                assert_eq!(sample.tangential_pressure, None);
                assert_eq!(sample.twist, None);
            }
            let pen = down(RawSensors {
                angles: pen_angles(std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2),
                ..touch_sensors(true, 1.0, 4.0, 0.0)
            });
            let orientation = pen.orientation.expect("reported Pencil angles");
            assert_eq!(orientation.altitude(), Some(std::f64::consts::FRAC_PI_4));
            assert_eq!(orientation.azimuth(), Some(std::f64::consts::FRAC_PI_2));
            assert_eq!(pen.contact_size, None);
            assert_eq!(pen_angles(f64::NAN, 0.0), None);
        }
    }
}
