//! The Apple displays: `NSScreen` on macOS ([`macos`]), `UIScreen` on iOS
//! ([`ios`]), and how the two sources name a display, which is what they share.
//!
//! macOS keys a screen by the UUID of the display behind it, and publishes its
//! frame in device pixels from the top-left of the primary screen. iOS has no
//! such identifier, so the source numbers each `UIScreen` it retains and
//! recognises the screen again only through the object it holds.
//!
//! The period a macOS mode's rate becomes ([`period`]) and the iOS display
//! link's frame-rate rules ([`link_rate`]) are pure, and every host runs their
//! tests.

#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(any(target_os = "ios", test))]
pub(crate) mod link_rate;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod period;

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
) -> Option<flui_platform::DisplayKey> {
    let display_id = u32::try_from(screen_number?).ok()?;
    uuid_of(display_id).map(flui_platform::DisplayKey::MacosUuid)
}

/// The text that names a macOS display across restarts: its UUID in the
/// canonical form (`8-4-4-4-12` uppercase hexadecimal digits), which is how
/// `CFUUIDCreateString` writes it.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_display_persistent_key(uuid: [u8; 16]) -> String {
    use std::fmt::Write as _;

    let mut key = String::with_capacity(36);
    for (index, byte) in uuid.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            key.push('-');
        }
        write!(key, "{byte:02X}").expect("BUG: writing to a String cannot fail");
    }
    key
}

/// A macOS screen's frame in the display snapshot's terms: device pixels of
/// that screen, measured from the top-left of the primary screen. AppKit
/// measures from the bottom-left corner of the primary screen, whose height is
/// `primary_height`, in points.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn device_bounds_of_screen(
    frame: flui_foundation::geometry::Bounds<f64>,
    primary_height: f64,
    scale: f64,
) -> flui_foundation::geometry::Bounds<i32> {
    use flui_foundation::geometry::{Bounds, Point, Size};

    let top = primary_height - (frame.origin.y + frame.size.height);
    let device = |points: f64| (points * scale).round() as i32;
    Bounds {
        origin: Point::new(device(frame.origin.x), device(top)),
        size: Size::new(device(frame.size.width), device(frame.size.height)),
    }
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

/// Set the screen identity a window names to `next`. Answers the previous
/// identity when it differed (the caller owes a display-changed notification
/// and the forgetting of the old screen's link period), `None` when nothing
/// changed.
///
/// A window that is detached from its scene names no screen: it moves to
/// `I::default()`, the absent identity.
#[cfg(any(target_os = "ios", test))]
pub(crate) fn change_screen<I: Copy + PartialEq>(slot: &mut I, next: I) -> Option<I> {
    let previous = std::mem::replace(slot, next);
    (previous != next).then_some(previous)
}

#[cfg(any(target_os = "ios", test))]
impl<S: Clone + SameObject> ScreenLedger<S> {
    pub(crate) fn new() -> Self {
        Self {
            retained: Vec::new(),
            next: Some(1),
        }
    }

    /// The number of `screen` when the ledger holds it; never numbers a new one.
    pub(crate) fn number_if_held(&self, screen: &S) -> Option<u64> {
        self.retained
            .iter()
            .find(|(held, _)| held.same_object(screen))
            .map(|(_, number)| *number)
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
    use flui_platform::{DisplayKey, DisplayRegistry};

    use flui_foundation::geometry::{Bounds, Point, Size};

    use super::{
        SameObject, ScreenLedger, change_screen, device_bounds_of_screen, macos_display_key,
        macos_display_persistent_key,
    };

    const UUID_A: [u8; 16] = [0xA0; 16];
    const UUID_B: [u8; 16] = [0xB0; 16];

    #[test]
    fn a_screen_is_published_in_device_pixels_from_the_top_left() {
        const PRIMARY: f64 = 900.0;
        let frame = |x, y, width, height| Bounds {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        };
        // (name, cocoa frame, scale, device bounds as (x, y, width, height))
        let rows = [
            (
                "the primary screen at 2x",
                frame(0.0, 0.0, 1440.0, 900.0),
                2.0,
                (0, 0, 2880, 1800),
            ),
            (
                "a 1x screen above the primary",
                frame(0.0, 900.0, 1920.0, 1080.0),
                1.0,
                (0, -1080, 1920, 1080),
            ),
            (
                "a 2x screen right of the primary",
                frame(1440.0, 0.0, 1280.0, 720.0),
                2.0,
                (2880, 360, 2560, 1440),
            ),
        ];
        for (name, input, scale, (x, y, width, height)) in rows {
            let bounds = device_bounds_of_screen(input, PRIMARY, scale);
            assert_eq!(
                (
                    bounds.origin.x,
                    bounds.origin.y,
                    bounds.size.width,
                    bounds.size.height
                ),
                (x, y, width, height),
                "{name}"
            );
        }
    }

    #[test]
    fn a_display_uuid_is_written_in_the_canonical_form() {
        let uuid = [
            0x37, 0xD8, 0x83, 0x2A, 0x2D, 0x66, 0x02, 0xCA, 0x4A, 0x8F, 0x03, 0x0B, 0x15, 0x9C,
            0xF2, 0x11,
        ];
        assert_eq!(
            macos_display_persistent_key(uuid),
            "37D8832A-2D66-02CA-4A8F-030B159CF211"
        );
        assert_ne!(
            macos_display_persistent_key(UUID_A),
            macos_display_persistent_key(UUID_B),
            "two monitors have two keys"
        );
    }

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
    fn a_detached_window_names_no_screen() {
        let mut slot = 7u32;
        // Detaching a window that named a screen clears it and owes a report.
        assert_eq!(change_screen(&mut slot, u32::default()), Some(7));
        assert_eq!(slot, 0);
        // Detaching again changes nothing: no second notification.
        assert_eq!(change_screen(&mut slot, u32::default()), None);
        // Re-adopting the same identity is silent; a new one reports the old.
        assert_eq!(change_screen(&mut slot, 3), Some(0));
        assert_eq!(change_screen(&mut slot, 3), None);
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
        assert_eq!(ledger.number_if_held(&main), Some(main_number));
        assert_eq!(ledger.number_if_held(&Screen { object: 3 }), None);
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
