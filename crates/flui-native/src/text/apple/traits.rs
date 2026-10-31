//! What UIKit's keyboard is told of the focused field (`UITextInputTraits`):
//! whether it is secure, which keyboard it shows and whether it corrects and
//! checks spelling, from the field's [`TextStoreStatus`]. Pure, so the choice
//! runs on every host; the iOS responder answers UIKit's trait getters from
//! it.

use flui_platform::text_store::{TextInputPurpose, TextStoreStatus};

/// `UIKeyboardType` values, from UIKit's `UITextInputTraits.h`.
pub(crate) mod keyboard {
    /// `UIKeyboardTypeDefault`.
    pub(crate) const DEFAULT: isize = 0;
    /// `UIKeyboardTypeURL`.
    pub(crate) const URL: isize = 3;
    /// `UIKeyboardTypeNumberPad`.
    pub(crate) const NUMBER_PAD: isize = 4;
    /// `UIKeyboardTypePhonePad`.
    pub(crate) const PHONE_PAD: isize = 5;
    /// `UIKeyboardTypeEmailAddress`.
    pub(crate) const EMAIL_ADDRESS: isize = 7;
    /// `UIKeyboardTypeWebSearch`.
    pub(crate) const WEB_SEARCH: isize = 10;
}

/// `UITextAutocorrectionType` and `UITextSpellCheckingType` values, which
/// UIKit numbers alike.
pub(crate) mod checking {
    /// `...TypeDefault`: the keyboard's own setting.
    pub(crate) const DEFAULT: isize = 0;
    /// `...TypeNo`.
    pub(crate) const NO: isize = 1;
}

/// The traits the responder reports for one field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyboardTraits {
    /// `isSecureTextEntry`: the keyboard neither shows, suggests, learns
    /// nor records what is typed, and the field's text is kept out of
    /// screen recordings of the keyboard.
    pub(crate) secure: bool,
    /// `keyboardType`.
    pub(crate) keyboard: isize,
    /// `autocorrectionType`.
    pub(crate) autocorrection: isize,
    /// `spellCheckingType`.
    pub(crate) spell_checking: isize,
}

impl KeyboardTraits {
    /// A free-text field's: the keyboard's own defaults.
    pub(crate) const DEFAULT: Self = Self {
        secure: false,
        keyboard: keyboard::DEFAULT,
        autocorrection: checking::DEFAULT,
        spell_checking: checking::DEFAULT,
    };
}

impl Default for KeyboardTraits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The traits for a field of `status`: the keyboard its purpose names, no
/// correction where a word is not prose (an address, a number), and a
/// protected field secure, uncorrected and unchecked, whatever its purpose.
pub(crate) const fn keyboard_traits(status: TextStoreStatus) -> KeyboardTraits {
    let (keyboard, prose) = match status.purpose {
        TextInputPurpose::Email => (keyboard::EMAIL_ADDRESS, false),
        TextInputPurpose::Url => (keyboard::URL, false),
        TextInputPurpose::Number => (keyboard::NUMBER_PAD, false),
        TextInputPurpose::Phone => (keyboard::PHONE_PAD, false),
        TextInputPurpose::Search => (keyboard::WEB_SEARCH, true),
        // Free text, and any purpose added later that has no keyboard here.
        _ => (keyboard::DEFAULT, true),
    };
    let checked = if prose && !status.protected {
        checking::DEFAULT
    } else {
        checking::NO
    };
    KeyboardTraits {
        secure: status.protected,
        keyboard,
        autocorrection: checked,
        spell_checking: checked,
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::text_store::{TextInputPurpose, TextStoreStatus};

    use super::{KeyboardTraits, checking, keyboard, keyboard_traits};

    #[test]
    fn a_fields_status_picks_its_keyboard_traits() {
        let plain = TextStoreStatus::EDITABLE_SINGLE_LINE;
        let traits = |secure, keyboard, checked| KeyboardTraits {
            secure,
            keyboard,
            autocorrection: checked,
            spell_checking: checked,
        };
        let rows = [
            (plain, traits(false, keyboard::DEFAULT, checking::DEFAULT)),
            (
                plain.with_purpose(TextInputPurpose::Email),
                traits(false, keyboard::EMAIL_ADDRESS, checking::NO),
            ),
            (
                plain.with_purpose(TextInputPurpose::Url),
                traits(false, keyboard::URL, checking::NO),
            ),
            (
                plain.with_purpose(TextInputPurpose::Number),
                traits(false, keyboard::NUMBER_PAD, checking::NO),
            ),
            (
                plain.with_purpose(TextInputPurpose::Phone),
                traits(false, keyboard::PHONE_PAD, checking::NO),
            ),
            (
                plain.with_purpose(TextInputPurpose::Search),
                traits(false, keyboard::WEB_SEARCH, checking::DEFAULT),
            ),
            (
                plain.with_protected(true),
                traits(true, keyboard::DEFAULT, checking::NO),
            ),
            (
                plain
                    .with_protected(true)
                    .with_purpose(TextInputPurpose::Number),
                traits(true, keyboard::NUMBER_PAD, checking::NO),
            ),
        ];
        for (status, expected) in rows {
            assert_eq!(keyboard_traits(status), expected, "{status:?}");
        }
        assert_eq!(KeyboardTraits::default(), keyboard_traits(plain));
    }
}
