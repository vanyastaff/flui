//! What UIKit's keyboard is told of the focused field (`UITextInputTraits`):
//! whether it is secure, which keyboard it shows, what its Return key says,
//! how it capitalizes, corrects and checks spelling, and what it may fill
//! the field with, from the field's [`TextStoreStatus`]. Pure, so the choice
//! runs on every host; the iOS responder answers UIKit's trait getters from
//! it.

use flui_platform::text_store::{
    Autofill, Capitalization, Correction, EnterAction, NumberFormat, TextInputPurpose,
    TextStoreStatus,
};

/// `UIKeyboardType` values, from UIKit's `UITextInputTraits.h`.
pub(crate) mod keyboard {
    /// `UIKeyboardTypeDefault`.
    pub(crate) const DEFAULT: isize = 0;
    /// `UIKeyboardTypeNumbersAndPunctuation`: digits with a minus sign and a
    /// decimal separator, the one keyboard UIKit has for a signed number.
    pub(crate) const NUMBERS_AND_PUNCTUATION: isize = 2;
    /// `UIKeyboardTypeURL`.
    pub(crate) const URL: isize = 3;
    /// `UIKeyboardTypeNumberPad`.
    pub(crate) const NUMBER_PAD: isize = 4;
    /// `UIKeyboardTypePhonePad`.
    pub(crate) const PHONE_PAD: isize = 5;
    /// `UIKeyboardTypeEmailAddress`.
    pub(crate) const EMAIL_ADDRESS: isize = 7;
    /// `UIKeyboardTypeDecimalPad`.
    pub(crate) const DECIMAL_PAD: isize = 8;
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

/// `UITextAutocapitalizationType` values.
pub(crate) mod capitalization {
    /// `UITextAutocapitalizationTypeNone`.
    pub(crate) const NONE: isize = 0;
    /// `UITextAutocapitalizationTypeWords`.
    pub(crate) const WORDS: isize = 1;
    /// `UITextAutocapitalizationTypeSentences`.
    pub(crate) const SENTENCES: isize = 2;
    /// `UITextAutocapitalizationTypeAllCharacters`.
    pub(crate) const ALL_CHARACTERS: isize = 3;
}

/// `UIReturnKeyType` values.
pub(crate) mod return_key {
    /// `UIReturnKeyDefault`: "return".
    pub(crate) const DEFAULT: isize = 0;
    /// `UIReturnKeyGo`.
    pub(crate) const GO: isize = 1;
    /// `UIReturnKeyNext`.
    pub(crate) const NEXT: isize = 4;
    /// `UIReturnKeySearch`.
    pub(crate) const SEARCH: isize = 6;
    /// `UIReturnKeySend`.
    pub(crate) const SEND: isize = 7;
    /// `UIReturnKeyDone`.
    pub(crate) const DONE: isize = 9;
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
    /// `returnKeyType`.
    pub(crate) return_key: isize,
    /// `autocapitalizationType`.
    pub(crate) autocapitalization: isize,
    /// `autocorrectionType`.
    pub(crate) autocorrection: isize,
    /// `spellCheckingType`.
    pub(crate) spell_checking: isize,
    /// `textContentType`, which the responder answers with UIKit's constant
    /// for it; `None` answers `nil`.
    pub(crate) content: Option<Autofill>,
}

impl KeyboardTraits {
    /// A free-text field's.
    pub(crate) const DEFAULT: Self = keyboard_traits(TextStoreStatus::EDITABLE_SINGLE_LINE);
}

impl Default for KeyboardTraits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The traits for a field of `status`: the keyboard its purpose names (the
/// numbers-and-punctuation keyboard for a signed number, which no pad
/// types), the Return key its Enter action names (UIKit has no Previous and
/// shows "return"), its capitalization and correction (UIKit has no
/// suggestions without corrections, so [`Correction::Suggestions`] keeps
/// only the spell check), and what it may be filled with; a protected field
/// is secure, uncorrected and unchecked, whatever it asks.
pub(crate) const fn keyboard_traits(status: TextStoreStatus) -> KeyboardTraits {
    let options = status.text_input;
    let keyboard = match options.purpose {
        TextInputPurpose::Email => keyboard::EMAIL_ADDRESS,
        TextInputPurpose::Url => keyboard::URL,
        TextInputPurpose::Number(NumberFormat {
            signed: false,
            decimal: false,
        }) => keyboard::NUMBER_PAD,
        TextInputPurpose::Number(NumberFormat {
            signed: false,
            decimal: true,
        }) => keyboard::DECIMAL_PAD,
        TextInputPurpose::Number(_) => keyboard::NUMBERS_AND_PUNCTUATION,
        TextInputPurpose::Phone => keyboard::PHONE_PAD,
        TextInputPurpose::Search => keyboard::WEB_SEARCH,
        // Free text, and any purpose added later that has no keyboard here.
        _ => keyboard::DEFAULT,
    };
    let return_key = match options.enter_action {
        EnterAction::Done => return_key::DONE,
        EnterAction::Go => return_key::GO,
        EnterAction::Next => return_key::NEXT,
        EnterAction::Search => return_key::SEARCH,
        EnterAction::SendMessage => return_key::SEND,
        // The keyboard's own, Previous (no such key) and any added later.
        _ => return_key::DEFAULT,
    };
    let autocapitalization = match options.capitalization {
        Capitalization::Sentences => capitalization::SENTENCES,
        Capitalization::Words => capitalization::WORDS,
        Capitalization::Characters => capitalization::ALL_CHARACTERS,
        // None, and any added later.
        _ => capitalization::NONE,
    };
    let (autocorrection, spell_checking) = match options.correction {
        _ if status.holds_secret() => (checking::NO, checking::NO),
        Correction::Autocorrect => (checking::DEFAULT, checking::DEFAULT),
        Correction::Suggestions => (checking::NO, checking::DEFAULT),
        // Off, and any added later.
        _ => (checking::NO, checking::NO),
    };
    KeyboardTraits {
        secure: status.protected,
        keyboard,
        return_key,
        autocapitalization,
        autocorrection,
        spell_checking,
        content: options.autofill,
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::text_store::{
        Autofill, Capitalization, Correction, EnterAction, NumberFormat, TextInputOptions,
        TextInputPurpose, TextStoreStatus,
    };

    use super::{KeyboardTraits, capitalization, checking, keyboard, keyboard_traits, return_key};

    #[test]
    fn a_fields_purpose_picks_its_keyboard() {
        let of = |purpose| {
            keyboard_traits(
                TextStoreStatus::EDITABLE_SINGLE_LINE
                    .with_text_input(TextInputOptions::new(purpose)),
            )
        };
        let number = |signed, decimal| TextInputPurpose::Number(NumberFormat { signed, decimal });
        // Prose is checked by the keyboard's own setting; an address or a
        // number is neither corrected nor spell-checked.
        let rows = [
            (TextInputPurpose::Text, keyboard::DEFAULT, checking::DEFAULT),
            (
                TextInputPurpose::Email,
                keyboard::EMAIL_ADDRESS,
                checking::NO,
            ),
            (TextInputPurpose::Url, keyboard::URL, checking::NO),
            (number(false, false), keyboard::NUMBER_PAD, checking::NO),
            (number(false, true), keyboard::DECIMAL_PAD, checking::NO),
            (
                number(true, false),
                keyboard::NUMBERS_AND_PUNCTUATION,
                checking::NO,
            ),
            (
                number(true, true),
                keyboard::NUMBERS_AND_PUNCTUATION,
                checking::NO,
            ),
            (TextInputPurpose::Phone, keyboard::PHONE_PAD, checking::NO),
            (
                TextInputPurpose::Search,
                keyboard::WEB_SEARCH,
                checking::DEFAULT,
            ),
        ];
        for (purpose, expected_keyboard, checked) in rows {
            let traits = of(purpose);
            assert_eq!(traits.keyboard, expected_keyboard, "{purpose:?}");
            assert_eq!(traits.autocorrection, checked, "{purpose:?}");
            assert_eq!(traits.spell_checking, checked, "{purpose:?}");
        }
        assert_eq!(of(TextInputPurpose::Search).return_key, return_key::SEARCH);
        assert_eq!(KeyboardTraits::default(), of(TextInputPurpose::Text));
    }

    #[test]
    fn a_fields_options_pick_its_return_key_capitalization_correction_and_content() {
        let traits = |options: TextInputOptions| {
            keyboard_traits(TextStoreStatus::EDITABLE_SINGLE_LINE.with_text_input(options))
        };
        let text = TextInputOptions::TEXT;
        for (action, expected) in [
            (EnterAction::Default, return_key::DEFAULT),
            (EnterAction::Done, return_key::DONE),
            (EnterAction::Go, return_key::GO),
            (EnterAction::Next, return_key::NEXT),
            (EnterAction::Previous, return_key::DEFAULT),
            (EnterAction::Search, return_key::SEARCH),
            (EnterAction::SendMessage, return_key::SEND),
        ] {
            assert_eq!(
                traits(text.enter_action(action)).return_key,
                expected,
                "{action:?}"
            );
        }
        for (shift, expected) in [
            (Capitalization::None, capitalization::NONE),
            (Capitalization::Sentences, capitalization::SENTENCES),
            (Capitalization::Words, capitalization::WORDS),
            (Capitalization::Characters, capitalization::ALL_CHARACTERS),
        ] {
            assert_eq!(
                traits(text.capitalization(shift)).autocapitalization,
                expected,
                "{shift:?}"
            );
        }
        for (correction, corrected, spelled) in [
            (Correction::Off, checking::NO, checking::NO),
            (Correction::Suggestions, checking::NO, checking::DEFAULT),
            (
                Correction::Autocorrect,
                checking::DEFAULT,
                checking::DEFAULT,
            ),
        ] {
            let checked = traits(text.correction(correction));
            assert_eq!(
                (checked.autocorrection, checked.spell_checking),
                (corrected, spelled),
                "{correction:?}"
            );
        }
        for kind in [
            Autofill::Username,
            Autofill::CurrentPassword,
            Autofill::NewPassword,
            Autofill::OneTimeCode,
        ] {
            assert_eq!(traits(text.autofill(kind)).content, Some(kind));
        }
        assert_eq!(traits(text).content, None);
    }

    #[test]
    fn a_protected_field_is_secure_and_unchecked_whatever_it_asks() {
        let asked = TextInputOptions::new(TextInputPurpose::Number(NumberFormat::WHOLE))
            .correction(Correction::Autocorrect)
            .autofill(Autofill::NewPassword);
        let traits = keyboard_traits(
            TextStoreStatus::EDITABLE_SINGLE_LINE
                .with_protected(true)
                .with_text_input(asked),
        );
        assert!(traits.secure);
        assert_eq!(traits.keyboard, keyboard::NUMBER_PAD);
        assert_eq!(
            (traits.autocorrection, traits.spell_checking),
            (checking::NO, checking::NO)
        );
        assert_eq!(traits.content, Some(Autofill::NewPassword));

        // A one-time code shown in clear is not secure entry, but no
        // dictionary sees it either.
        let code = keyboard_traits(
            TextStoreStatus::EDITABLE_SINGLE_LINE
                .with_text_input(TextInputOptions::TEXT.autofill(Autofill::OneTimeCode)),
        );
        assert!(!code.secure);
        assert_eq!(
            (code.autocorrection, code.spell_checking),
            (checking::NO, checking::NO)
        );
        assert_eq!(code.content, Some(Autofill::OneTimeCode));
    }
}
