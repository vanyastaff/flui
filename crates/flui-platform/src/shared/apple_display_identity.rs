//! How the macOS and iOS display sources name a display.
//!
//! macOS keys a screen by the UUID of the display behind it. iOS has no such
//! identifier, so the source numbers each `UIScreen` it retains and recognises
//! the screen again only through the object it holds.

use flui_platform_api::DisplayKey;

/// The key of a macOS screen, from the `NSScreenNumber` of its
/// `deviceDescription` and a lookup of the display UUID for a
/// `CGDirectDisplayID`.
///
/// `None` leaves the screen out of the snapshot: it has no display number, the
/// number is not a `CGDirectDisplayID`, or CoreGraphics knows no UUID for it.
/// The UUID identifies the display, not its current connection, so the same
/// monitor keeps its key when it is replugged and macOS hands it a new
/// `CGDirectDisplayID`; two identical monitors have two UUIDs.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_display_key(
    screen_number: Option<u64>,
    uuid_of: impl FnOnce(u32) -> Option<[u8; 16]>,
) -> Option<DisplayKey> {
    let display_id = u32::try_from(screen_number?).ok()?;
    uuid_of(display_id).map(DisplayKey::MacosUuid)
}

/// An object that can tell whether another handle names the very same native
/// object.
#[cfg(any(target_os = "ios", test))]
pub(crate) trait SameObject {
    /// `true` when both handles refer to one live object.
    fn same_object(&self, other: &Self) -> bool;
}

/// The screens an iOS source retains, each with the number that keys it.
///
/// A screen is recognised by comparing it with the objects the ledger holds.
/// Because the ledger holds them, a held object's address cannot belong to
/// anything else; a screen the ledger no longer holds is never matched again
/// and its number is never reissued.
#[cfg(any(target_os = "ios", test))]
#[derive(Debug)]
pub(crate) struct ScreenLedger<S> {
    retained: Vec<(S, u64)>,
    /// The next number to assign; `None` once the counter is used up.
    next: Option<u64>,
}

#[cfg(any(target_os = "ios", test))]
impl<S: Clone + SameObject> ScreenLedger<S> {
    pub(crate) fn new() -> Self {
        Self {
            retained: Vec::new(),
            next: Some(1),
        }
    }

    /// The number of `screen`: its own when the ledger holds it, otherwise a
    /// new number, retaining the screen. `None` once the counter is used up
    /// and the screen is not already held.
    pub(crate) fn number_of(&mut self, screen: &S) -> Option<u64> {
        if let Some((_, number)) = self
            .retained
            .iter()
            .find(|(held, _)| held.same_object(screen))
        {
            return Some(*number);
        }
        let number = self.next?;
        self.next = number.checked_add(1);
        self.retained.push((screen.clone(), number));
        Some(number)
    }

    /// Release every held screen that is not among `live`.
    pub(crate) fn release_unlisted(&mut self, live: &[S]) {
        self.retained
            .retain(|(held, _)| live.iter().any(|screen| screen.same_object(held)));
    }
}

#[cfg(test)]
mod tests {
    use flui_platform_api::{DisplayKey, DisplayRegistry};

    use super::{SameObject, ScreenLedger, macos_display_key};

    const UUID_A: [u8; 16] = [0xA0; 16];
    const UUID_B: [u8; 16] = [0xB0; 16];

    #[test]
    fn macos_display_key_is_the_display_uuid() {
        let registry = DisplayRegistry::new();
        let id_of = |number, uuid| {
            let key = macos_display_key(number, |_| uuid).expect("BUG: the screen is keyed");
            registry.mint(&key).expect("ids are not exhausted")
        };
        // The same monitor replugged: a new display number, the same UUID.
        let before = id_of(Some(3), Some(UUID_A));
        let replugged = id_of(Some(7), Some(UUID_A));
        assert_eq!(before, replugged);
        // Another monitor, even one that now holds the old number.
        assert_ne!(before, id_of(Some(3), Some(UUID_B)));

        assert_eq!(
            macos_display_key(Some(3), |_| Some(UUID_A)),
            Some(DisplayKey::MacosUuid(UUID_A))
        );
        // The lookup is asked for the display number of the screen.
        assert_eq!(
            macos_display_key(Some(12), |number| (number == 12).then_some(UUID_B)),
            Some(DisplayKey::MacosUuid(UUID_B))
        );
    }

    #[test]
    fn macos_screen_without_a_display_number_or_uuid_is_left_out() {
        let mut asked = false;
        assert_eq!(
            macos_display_key(None, |_| {
                asked = true;
                Some(UUID_A)
            }),
            None
        );
        assert!(!asked, "no number means no lookup");
        assert_eq!(
            macos_display_key(Some(u64::from(u32::MAX) + 1), |_| Some(UUID_A)),
            None
        );
        assert_eq!(macos_display_key(Some(3), |_| None), None);
    }

    /// A stand-in for a retained native object: clones share `object`.
    #[derive(Debug, Clone)]
    struct Screen {
        object: u32,
    }

    impl SameObject for Screen {
        fn same_object(&self, other: &Self) -> bool {
            self.object == other.object
        }
    }

    #[test]
    fn ios_screen_keeps_its_number_while_retained() {
        let mut ledger = ScreenLedger::new();
        let main = Screen { object: 1 };
        let external = Screen { object: 2 };
        let main_number = ledger.number_of(&main).expect("a number");
        let external_number = ledger.number_of(&external).expect("a number");
        assert_ne!(main_number, external_number);
        assert_eq!(ledger.number_of(&main.clone()), Some(main_number));
        ledger.release_unlisted(std::slice::from_ref(&main));
        assert_eq!(ledger.number_of(&main), Some(main_number));
    }

    #[test]
    fn ios_released_screen_number_is_never_reissued() {
        let mut ledger = ScreenLedger::new();
        let first = Screen { object: 9 };
        let first_number = ledger.number_of(&first).expect("a number");
        ledger.release_unlisted(&[]);
        // Another screen now sits at the released one's address: object 9.
        let second = Screen { object: 9 };
        let second_number = ledger.number_of(&second).expect("a number");
        assert_ne!(first_number, second_number);
    }

    #[test]
    fn ios_screen_numbers_stop_at_the_counter_boundary() {
        let mut ledger = ScreenLedger::new();
        ledger.next = Some(u64::MAX);
        let last = Screen { object: 1 };
        let refused = Screen { object: 2 };
        assert_eq!(ledger.number_of(&last), Some(u64::MAX));
        assert_eq!(ledger.number_of(&refused), None);
        assert_eq!(ledger.number_of(&refused), None);
        // A screen already held still resolves after the counter is used up.
        assert_eq!(ledger.number_of(&last), Some(u64::MAX));
    }
}
