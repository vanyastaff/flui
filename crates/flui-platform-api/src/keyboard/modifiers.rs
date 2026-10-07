//! The modifier and lock keys held during an event.

use core::fmt;
use core::ops::{BitOr, BitOrAssign};

/// The set of modifier and lock keys active during an input event: the W3C
/// `getModifierState` keys.
///
/// The legacy `Hyper` and `Super` modifiers the specification lists are reported as
/// [`Modifiers::META`], which the specification names in their place.
///
/// # Examples
///
/// ```
/// use flui_platform_api::keyboard::Modifiers;
///
/// let held = Modifiers::CONTROL | Modifiers::SHIFT;
/// assert!(held.contains(Modifiers::CONTROL));
/// assert!(!held.contains(Modifiers::CONTROL | Modifiers::ALT));
/// assert_eq!(format!("{held:?}"), "{CONTROL, SHIFT}");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u16);

impl Modifiers {
    /// No modifier.
    pub const NONE: Self = Self(0);
    /// The Alt key (Option on Apple keyboards).
    pub const ALT: Self = Self(1 << 0);
    /// The AltGr key, or Alt and Control together where the layout treats them as AltGr.
    pub const ALT_GRAPH: Self = Self(1 << 1);
    /// Caps Lock is on.
    pub const CAPS_LOCK: Self = Self(1 << 2);
    /// The Control key.
    pub const CONTROL: Self = Self(1 << 3);
    /// The Fn key.
    pub const FN: Self = Self(1 << 4);
    /// Fn Lock is on.
    pub const FN_LOCK: Self = Self(1 << 5);
    /// The Meta key: Command on Apple keyboards, the Windows logo key on Windows.
    pub const META: Self = Self(1 << 6);
    /// Num Lock is on.
    pub const NUM_LOCK: Self = Self(1 << 7);
    /// Scroll Lock is on.
    pub const SCROLL_LOCK: Self = Self(1 << 8);
    /// The Shift key.
    pub const SHIFT: Self = Self(1 << 9);
    /// The Symbol key.
    pub const SYMBOL: Self = Self(1 << 10);
    /// Symbol Lock is on.
    pub const SYMBOL_LOCK: Self = Self(1 << 11);

    const NAMES: [(Self, &'static str); 12] = [
        (Self::ALT, "ALT"),
        (Self::ALT_GRAPH, "ALT_GRAPH"),
        (Self::CAPS_LOCK, "CAPS_LOCK"),
        (Self::CONTROL, "CONTROL"),
        (Self::FN, "FN"),
        (Self::FN_LOCK, "FN_LOCK"),
        (Self::META, "META"),
        (Self::NUM_LOCK, "NUM_LOCK"),
        (Self::SCROLL_LOCK, "SCROLL_LOCK"),
        (Self::SHIFT, "SHIFT"),
        (Self::SYMBOL, "SYMBOL"),
        (Self::SYMBOL_LOCK, "SYMBOL_LOCK"),
    ];

    /// Whether every modifier in `other` is active.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no modifier is active.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The modifiers active in `self` or in `other`.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, other: Self) {
        *self = self.union(other);
    }
}

impl fmt::Debug for Modifiers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut set = f.debug_set();
        for (modifier, name) in Self::NAMES {
            if self.contains(modifier) {
                set.entry(&format_args!("{name}"));
            }
        }
        set.finish()
    }
}
