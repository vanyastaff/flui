//! Pointer buttons and the set of buttons held.

use core::fmt;
use core::num::NonZeroU8;

/// A button number [`PointerButton::try_from`] refuses: 0, 6 (the W3C eraser slot, which
/// FLUI reports as [`PenTool::Eraser`](super::PenTool::Eraser)) or above 32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{0} is not a pointer button number (1-5 or 7-32)")]
pub struct InvalidButtonNumber(pub u8);

/// One button of a pointer device, numbered from 1 the way the W3C `buttons` bits and the
/// platforms number them.
///
/// A touch contact and a pen tip in contact press [`PointerButton::PRIMARY`]. The pen's eraser
/// is not a button here: it is the pen's tool ([`PenTool::Eraser`](super::PenTool::Eraser)),
/// because the eraser end can come into range without any contact.
///
/// # Examples
///
/// ```
/// use flui_platform_api::pointer::PointerButton;
///
/// assert_eq!(PointerButton::try_from(1), Ok(PointerButton::PRIMARY));
/// assert_eq!(PointerButton::try_from(7).map(|b| b.number().get()), Ok(7));
/// assert!(PointerButton::try_from(6).is_err()); // the eraser is a tool
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PointerButton(u8);

impl PointerButton {
    /// Button 1, the main button: the left mouse button, a touch contact, a pen tip in contact.
    pub const PRIMARY: Self = Self(0);
    /// Button 2: the right mouse button, or a pen's barrel button.
    pub const SECONDARY: Self = Self(1);
    /// Button 3: the middle mouse button (the wheel pressed).
    pub const AUXILIARY: Self = Self(2);
    /// Button 4: the "back" side button (X1).
    pub const BACK: Self = Self(3);
    /// Button 5: the "forward" side button (X2).
    pub const FORWARD: Self = Self(4);

    /// The button's number, counted from 1: 1 is [`PRIMARY`](Self::PRIMARY), 5 is
    /// [`FORWARD`](Self::FORWARD).
    #[must_use]
    pub const fn number(self) -> NonZeroU8 {
        NonZeroU8::new(self.0 + 1).expect("BUG: a button index is below 32")
    }

    const fn bit(self) -> u32 {
        1 << self.0
    }
}

impl TryFrom<u8> for PointerButton {
    type Error = InvalidButtonNumber;

    /// The button numbered `number`.
    ///
    /// # Errors
    ///
    /// [`InvalidButtonNumber`] for 0, 6 or a number above 32.
    fn try_from(number: u8) -> Result<Self, Self::Error> {
        match number {
            1..=5 | 7..=32 => Ok(Self(number - 1)),
            _ => Err(InvalidButtonNumber(number)),
        }
    }
}

impl fmt::Debug for PointerButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::PRIMARY => f.write_str("PRIMARY"),
            Self::SECONDARY => f.write_str("SECONDARY"),
            Self::AUXILIARY => f.write_str("AUXILIARY"),
            Self::BACK => f.write_str("BACK"),
            Self::FORWARD => f.write_str("FORWARD"),
            other => write!(f, "Button{}", other.number()),
        }
    }
}

/// The set of buttons held on one pointer.
///
/// # Examples
///
/// ```
/// use flui_platform_api::pointer::{PointerButton, PointerButtons};
///
/// let held = PointerButtons::only(PointerButton::PRIMARY).with(PointerButton::SECONDARY);
/// assert!(held.contains(PointerButton::SECONDARY));
/// assert!(held.without(PointerButton::PRIMARY).without(PointerButton::SECONDARY).is_empty());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PointerButtons(u32);

impl PointerButtons {
    /// No button held.
    pub const NONE: Self = Self(0);

    /// The set holding only `button`.
    #[must_use]
    pub const fn only(button: PointerButton) -> Self {
        Self(button.bit())
    }

    /// This set with `button` held.
    #[must_use]
    pub const fn with(self, button: PointerButton) -> Self {
        Self(self.0 | button.bit())
    }

    /// This set with `button` released.
    #[must_use]
    pub const fn without(self, button: PointerButton) -> Self {
        Self(self.0 & !button.bit())
    }

    /// Whether `button` is held.
    #[must_use]
    pub const fn contains(self, button: PointerButton) -> bool {
        self.0 & button.bit() != 0
    }

    /// Whether no button is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The held buttons, lowest number first.
    pub fn iter(self) -> impl Iterator<Item = PointerButton> {
        (0..32_u8)
            .map(PointerButton)
            .filter(move |button| self.contains(*button))
    }
}

impl FromIterator<PointerButton> for PointerButtons {
    fn from_iter<I: IntoIterator<Item = PointerButton>>(buttons: I) -> Self {
        buttons.into_iter().fold(Self::NONE, Self::with)
    }
}

impl fmt::Debug for PointerButtons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}
