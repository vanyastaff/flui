//! The Apple displays: `NSScreen` on macOS (`macos`), `UIScreen` on iOS
//! (`ios`), and how the two sources name a display, which is what they share.
//!
//! macOS keys a screen by the UUID of the display behind it, and publishes its
//! frame in device pixels from the top-left of the primary screen. iOS has no
//! such identifier, so the source numbers each `UIScreen` it retains and
//! recognises the screen again only through the object it holds.
//!
//! The pure half, which every host compiles and tests, is how a screen is keyed
//! and placed ([`screen`]) and the iOS display link's frame-rate rules
//! (`link_rate`). A macOS mode's rate and a link's tick become a period by
//! the one constructor in `display::source`.

#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(any(target_os = "macos", target_os = "ios", test))]
#[forbid(unsafe_code)]
pub(crate) mod screen {
    //! How the Apple sources key a screen and where they place it: a macOS
    //! screen by its display's UUID, its frame in device pixels; an iOS screen
    //! by the number of the object a ledger retains.

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
}

#[cfg(any(target_os = "ios", test))]
#[forbid(unsafe_code)]
pub(crate) mod link_rate {
    //! The two pure rules behind iOS frame pacing: which frame-rate request a
    //! `CADisplayLink` gets on a given iOS version, and when the cadence the link
    //! measures becomes a new published refresh period, by the contract's
    //! constructor and the display model's one publisher.

    use flui_platform::{FrameRateHint, RefreshPeriod};

    use crate::display::source::MeasuredPeriod;
    use crate::version::gate::{Gate, Since, apple::AppleVersion};

    /// The floor of the requested range: an adaptive ProMotion panel may drop to
    /// 60 Hz when nothing moves, never lower while a frame source is running.
    const RANGE_FLOOR: u16 = 60;

    /// What a screen is assumed to sustain when it reports no maximum.
    const ASSUMED_MAXIMUM: u16 = 60;

    /// The highest rate a screen is believed to report; matches the largest
    /// cadence a [`RefreshPeriod`] describes.
    const MAXIMUM_CEILING: u16 = 1000;

    /// `CADisplayLink.preferredFrameRateRange`, which iOS 15 introduced.
    /// Without it the link gets a single `preferredFramesPerSecond`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct FrameRateRangeProperty;

    impl Gate for FrameRateRangeProperty {
        type Version = AppleVersion;
        const SINCE: u64 = AppleVersion::new(15, 0, 0).rank();
    }

    /// A frame-rate range in frames per second.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) struct FrameRateRange {
        pub(crate) minimum: f32,
        pub(crate) maximum: f32,
        pub(crate) preferred: f32,
    }

    /// How a `CADisplayLink` is told to run at the screen's best rate.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub(crate) enum FrameRatePreference {
        /// iOS 15 and later: a range, carrying the proof the property exists.
        Range(Since<FrameRateRangeProperty>, FrameRateRange),
        /// Before iOS 15: a single `preferredFramesPerSecond`.
        FramesPerSecond(u16),
    }

    impl FrameRateRange {
        /// No preference: the system picks the rate (`CAFrameRateRangeDefault`).
        pub(crate) const NO_PREFERENCE: Self = Self {
            minimum: 0.0,
            maximum: 0.0,
            preferred: 0.0,
        };
    }

    /// The request a link gets on `version` for the window's `hint`, given the
    /// screen's `maximumFramesPerSecond`.
    ///
    /// [`FrameRateHint::High`] runs the link at the screen's best rate: a maximum
    /// below one is a screen that reports nothing and reads as 60; one above 1000
    /// is held at 1000; the range's minimum is 60, or the maximum when that is
    /// lower, so the range is never inverted. Any other hint withdraws the
    /// request: the default range, or `preferredFramesPerSecond` zero before
    /// iOS 15, both of which leave the rate to the system.
    pub(crate) fn frame_rate_preference(
        version: AppleVersion,
        maximum_frames_per_second: i64,
        hint: FrameRateHint,
    ) -> FrameRatePreference {
        let wanted = match hint {
            FrameRateHint::High => Some(if maximum_frames_per_second < 1 {
                ASSUMED_MAXIMUM
            } else {
                u16::try_from(maximum_frames_per_second)
                    .unwrap_or(MAXIMUM_CEILING)
                    .min(MAXIMUM_CEILING)
            }),
            _ => None,
        };
        match version.since::<FrameRateRangeProperty>() {
            Some(proof) => FrameRatePreference::Range(
                proof,
                wanted.map_or(FrameRateRange::NO_PREFERENCE, |maximum| FrameRateRange {
                    minimum: f32::from(RANGE_FLOOR.min(maximum)),
                    maximum: f32::from(maximum),
                    preferred: f32::from(maximum),
                }),
            ),
            None => FrameRatePreference::FramesPerSecond(wanted.unwrap_or(0)),
        }
    }

    /// How far a measured period may sit from the published one, as a fraction of
    /// the published period, before it is published. An adaptive ProMotion panel
    /// jitters inside the band every tick; a real rate switch (60 to 120 Hz is a
    /// 50% step) is far outside it.
    const PUBLISH_BAND: f64 = 0.05;

    /// The refresh period a display link publishes, from the interval between its
    /// ticks.
    #[derive(Debug, Default)]
    pub(crate) struct LinkPeriod {
        published: MeasuredPeriod,
    }

    impl LinkPeriod {
        /// Take the interval one tick reports (`targetTimestamp - timestamp`, in
        /// seconds). Answers the new period when it leaves the band around the
        /// published one (the first valid interval always does), and `None` when
        /// nothing changes. An interval that describes no cadence (negative, not
        /// finite, outside 1 ms to 1 s) is ignored.
        pub(crate) fn observe(&mut self, interval_seconds: f64) -> Option<RefreshPeriod> {
            let measured = RefreshPeriod::from_secs_f64(interval_seconds).ok()?;
            self.published.offer(measured, PUBLISH_BAND)
        }

        /// Forget the published period: the link moved to another screen, or was
        /// rebuilt, and its cadence is unknown until the next tick.
        pub(crate) fn forget(&mut self) {
            self.published.forget();
        }
    }

    /// The periods display links published, by screen and the window whose link
    /// measured them. A screen can host several windows, each with its own link, so
    /// an entry belongs to its window: one window's link going away takes that
    /// window's entry and leaves the screen's other entries.
    #[derive(Debug)]
    pub(crate) struct ScreenPeriods<W> {
        /// `(screen number, window, period)` in the order windows first reported.
        entries: Vec<(u64, W, RefreshPeriod)>,
    }

    impl<W: Copy + PartialEq> ScreenPeriods<W> {
        pub(crate) fn new() -> Self {
            Self {
                entries: Vec::new(),
            }
        }

        /// Record the period `window`'s link measured on `screen`.
        pub(crate) fn report(&mut self, screen: u64, window: W, period: RefreshPeriod) {
            match self
                .entries
                .iter_mut()
                .find(|(held, owner, _)| *held == screen && *owner == window)
            {
                Some(entry) => entry.2 = period,
                None => self.entries.push((screen, window, period)),
            }
        }

        /// Drop `window`'s entry for `screen`; whether there was one.
        pub(crate) fn forget(&mut self, screen: u64, window: W) -> bool {
            let before = self.entries.len();
            self.entries
                .retain(|(held, owner, _)| !(*held == screen && *owner == window));
            self.entries.len() != before
        }

        /// The period of `screen`: that of the window that reported first among
        /// those still live, or `None` when no link on it is measuring.
        pub(crate) fn period_of(&self, screen: u64) -> Option<RefreshPeriod> {
            self.entries
                .iter()
                .find(|(held, _, _)| *held == screen)
                .map(|(_, _, period)| *period)
        }

        /// Keep only the entries of the screens in `live`.
        pub(crate) fn retain_screens(&mut self, live: &[u64]) {
            self.entries.retain(|(held, _, _)| live.contains(held));
        }
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::{FrameRateHint, RefreshPeriod};

        use super::{
            AppleVersion, FrameRatePreference, FrameRateRange, LinkPeriod, ScreenPeriods,
            frame_rate_preference,
        };

        fn period_hz(hz: f64) -> RefreshPeriod {
            RefreshPeriod::from_hz(hz).expect("BUG: a test rate describes a cadence")
        }

        fn range(preference: FrameRatePreference) -> Option<FrameRateRange> {
            match preference {
                FrameRatePreference::Range(_, range) => Some(range),
                FrameRatePreference::FramesPerSecond(_) => None,
            }
        }

        #[test]
        fn ios_14_uses_preferred_frames_per_second() {
            for (version, maximum, expected) in [
                (AppleVersion::new(13, 0, 0), 60, 60),
                (AppleVersion::new(14, 8, 1), 120, 120),
                (AppleVersion::new(14, 0, 0), 0, 60),
                (AppleVersion::new(14, 0, 0), -4, 60),
                (AppleVersion::new(14, 0, 0), 1_000_000, 1000),
            ] {
                assert_eq!(
                    frame_rate_preference(version, maximum, FrameRateHint::High),
                    FrameRatePreference::FramesPerSecond(expected),
                    "{version:?} max {maximum}"
                );
            }
        }

        #[test]
        fn ios_15_uses_the_frame_rate_range() {
            for (version, maximum, minimum, ceiling) in [
                (AppleVersion::new(15, 0, 0), 120, 60.0, 120.0),
                (AppleVersion::new(17, 4, 1), 60, 60.0, 60.0),
                (AppleVersion::new(18, 0, 0), 240, 60.0, 240.0),
                // A maximum below the floor never inverts the range.
                (AppleVersion::new(16, 0, 0), 30, 30.0, 30.0),
                // A screen reporting nothing reads as 60.
                (AppleVersion::new(16, 0, 0), 0, 60.0, 60.0),
            ] {
                let range = range(frame_rate_preference(version, maximum, FrameRateHint::High))
                    .unwrap_or_else(|| panic!("{version:?} has the range property"));
                assert_eq!(
                    range,
                    FrameRateRange {
                        minimum,
                        maximum: ceiling,
                        preferred: ceiling,
                    },
                    "{version:?} max {maximum}"
                );
            }
        }

        /// The window's hint is what the link asks for: `High` the screen's
        /// best rate, the default no preference at all, on either side of the
        /// range property's gate.
        #[test]
        fn the_link_asks_for_what_the_window_hints() {
            let ios_15 = AppleVersion::new(15, 0, 0);
            let ios_14 = AppleVersion::new(14, 0, 0);
            assert_eq!(
                range(frame_rate_preference(ios_15, 120, FrameRateHint::Default)),
                Some(FrameRateRange::NO_PREFERENCE)
            );
            assert_eq!(
                frame_rate_preference(ios_14, 120, FrameRateHint::Default),
                FrameRatePreference::FramesPerSecond(0)
            );
            assert_eq!(
                range(frame_rate_preference(ios_15, 120, FrameRateHint::High))
                    .map(|range| range.preferred),
                Some(120.0)
            );
            assert_eq!(
                frame_rate_preference(ios_14, 120, FrameRateHint::High),
                FrameRatePreference::FramesPerSecond(120)
            );
        }

        /// The display link's gate, the row of `version::gate`'s table for the
        /// marker declared here.
        #[test]
        fn frame_rate_range_gate_table() {
            let failures = crate::version::gate::testing::row::<super::FrameRateRangeProperty>(
                "FrameRateRangeProperty",
                AppleVersion::new(14, 99, 99),
                AppleVersion::new(15, 0, 0),
                AppleVersion::UNKNOWN,
            );
            assert!(failures.is_empty(), "{failures:#?}");
        }

        #[test]
        fn ios_period_follows_link_ticks() {
            let mut link = LinkPeriod::default();
            // The first valid tick publishes; before it nothing is known.
            assert_eq!(link.observe(1.0 / 60.0), Some(period_hz(60.0)));
            // A rate switch to 120 Hz and back publishes each step once.
            assert_eq!(link.observe(1.0 / 120.0), Some(period_hz(120.0)));
            assert_eq!(link.observe(1.0 / 120.0), None);
            assert_eq!(link.observe(1.0 / 60.0), Some(period_hz(60.0)));
            // Low power mode halves the rate.
            assert_eq!(link.observe(1.0 / 30.0), Some(period_hz(30.0)));
            // Intervals that describe no cadence change nothing.
            for interval in [-0.016, f64::NAN, f64::INFINITY, 0.0, 0.0001, 5.0] {
                assert_eq!(link.observe(interval), None, "{interval}");
            }
            assert_eq!(link.observe(1.0 / 30.0), None);
            // A link that was rebuilt starts again from unknown.
            link.forget();
            assert_eq!(link.observe(1.0 / 30.0), Some(period_hz(30.0)));
        }

        #[test]
        fn closing_one_window_keeps_the_screen_period_of_another() {
            let mut periods = ScreenPeriods::new();
            periods.report(1, 10u64, period_hz(120.0));
            periods.report(1, 11, period_hz(60.0));
            periods.report(2, 10, period_hz(60.0));
            // The first window's link goes away: the second still measures screen 1.
            assert!(periods.forget(1, 10));
            assert_eq!(periods.period_of(1), Some(period_hz(60.0)));
            // Screen 2 had only that window's entry for 10, which is untouched.
            assert_eq!(periods.period_of(2), Some(period_hz(60.0)));
            // Forgetting an entry that is not there changes nothing.
            assert!(!periods.forget(1, 10));
            // The last link on a screen takes the screen's period with it.
            assert!(periods.forget(1, 11));
            assert_eq!(periods.period_of(1), None);
            // A window re-reporting replaces its own entry.
            periods.report(2, 10, period_hz(120.0));
            assert_eq!(periods.period_of(2), Some(period_hz(120.0)));
            periods.retain_screens(&[]);
            assert_eq!(periods.period_of(2), None);
        }

        #[test]
        fn ios_adaptive_ticks_inside_the_band_publish_nothing() {
            let mut link = LinkPeriod::default();
            let base = 1.0 / 120.0;
            assert_eq!(link.observe(base), Some(period_hz(120.0)));
            // Jitter of up to 5% either way is silent and does not move the band:
            // drift is judged against the published value, not the last tick.
            for factor in [1.0, 1.02, 0.98, 1.049, 0.951, 1.049, 0.951, 1.0] {
                assert_eq!(link.observe(base * factor), None, "x{factor}");
            }
            // Just outside the band publishes the measured value.
            let outside = base * 1.06;
            let published = link.observe(outside).expect("outside the band publishes");
            assert!((published.get().as_secs_f64() - outside).abs() < 1e-9);
            // Slow drift never accumulates past the band without publishing.
            let mut seen = 0;
            for step in 0..40 {
                if link
                    .observe(outside * (1.0 + 0.01 * f64::from(step)))
                    .is_some()
                {
                    seen += 1;
                }
            }
            assert!(seen >= 1, "a long drift must publish eventually");
        }
    }
}
