//! Tests for the winit backend's keyboard-conversion delegation to
//! `ui-events-winit`.
//!
//! Split out of `events.rs`'s inline test modules once that file passed
//! 1600 lines with production code ending well before halfway (issue
//! #1092), mirroring how `platform.rs`'s real-event-loop family moved to
//! `platform/real_loop_tests.rs` (issue #923): one cohesive family, nothing
//! else in the crate uses it.
//!
//! Declared with `#[path]` from `events.rs` as a sibling of its inline
//! test modules, so these tests read as
//! `platforms::winit::events::keyboard_tests::*`. Both modules below use
//! only `pub` items from `ui_events`, `ui_events_winit`, `winit`, and
//! `crate::shared`, so nothing here needs `use super::*`.

#[cfg(test)]
mod keyboard_conversion_tests {
    //! Pins the winit backend's delegation to `ui-events-winit` (see
    //! `events::keyboard_event`'s doc) rather than exercising
    //! `keyboard_event` itself: `winit::event::KeyEvent` has a
    //! `pub(crate) platform_specific` field
    //! (`winit-0.30.13/src/event.rs:133`), so nothing outside winit can
    //! construct one. `keyboard_event` now contains no conversion logic of
    //! its own either way — it is one call into
    //! `from_winit_keyboard_event`, which itself calls the
    //! `ui_events_winit::keyboard::from_winit_*` functions tested here
    //! directly. See `every_winit_keycode_maps_to_a_canonical_code`'s doc
    //! for exactly what that leaves unpinned.
    use std::collections::BTreeSet;

    use ui_events::keyboard::{Code, Key, Location, NamedKey};
    use ui_events_winit::keyboard::{from_winit_code, from_winit_key, from_winit_location};
    use winit::keyboard::{
        Key as WinitKey, KeyCode, KeyLocation, NamedKey as WinitNamedKey, PhysicalKey,
    };

    /// Every `winit::keyboard::KeyCode` variant, winit 0.30.13
    /// (`winit-0.30.13/src/keyboard.rs`, the `KeyCode` enum), in the exact
    /// order it is declared there. The length assertion in the test below
    /// is what catches a silently truncated copy of this list; the
    /// `BTreeSet` assertion alongside it catches a duplicated entry
    /// standing in for a dropped one (same length, wrong coverage).
    const ALL_WINIT_KEYCODES: &[KeyCode] = &[
        KeyCode::Backquote,
        KeyCode::Backslash,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
        KeyCode::Comma,
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Equal,
        KeyCode::IntlBackslash,
        KeyCode::IntlRo,
        KeyCode::IntlYen,
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
        KeyCode::Minus,
        KeyCode::Period,
        KeyCode::Quote,
        KeyCode::Semicolon,
        KeyCode::Slash,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::Backspace,
        KeyCode::CapsLock,
        KeyCode::ContextMenu,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::Enter,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::Space,
        KeyCode::Tab,
        KeyCode::Convert,
        KeyCode::KanaMode,
        KeyCode::Lang1,
        KeyCode::Lang2,
        KeyCode::Lang3,
        KeyCode::Lang4,
        KeyCode::Lang5,
        KeyCode::NonConvert,
        KeyCode::Delete,
        KeyCode::End,
        KeyCode::Help,
        KeyCode::Home,
        KeyCode::Insert,
        KeyCode::PageDown,
        KeyCode::PageUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::NumLock,
        KeyCode::Numpad0,
        KeyCode::Numpad1,
        KeyCode::Numpad2,
        KeyCode::Numpad3,
        KeyCode::Numpad4,
        KeyCode::Numpad5,
        KeyCode::Numpad6,
        KeyCode::Numpad7,
        KeyCode::Numpad8,
        KeyCode::Numpad9,
        KeyCode::NumpadAdd,
        KeyCode::NumpadBackspace,
        KeyCode::NumpadClear,
        KeyCode::NumpadClearEntry,
        KeyCode::NumpadComma,
        KeyCode::NumpadDecimal,
        KeyCode::NumpadDivide,
        KeyCode::NumpadEnter,
        KeyCode::NumpadEqual,
        KeyCode::NumpadHash,
        KeyCode::NumpadMemoryAdd,
        KeyCode::NumpadMemoryClear,
        KeyCode::NumpadMemoryRecall,
        KeyCode::NumpadMemoryStore,
        KeyCode::NumpadMemorySubtract,
        KeyCode::NumpadMultiply,
        KeyCode::NumpadParenLeft,
        KeyCode::NumpadParenRight,
        KeyCode::NumpadStar,
        KeyCode::NumpadSubtract,
        KeyCode::Escape,
        KeyCode::Fn,
        KeyCode::FnLock,
        KeyCode::PrintScreen,
        KeyCode::ScrollLock,
        KeyCode::Pause,
        KeyCode::BrowserBack,
        KeyCode::BrowserFavorites,
        KeyCode::BrowserForward,
        KeyCode::BrowserHome,
        KeyCode::BrowserRefresh,
        KeyCode::BrowserSearch,
        KeyCode::BrowserStop,
        KeyCode::Eject,
        KeyCode::LaunchApp1,
        KeyCode::LaunchApp2,
        KeyCode::LaunchMail,
        KeyCode::MediaPlayPause,
        KeyCode::MediaSelect,
        KeyCode::MediaStop,
        KeyCode::MediaTrackNext,
        KeyCode::MediaTrackPrevious,
        KeyCode::Power,
        KeyCode::Sleep,
        KeyCode::AudioVolumeDown,
        KeyCode::AudioVolumeMute,
        KeyCode::AudioVolumeUp,
        KeyCode::WakeUp,
        KeyCode::Meta,
        KeyCode::Hyper,
        KeyCode::Turbo,
        KeyCode::Abort,
        KeyCode::Resume,
        KeyCode::Suspend,
        KeyCode::Again,
        KeyCode::Copy,
        KeyCode::Cut,
        KeyCode::Find,
        KeyCode::Open,
        KeyCode::Paste,
        KeyCode::Props,
        KeyCode::Select,
        KeyCode::Undo,
        KeyCode::Hiragana,
        KeyCode::Katakana,
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
        KeyCode::F13,
        KeyCode::F14,
        KeyCode::F15,
        KeyCode::F16,
        KeyCode::F17,
        KeyCode::F18,
        KeyCode::F19,
        KeyCode::F20,
        KeyCode::F21,
        KeyCode::F22,
        KeyCode::F23,
        KeyCode::F24,
        KeyCode::F25,
        KeyCode::F26,
        KeyCode::F27,
        KeyCode::F28,
        KeyCode::F29,
        KeyCode::F30,
        KeyCode::F31,
        KeyCode::F32,
        KeyCode::F33,
        KeyCode::F34,
        KeyCode::F35,
    ];

    /// Every winit 0.30.13 `KeyCode` has a non-`Unidentified` canonical
    /// `Code` through `ui-events-winit`'s `from_winit_code` — the property
    /// this whole issue is about. Before this change,
    /// `convert_physical_key`'s own hand table covered 71 of these 194
    /// variants (letters, digits, a small nav/modifier subset, F1..F12) and
    /// fell through to `Unidentified` for the other 123 — numpad, Intl*,
    /// F13+, media/browser/system keys among them.
    ///
    /// What this test does NOT pin: a regression introduced *inside*
    /// `keyboard_event` itself (a wrong function called, a field dropped
    /// from the event it builds) would stay invisible here, because
    /// `winit::event::KeyEvent` cannot be constructed outside winit (its
    /// `platform_specific` field is `pub(crate)`,
    /// `winit-0.30.13/src/event.rs:133`) and so `keyboard_event` can never
    /// be called directly from a test. What this test pins is the
    /// completeness of the `ui-events-winit` version `Cargo.lock` resolves
    /// to — the dependency `keyboard_event` delegates to unconditionally.
    #[test]
    fn every_winit_keycode_maps_to_a_canonical_code() {
        assert_eq!(ALL_WINIT_KEYCODES.len(), 194);
        assert_eq!(
            ALL_WINIT_KEYCODES.iter().collect::<BTreeSet<_>>().len(),
            194,
            "a duplicated entry would keep the length check above green while \
             silently standing in for a variant this list dropped"
        );
        let unidentified: Vec<KeyCode> = ALL_WINIT_KEYCODES
            .iter()
            .copied()
            .filter(|&k| from_winit_code(PhysicalKey::Code(k)) == Code::Unidentified)
            .collect();
        assert!(
            unidentified.is_empty(),
            "{} winit KeyCode variant(s) produced Code::Unidentified: {unidentified:?}",
            unidentified.len()
        );
    }

    /// A `PhysicalKey::Unidentified` stays `Code::Unidentified` — the one
    /// case that value is reserved for (see `events::keyboard_event`'s
    /// doc): winit itself could not name the physical key, distinct from
    /// every case above where winit named one and the table failed to
    /// preserve it.
    #[test]
    fn physical_key_unidentified_stays_unidentified() {
        assert_eq!(
            from_winit_code(PhysicalKey::Unidentified(
                winit::keyboard::NativeKeyCode::Unidentified
            )),
            Code::Unidentified
        );
    }

    /// Spot pairs named in issue #1092's acceptance criteria, run through
    /// the exact function `keyboard_event` calls.
    #[test]
    fn spot_pairs_match_the_issues_acceptance_criteria() {
        let cases = [
            (KeyCode::NumpadEnter, Code::NumpadEnter),
            (KeyCode::IntlYen, Code::IntlYen),
            (KeyCode::AudioVolumeUp, Code::AudioVolumeUp),
            (KeyCode::F24, Code::F24),
            (KeyCode::ContextMenu, Code::ContextMenu),
            (KeyCode::Comma, Code::Comma),
            (KeyCode::CapsLock, Code::CapsLock),
        ];
        for (winit_code, expected) in cases {
            assert_eq!(
                from_winit_code(PhysicalKey::Code(winit_code)),
                expected,
                "{winit_code:?}"
            );
        }
    }

    /// `KeyLocation`'s four variants map name-for-name onto
    /// `ui_events::keyboard::Location`. Unlike the pre-fix code, location
    /// is no longer *derived* from the physical `Code` at all — it comes
    /// straight from winit's own `KeyEvent.location` field (see
    /// `events::keyboard_event`), so there is no "does `Code::X` imply
    /// `Location::Numpad`" property left to test here; that coupling is
    /// exactly what this fix removed.
    #[test]
    fn every_key_location_maps_to_its_canonical_location() {
        let cases = [
            (KeyLocation::Standard, Location::Standard),
            (KeyLocation::Left, Location::Left),
            (KeyLocation::Right, Location::Right),
            (KeyLocation::Numpad, Location::Numpad),
        ];
        for (winit_location, expected) in cases {
            assert_eq!(
                from_winit_location(winit_location),
                expected,
                "{winit_location:?}"
            );
        }
    }

    /// A handful of `NamedKey` pairs, plus the `Character` pass-through
    /// (including a non-ASCII character, since this crosses a
    /// `SmolStr`-to-`String` conversion). Exhaustive `NamedKey` coverage is
    /// `ui-events-winit`'s test suite's job, not ours; these pin the shape
    /// this crate relies on.
    #[test]
    fn logical_key_spot_pairs() {
        assert_eq!(
            from_winit_key(WinitKey::Named(WinitNamedKey::Enter)),
            Key::Named(NamedKey::Enter)
        );
        assert_eq!(
            from_winit_key(WinitKey::Named(WinitNamedKey::ContextMenu)),
            Key::Named(NamedKey::ContextMenu)
        );
        // winit intercepts Space as a named key; the canonical vocabulary
        // reports it as the character it types, matching every other
        // printable key.
        assert_eq!(
            from_winit_key(WinitKey::Named(WinitNamedKey::Space)),
            Key::Character(" ".to_string())
        );
        assert_eq!(
            from_winit_key(WinitKey::Character("e".into())),
            Key::Character("e".to_string())
        );
        assert_eq!(
            from_winit_key(WinitKey::Character("\u{e9}".into())),
            Key::Character("\u{e9}".to_string())
        );
    }
}

#[cfg(test)]
mod cross_backend_physical_key_agreement {
    //! Cross-checks a curated set of physical keys that the winit, Win32,
    //! and AppKit conversions all claim to name explicitly (see
    //! `crate::shared::keys::scancode_to_code`'s and
    //! `crate::shared::keys_macos::keycode_to_code`'s own module docs), so a
    //! future edit to any one of the three tables that silently retargets a
    //! shared physical key is caught here instead of only shipping as a
    //! per-backend behavior change. Win32/AppKit scancodes and keycodes are
    //! transcribed directly from those modules' own match arms — read there
    //! before adding a row, since only the values THOSE tables actually use
    //! are meaningful cross-checks.
    //!
    //! Coverage is representative, not exhaustive: only keys where a
    //! meaning-preserving correspondence between all three native id spaces
    //! (a winit `KeyCode` variant, a Win32 scancode, an AppKit keycode) is
    //! unambiguous. Two families are deliberately excluded with a reason
    //! rather than silently omitted:
    //! - Win32's `KanaMode` (scancode `0x70`) has no AppKit counterpart —
    //!   the AppKit table maps its Kana key straight to `Lang1` (matching
    //!   Win32's *separate* `Lang1` scancode `0x72`), per the W3C code
    //!   registry's own platform-conditional reuse of the `Lang1`/`Lang2`
    //!   slots (Japanese Mac Eisu/Kana vs. Korean Hangul/Hanja) — not a
    //!   defect, so `KanaMode` stays a Win32-only row below.
    //! - Browser/media/launch keys (`BrowserBack`, `MediaPlayPause`, …) have
    //!   no AppKit table entries at all; those rows carry a Win32
    //!   scancode only.
    use ui_events::keyboard::Code;
    use ui_events_winit::keyboard::from_winit_code;
    use winit::keyboard::{KeyCode, PhysicalKey};

    use crate::shared::{keys::scancode_to_code, keys_macos::keycode_to_code};

    /// `(label, winit KeyCode, Win32 (scancode, extended), AppKit keycode)`.
    /// `None` means that backend has no table entry for this key.
    type PhysicalKeyRow = (&'static str, KeyCode, Option<(u16, bool)>, Option<u16>);

    const PHYSICAL_KEYS: &[PhysicalKeyRow] = &[
        // Letters/digits/punctuation (a sample, not all 26+10).
        ("KeyA", KeyCode::KeyA, Some((0x001E, false)), Some(0x00)),
        ("Digit1", KeyCode::Digit1, Some((0x0002, false)), Some(0x12)),
        ("Minus", KeyCode::Minus, Some((0x000C, false)), Some(0x1B)),
        ("Equal", KeyCode::Equal, Some((0x000D, false)), Some(0x18)),
        (
            "BracketLeft",
            KeyCode::BracketLeft,
            Some((0x001A, false)),
            Some(0x21),
        ),
        (
            "BracketRight",
            KeyCode::BracketRight,
            Some((0x001B, false)),
            Some(0x1E),
        ),
        (
            "Semicolon",
            KeyCode::Semicolon,
            Some((0x0027, false)),
            Some(0x29),
        ),
        ("Quote", KeyCode::Quote, Some((0x0028, false)), Some(0x27)),
        (
            "Backquote",
            KeyCode::Backquote,
            Some((0x0029, false)),
            Some(0x32),
        ),
        (
            "Backslash",
            KeyCode::Backslash,
            Some((0x002B, false)),
            Some(0x2A),
        ),
        ("Comma", KeyCode::Comma, Some((0x0033, false)), Some(0x2B)),
        ("Period", KeyCode::Period, Some((0x0034, false)), Some(0x2F)),
        ("Slash", KeyCode::Slash, Some((0x0035, false)), Some(0x2C)),
        // Editing / whitespace.
        ("Enter", KeyCode::Enter, Some((0x001C, false)), Some(0x24)),
        ("Escape", KeyCode::Escape, Some((0x0001, false)), Some(0x35)),
        ("Tab", KeyCode::Tab, Some((0x000F, false)), Some(0x30)),
        ("Space", KeyCode::Space, Some((0x0039, false)), Some(0x31)),
        (
            "Backspace",
            KeyCode::Backspace,
            Some((0x000E, false)),
            Some(0x33),
        ),
        (
            "CapsLock",
            KeyCode::CapsLock,
            Some((0x003A, false)),
            Some(0x39),
        ),
        // Sided modifiers.
        (
            "ShiftLeft",
            KeyCode::ShiftLeft,
            Some((0x002A, false)),
            Some(0x38),
        ),
        (
            "ShiftRight",
            KeyCode::ShiftRight,
            Some((0x0036, false)),
            Some(0x3C),
        ),
        (
            "ControlLeft",
            KeyCode::ControlLeft,
            Some((0x001D, false)),
            Some(0x3B),
        ),
        (
            "ControlRight",
            KeyCode::ControlRight,
            Some((0x001D, true)),
            Some(0x3E),
        ),
        (
            "AltLeft",
            KeyCode::AltLeft,
            Some((0x0038, false)),
            Some(0x3A),
        ),
        (
            "AltRight",
            KeyCode::AltRight,
            Some((0x0038, true)),
            Some(0x3D),
        ),
        (
            "MetaLeft",
            KeyCode::SuperLeft,
            Some((0x005B, true)),
            Some(0x37),
        ),
        (
            "MetaRight",
            KeyCode::SuperRight,
            Some((0x005C, true)),
            Some(0x36),
        ),
        // Navigation cluster.
        (
            "ArrowLeft",
            KeyCode::ArrowLeft,
            Some((0x004B, true)),
            Some(0x7B),
        ),
        (
            "ArrowRight",
            KeyCode::ArrowRight,
            Some((0x004D, true)),
            Some(0x7C),
        ),
        (
            "ArrowUp",
            KeyCode::ArrowUp,
            Some((0x0048, true)),
            Some(0x7E),
        ),
        (
            "ArrowDown",
            KeyCode::ArrowDown,
            Some((0x0050, true)),
            Some(0x7D),
        ),
        ("Home", KeyCode::Home, Some((0x0047, true)), Some(0x73)),
        ("End", KeyCode::End, Some((0x004F, true)), Some(0x77)),
        ("PageUp", KeyCode::PageUp, Some((0x0049, true)), Some(0x74)),
        (
            "PageDown",
            KeyCode::PageDown,
            Some((0x0051, true)),
            Some(0x79),
        ),
        // Apple's Insert/Delete labeling is a documented cross-platform
        // divergence in position, not in the `Code` value each produces.
        ("Insert", KeyCode::Insert, Some((0x0052, true)), Some(0x72)),
        ("Delete", KeyCode::Delete, Some((0x0053, true)), Some(0x75)),
        // NumLock: Win32's own key vs. the Mac numpad Clear key.
        (
            "NumLock",
            KeyCode::NumLock,
            Some((0x0045, true)),
            Some(0x47),
        ),
        // Numpad cluster.
        (
            "Numpad0",
            KeyCode::Numpad0,
            Some((0x0052, false)),
            Some(0x52),
        ),
        (
            "Numpad1",
            KeyCode::Numpad1,
            Some((0x004F, false)),
            Some(0x53),
        ),
        (
            "Numpad2",
            KeyCode::Numpad2,
            Some((0x0050, false)),
            Some(0x54),
        ),
        (
            "Numpad3",
            KeyCode::Numpad3,
            Some((0x0051, false)),
            Some(0x55),
        ),
        (
            "Numpad4",
            KeyCode::Numpad4,
            Some((0x004B, false)),
            Some(0x56),
        ),
        (
            "Numpad5",
            KeyCode::Numpad5,
            Some((0x004C, false)),
            Some(0x57),
        ),
        (
            "Numpad6",
            KeyCode::Numpad6,
            Some((0x004D, false)),
            Some(0x58),
        ),
        (
            "Numpad7",
            KeyCode::Numpad7,
            Some((0x0047, false)),
            Some(0x59),
        ),
        (
            "Numpad8",
            KeyCode::Numpad8,
            Some((0x0048, false)),
            Some(0x5B),
        ),
        (
            "Numpad9",
            KeyCode::Numpad9,
            Some((0x0049, false)),
            Some(0x5C),
        ),
        (
            "NumpadAdd",
            KeyCode::NumpadAdd,
            Some((0x004E, false)),
            Some(0x45),
        ),
        (
            "NumpadSubtract",
            KeyCode::NumpadSubtract,
            Some((0x004A, false)),
            Some(0x4E),
        ),
        (
            "NumpadMultiply",
            KeyCode::NumpadMultiply,
            Some((0x0037, false)),
            Some(0x43),
        ),
        (
            "NumpadDivide",
            KeyCode::NumpadDivide,
            Some((0x0035, true)),
            Some(0x4B),
        ),
        (
            "NumpadDecimal",
            KeyCode::NumpadDecimal,
            Some((0x0053, false)),
            Some(0x41),
        ),
        (
            "NumpadEnter",
            KeyCode::NumpadEnter,
            Some((0x001C, true)),
            Some(0x4C),
        ),
        (
            "NumpadEqual",
            KeyCode::NumpadEqual,
            Some((0x0059, false)),
            Some(0x51),
        ),
        (
            "NumpadComma",
            KeyCode::NumpadComma,
            Some((0x007E, false)),
            Some(0x5F),
        ),
        // System / IME keys.
        (
            "ContextMenu",
            KeyCode::ContextMenu,
            Some((0x005D, true)),
            Some(0x6E),
        ),
        ("Lang1", KeyCode::Lang1, Some((0x0072, false)), Some(0x68)),
        ("Lang2", KeyCode::Lang2, Some((0x0071, false)), Some(0x66)),
        (
            "IntlBackslash",
            KeyCode::IntlBackslash,
            Some((0x0056, false)),
            Some(0x0A),
        ),
        ("IntlRo", KeyCode::IntlRo, Some((0x0073, false)), Some(0x5E)),
        (
            "IntlYen",
            KeyCode::IntlYen,
            Some((0x007D, false)),
            Some(0x5D),
        ),
        // Media keys.
        (
            "AudioVolumeUp",
            KeyCode::AudioVolumeUp,
            Some((0x0030, true)),
            Some(0x48),
        ),
        (
            "AudioVolumeDown",
            KeyCode::AudioVolumeDown,
            Some((0x002E, true)),
            Some(0x49),
        ),
        (
            "AudioVolumeMute",
            KeyCode::AudioVolumeMute,
            Some((0x0020, true)),
            Some(0x4A),
        ),
        // Function keys, including the F13-F20 band all three tables cover.
        ("F1", KeyCode::F1, Some((0x003B, false)), Some(0x7A)),
        ("F12", KeyCode::F12, Some((0x0058, false)), Some(0x6F)),
        ("F13", KeyCode::F13, Some((0x0064, false)), Some(0x69)),
        ("F14", KeyCode::F14, Some((0x0065, false)), Some(0x6B)),
        ("F15", KeyCode::F15, Some((0x0066, false)), Some(0x71)),
        ("F16", KeyCode::F16, Some((0x0067, false)), Some(0x6A)),
        ("F17", KeyCode::F17, Some((0x0068, false)), Some(0x40)),
        ("F18", KeyCode::F18, Some((0x0069, false)), Some(0x4F)),
        ("F19", KeyCode::F19, Some((0x006A, false)), Some(0x50)),
        ("F20", KeyCode::F20, Some((0x006B, false)), Some(0x5A)),
        // Win32-only families: no AppKit table entry exists for these.
        (
            "ScrollLock",
            KeyCode::ScrollLock,
            Some((0x0046, false)),
            None,
        ),
        ("Pause", KeyCode::Pause, Some((0x0045, false)), None),
        (
            "PrintScreen",
            KeyCode::PrintScreen,
            Some((0x0037, true)),
            None,
        ),
        ("Power", KeyCode::Power, Some((0x005E, true)), None),
        ("KanaMode", KeyCode::KanaMode, Some((0x0070, false)), None),
        ("Convert", KeyCode::Convert, Some((0x0079, false)), None),
        (
            "NonConvert",
            KeyCode::NonConvert,
            Some((0x007B, false)),
            None,
        ),
        ("F21", KeyCode::F21, Some((0x006C, false)), None),
        ("F24", KeyCode::F24, Some((0x0076, false)), None),
        (
            "BrowserBack",
            KeyCode::BrowserBack,
            Some((0x006A, true)),
            None,
        ),
        (
            "BrowserForward",
            KeyCode::BrowserForward,
            Some((0x0069, true)),
            None,
        ),
        (
            "BrowserRefresh",
            KeyCode::BrowserRefresh,
            Some((0x0067, true)),
            None,
        ),
        (
            "BrowserSearch",
            KeyCode::BrowserSearch,
            Some((0x0065, true)),
            None,
        ),
        (
            "BrowserFavorites",
            KeyCode::BrowserFavorites,
            Some((0x0066, true)),
            None,
        ),
        (
            "BrowserStop",
            KeyCode::BrowserStop,
            Some((0x0068, true)),
            None,
        ),
        (
            "BrowserHome",
            KeyCode::BrowserHome,
            Some((0x0032, true)),
            None,
        ),
        (
            "MediaPlayPause",
            KeyCode::MediaPlayPause,
            Some((0x0022, true)),
            None,
        ),
        ("MediaStop", KeyCode::MediaStop, Some((0x0024, true)), None),
        (
            "MediaTrackNext",
            KeyCode::MediaTrackNext,
            Some((0x0019, true)),
            None,
        ),
        (
            "MediaTrackPrevious",
            KeyCode::MediaTrackPrevious,
            Some((0x0010, true)),
            None,
        ),
        (
            "MediaSelect",
            KeyCode::MediaSelect,
            Some((0x006D, true)),
            None,
        ),
        (
            "LaunchApp1",
            KeyCode::LaunchApp1,
            Some((0x006B, true)),
            None,
        ),
        (
            "LaunchApp2",
            KeyCode::LaunchApp2,
            Some((0x0021, true)),
            None,
        ),
        (
            "LaunchMail",
            KeyCode::LaunchMail,
            Some((0x006C, true)),
            None,
        ),
    ];

    #[test]
    fn winit_win32_and_appkit_agree_on_shared_physical_keys() {
        let mut disagreements = Vec::new();
        for &(label, winit_code, win32, appkit) in PHYSICAL_KEYS {
            let winit_result = from_winit_code(PhysicalKey::Code(winit_code));
            if let Some((scancode, extended)) = win32 {
                let win32_result = scancode_to_code(scancode, extended);
                if win32_result != winit_result {
                    disagreements.push(format!(
                        "{label}: winit={winit_result:?} win32={win32_result:?}"
                    ));
                }
            }
            if let Some(keycode) = appkit {
                let appkit_result = keycode_to_code(keycode);
                if appkit_result != winit_result {
                    disagreements.push(format!(
                        "{label}: winit={winit_result:?} appkit={appkit_result:?}"
                    ));
                }
            }
        }
        assert!(
            disagreements.is_empty(),
            "{} cross-backend disagreement(s):\n{}",
            disagreements.len(),
            disagreements.join("\n")
        );
    }

    /// Every code named in [`PHYSICAL_KEYS`] must itself be a real,
    /// non-`Unidentified` `Code` — guards against a copy/paste `None` that
    /// silently turns a real disagreement into a skipped row.
    #[test]
    fn physical_keys_table_never_expects_unidentified() {
        for &(label, winit_code, _, _) in PHYSICAL_KEYS {
            assert_ne!(
                from_winit_code(PhysicalKey::Code(winit_code)),
                Code::Unidentified,
                "{label}"
            );
        }
    }
}
