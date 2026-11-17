//! What a field asks of the input method and the on-screen keyboard: the
//! layout its purpose names, the label of the Enter key, how typed text is
//! capitalized and corrected, and what the system may fill it with.
//!
//! Each value is a request the host passes on in its platform's terms
//! (UIKit's `UITextInputTraits`, Android's `EditorInfo`, the Web's
//! `inputmode`, `enterkeyhint`, `autocapitalize`, `autocorrect`, `spellcheck`
//! and `autocomplete`, `text-input-v3`'s content type, IBus's and Fcitx5's
//! hints, TSF's input scope). A host whose platform has no word for one
//! leaves it out; none of them filters what the field accepts.

/// What the field asks of the input method and the on-screen keyboard
/// ([`TextStoreStatus::text_input`](super::TextStoreStatus::text_input)).
///
/// Built by [`Self::new`] from the field's purpose, which picks the defaults
/// of the rest, and then the builder methods. The purpose has no builder:
/// the defaults it picks would otherwise depend on the order the methods
/// were called in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TextInputOptions {
    /// What the field is for, which picks the keyboard's layout.
    pub purpose: TextInputPurpose,
    /// What the on-screen keyboard's Enter key says.
    pub enter_action: EnterAction,
    /// Which letters the keyboard shifts on its own.
    pub capitalization: Capitalization,
    /// Whether the input method suggests and corrects words.
    pub correction: Correction,
    /// What the system may fill the field with, beyond what its purpose says.
    pub autofill: Option<Autofill>,
}

impl TextInputOptions {
    /// A free-text field's: [`Self::new`] of [`TextInputPurpose::Text`].
    pub const TEXT: Self = Self::new(TextInputPurpose::Text);

    /// A field for `purpose`, with the defaults the purpose picks:
    ///
    /// - the Enter key says [`EnterAction::Search`] for a
    ///   [`TextInputPurpose::Search`] field, and the keyboard's own label
    ///   ([`EnterAction::Default`]) otherwise;
    /// - prose (free text, a search) is [`Correction::Autocorrect`]ed, and an
    ///   address, a telephone number or a number is not corrected at all
    ///   ([`Correction::Off`]), since a correction of one is a mistake;
    /// - no letter is shifted on its own ([`Capitalization::None`]) and
    ///   nothing is offered to fill the field.
    #[must_use]
    pub const fn new(purpose: TextInputPurpose) -> Self {
        let (enter_action, correction) = match purpose {
            TextInputPurpose::Text => (EnterAction::Default, Correction::Autocorrect),
            TextInputPurpose::Search => (EnterAction::Search, Correction::Autocorrect),
            TextInputPurpose::Email
            | TextInputPurpose::Url
            | TextInputPurpose::Phone
            | TextInputPurpose::Number(_) => (EnterAction::Default, Correction::Off),
        };
        Self {
            purpose,
            enter_action,
            capitalization: Capitalization::None,
            correction,
            autofill: None,
        }
    }

    /// These options with the Enter key saying `enter_action`.
    #[must_use]
    pub const fn enter_action(self, enter_action: EnterAction) -> Self {
        Self {
            enter_action,
            ..self
        }
    }

    /// These options with the keyboard shifting `capitalization` on its own.
    #[must_use]
    pub const fn capitalization(self, capitalization: Capitalization) -> Self {
        Self {
            capitalization,
            ..self
        }
    }

    /// These options with the input method suggesting and correcting as
    /// `correction` says.
    #[must_use]
    pub const fn correction(self, correction: Correction) -> Self {
        Self { correction, ..self }
    }

    /// These options with the system offering to fill the field with
    /// `autofill`.
    #[must_use]
    pub const fn autofill(self, autofill: Autofill) -> Self {
        Self {
            autofill: Some(autofill),
            ..self
        }
    }
}

impl Default for TextInputOptions {
    fn default() -> Self {
        Self::TEXT
    }
}

/// What a text field is for.
///
/// Independent of [`TextStoreStatus::protected`](super::TextStoreStatus::protected):
/// a protected field hides its text and tells input methods not to learn
/// from it, whatever its purpose. A purpose only shapes the layout and
/// suggestions an input method offers; it never filters what the field
/// accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TextInputPurpose {
    /// Free text.
    #[default]
    Text,
    /// An email address.
    Email,
    /// A web address.
    Url,
    /// A telephone number.
    Phone,
    /// A number, with the characters it may hold beside the digits.
    Number(NumberFormat),
    /// A search query.
    Search,
}

/// What a number field's keyboard offers beside the digits.
///
/// Where the platform has no keyboard for a combination (UIKit has no pad
/// with a minus sign, so a signed number gets the numbers-and-punctuation
/// keyboard), the host picks the nearest one that can type every character
/// the format allows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_structs,
    reason = "a minus sign and a decimal separator, each offered or not"
)]
pub struct NumberFormat {
    /// A minus sign: the number may be negative.
    pub signed: bool,
    /// A decimal separator: the number may have a fractional part.
    pub decimal: bool,
}

impl NumberFormat {
    /// A whole number of zero or more: digits only.
    pub const WHOLE: Self = Self {
        signed: false,
        decimal: false,
    };
}

/// What the on-screen keyboard's Enter key says.
///
/// A label, which does not change what the key sends: its press reaches the
/// field as the Enter key on every host, and the field acts on it by its own
/// action (the framework's field moves the focus forward on [`Self::Next`]
/// and back on [`Self::Previous`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EnterAction {
    /// The keyboard's own label: a line break in a field of many lines, and
    /// whatever the platform shows for a field of one (Return, Done or an
    /// arrow).
    #[default]
    Default,
    /// Done: the entry is finished.
    Done,
    /// Go: open what the field names (an address).
    Go,
    /// Next: on to the next field.
    Next,
    /// Previous: back to the field before. UIKit has no such key and shows
    /// its own label.
    Previous,
    /// Search: run the query.
    Search,
    /// Send: send the message.
    Send,
}

/// Which letters the on-screen keyboard shifts on its own.
///
/// A request: the user can still type either case, and a hardware keyboard
/// shifts nothing on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Capitalization {
    /// None: every letter is typed as the keyboard's shift state says.
    #[default]
    None,
    /// The first letter of each sentence.
    Sentences,
    /// The first letter of each word.
    Words,
    /// Every letter.
    Characters,
}

/// Whether the input method suggests and corrects words as the user types.
///
/// A protected field is neither suggested nor corrected, whatever this says:
/// its text never reaches a dictionary.
///
/// Its default depends on the purpose ([`TextInputOptions::new`]), so it has
/// no `Default` of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Correction {
    /// Neither: no suggestion strip, no spell check, no correction (a code,
    /// a user name, an identifier).
    Off,
    /// Suggestions and the spell check's marks, but no word replaced without
    /// the user picking it (a person's name, a title).
    Suggestions,
    /// Suggestions, and a misspelled word replaced as the user types on.
    Autocorrect,
}

/// What the system may fill a field with, from the user's saved credentials
/// or a code it received: a request a password manager or the keyboard's
/// suggestion strip answers.
///
/// An address, a telephone number and a web address are filled by their
/// [`TextInputPurpose`]; these are the kinds a purpose does not say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Autofill {
    /// The user name of an account.
    Username,
    /// An account's existing password: offered from what is saved.
    CurrentPassword,
    /// A password being chosen: the system may suggest a strong one and save
    /// it.
    NewPassword,
    /// A one-time code sent by message or email.
    OneTimeCode,
}
