//! The Apple displays: `NSScreen` on macOS (`macos`), `UIScreen` on iOS
//! (`ios`), and how the two sources name a display, which is what they share.
//!
//! macOS keys a screen by the UUID of the display behind it, and publishes its
//! frame in device pixels from the top-left of the primary screen. iOS has no
//! such identifier, so the source numbers each `UIScreen` it retains and
//! recognises the screen again only through the object it holds.
//!
//! The pure half, which every host compiles and tests, is how a screen is keyed
//! and placed ([`screen`]), the period a macOS screen paces at
//! (`screen_cadence`) and the iOS display link's frame-rate rules
//! (`link_rate`). A macOS mode's rate and a link's tick become a period by
//! the contract's constructors.

#[cfg(target_os = "ios")]
pub(crate) mod ios;
#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(any(target_os = "macos", target_os = "ios", test))]
#[forbid(unsafe_code)]
pub(crate) mod screen {
    //! How the Apple sources key a screen and where they place it: a macOS
    //! screen by its display's UUID, its frame in device pixels; an iOS screen
    //! by the number of the object a ledger retains; and the colour a screen
    //! shows surfaces in.

    use flui_platform::{DisplayColor, Headroom};

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

    /// A macOS screen in the display snapshot's terms: where its top-left
    /// corner sits in AppKit's desktop, in points from the top-left of the
    /// primary screen (the desktop AppKit lays out, flipped), and its extent
    /// and usable area (`visible`) in its own logical pixels, which are
    /// points. AppKit measures from the bottom-left corner of the primary
    /// screen, whose height is `primary_height`, in points.
    ///
    /// `None` for a frame with no area or numbers that are not finite. A
    /// visible frame that reaches past the frame leaves the screen usable as
    /// a whole.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn screen_bounds(
        frame: flui_foundation::geometry::Bounds<f64>,
        visible: flui_foundation::geometry::Bounds<f64>,
        primary_height: f64,
    ) -> Option<(flui_platform::DesktopOrigin, flui_platform::DisplayBounds)> {
        use flui_foundation::geometry::Rect;
        use flui_platform::{DesktopOrigin, DisplayBounds};

        let top_of = |rect: flui_foundation::geometry::Bounds<f64>| {
            primary_height - (rect.origin.y + rect.size.height)
        };
        let (left, top) = (frame.origin.x, top_of(frame));
        let whole = DisplayBounds::whole(frame.size).ok()?;
        let usable = DisplayBounds::try_new(
            frame.size,
            Rect::from_ltwh(
                visible.origin.x - left,
                top_of(visible) - top,
                visible.size.width,
                visible.size.height,
            ),
        )
        .unwrap_or(whole);
        // AppKit lays screens out at whole points; one past the `i32` range
        // is no screen.
        let point = |value: f64| {
            let value = value.round();
            (value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX)).then(|| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a whole number checked inside the i32 range just above"
                )]
                let value = value as i32;
                value
            })
        };
        Some((DesktopOrigin::new(point(left)?, point(top)?), usable))
    }

    /// `-[NSScreen maximumPotentialExtendedDynamicRangeColorComponentValue]`
    /// (macOS 10.15). Before it a screen reports no extended-range headroom.
    ///
    /// Only the x86_64 floor (10.13) lies below it; arm64's floor (11.0) is
    /// above it, so there the call is made directly and a gate would not
    /// compile (ADR-0244).
    #[cfg(all(any(target_os = "macos", test), not(target_arch = "aarch64")))]
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct MacosPotentialHeadroom;

    #[cfg(all(any(target_os = "macos", test), not(target_arch = "aarch64")))]
    impl crate::version::gate::Gate for MacosPotentialHeadroom {
        type Version = crate::version::gate::apple::MacOSVersion;
        const SINCE: u64 = crate::version::gate::apple::MacOSVersion::new(10, 15, 0).rank();
    }

    /// `UIScreen.potentialEDRHeadroom` (iOS 16). Before it a screen reports no
    /// extended-range headroom.
    #[cfg(any(target_os = "ios", test))]
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct IosPotentialHeadroom;

    #[cfg(any(target_os = "ios", test))]
    impl crate::version::gate::Gate for IosPotentialHeadroom {
        type Version = crate::version::gate::apple::AppleVersion;
        const SINCE: u64 = crate::version::gate::apple::AppleVersion::new(16, 0, 0).rank();
    }

    /// The colour of an Apple screen: whether it shows the Display P3 gamut,
    /// and the extended-dynamic-range headroom it can reach
    /// (`maximumPotentialExtendedDynamicRangeColorComponentValue` on macOS,
    /// `potentialEDRHeadroom` on iOS), `None` where the system predates it.
    ///
    /// A screen whose potential headroom is over white is in high range at that
    /// headroom. The potential, not the current value, because the current one
    /// stays at white until a layer asks for extended range: reading it would
    /// keep the surface that asks from ever being chosen. A screen without the
    /// report, or with a potential at white or one that is not a finite multiple
    /// of it, is standard range: no extended range is published from a value
    /// that does not describe one.
    pub(crate) fn screen_color(p3: bool, potential: Option<f64>) -> DisplayColor {
        let gamut = crate::display::source::p3_or_srgb(p3);
        // A finite `f64` beyond `f32` becomes infinite here, which `try_new` refuses.
        match potential.and_then(|value| Headroom::try_new(value as f32).ok()) {
            Some(headroom) if headroom > Headroom::SDR => DisplayColor::High(gamut, Ok(headroom)),
            _ => DisplayColor::Standard(gamut),
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

        #[cfg(not(target_arch = "aarch64"))]
        use super::MacosPotentialHeadroom;
        use super::{
            IosPotentialHeadroom, SameObject, ScreenLedger, change_screen, macos_display_key,
            macos_display_persistent_key, screen_bounds, screen_color,
        };

        /// Each screen report Apple gives, to the display colour it means.
        #[test]
        fn apple_screen_reports_map_to_the_display_colour() {
            use flui_platform::{ColorGamut, DisplayColor, Headroom};

            assert_eq!(
                screen_color(true, Some(16.0)),
                DisplayColor::High(
                    ColorGamut::DisplayP3,
                    Ok(Headroom::try_new(16.0).expect("headroom"))
                ),
                "an XDR panel"
            );
            assert_eq!(
                screen_color(false, Some(1.0)),
                DisplayColor::Standard(ColorGamut::Srgb),
                "an SDR monitor"
            );
            assert_eq!(
                screen_color(true, None),
                DisplayColor::Standard(ColorGamut::DisplayP3),
                "a system without the headroom report"
            );
            for broken in [f64::NAN, f64::INFINITY, 0.0, -3.0, f64::MAX] {
                assert_eq!(
                    screen_color(false, Some(broken)),
                    DisplayColor::Standard(ColorGamut::Srgb),
                    "{broken} publishes no extended range"
                );
            }
        }

        /// The headroom reports' gates: none before the release that has them.
        #[test]
        fn potential_headroom_gate_table() {
            use crate::version::gate::apple::AppleVersion;

            #[cfg(not(target_arch = "aarch64"))]
            {
                use crate::version::gate::apple::MacOSVersion;

                assert!(
                    MacOSVersion::new(10, 14, 6)
                        .since::<MacosPotentialHeadroom>()
                        .is_none()
                );
                assert!(
                    MacOSVersion::new(10, 15, 0)
                        .since::<MacosPotentialHeadroom>()
                        .is_some()
                );
            }
            assert!(
                AppleVersion::new(15, 7, 0)
                    .since::<IosPotentialHeadroom>()
                    .is_none()
            );
            assert!(
                AppleVersion::new(16, 0, 0)
                    .since::<IosPotentialHeadroom>()
                    .is_some()
            );
        }

        const UUID_A: [u8; 16] = [0xA0; 16];
        const UUID_B: [u8; 16] = [0xB0; 16];

        #[test]
        fn a_screen_is_published_in_its_own_points_from_the_top_left() {
            const PRIMARY: f64 = 900.0;
            let frame = |x, y, width, height| Bounds {
                origin: Point::new(x, y),
                size: Size::new(width, height),
            };
            // (name, cocoa frame, cocoa visible frame, origin in flipped
            // points, extent, usable area as left, top, right, bottom)
            let rows = [
                (
                    "the primary screen, a menu bar and a dock",
                    frame(0.0, 0.0, 1440.0, 900.0),
                    frame(0.0, 70.0, 1440.0, 805.0),
                    (0, 0),
                    (1440.0, 900.0),
                    (0.0, 25.0, 1440.0, 830.0),
                ),
                (
                    "a screen above the primary",
                    frame(0.0, 900.0, 1920.0, 1080.0),
                    frame(0.0, 900.0, 1920.0, 1080.0),
                    (0, -1080),
                    (1920.0, 1080.0),
                    (0.0, 0.0, 1920.0, 1080.0),
                ),
                (
                    "a screen right of the primary, lower",
                    frame(1440.0, 0.0, 1280.0, 720.0),
                    frame(1440.0, 0.0, 1280.0, 720.0),
                    (1440, 180),
                    (1280.0, 720.0),
                    (0.0, 0.0, 1280.0, 720.0),
                ),
                (
                    "a visible frame past the screen leaves it whole",
                    frame(0.0, 0.0, 1440.0, 900.0),
                    frame(-10.0, 0.0, 1440.0, 900.0),
                    (0, 0),
                    (1440.0, 900.0),
                    (0.0, 0.0, 1440.0, 900.0),
                ),
            ];
            for (name, screen, visible, origin, extent, usable) in rows {
                let (placed, bounds) =
                    screen_bounds(screen, visible, PRIMARY).expect("a screen with an area");
                let area = bounds.usable();
                assert_eq!(
                    (
                        (placed.get().x, placed.get().y),
                        (bounds.extent().width, bounds.extent().height),
                        (area.left(), area.top(), area.right(), area.bottom())
                    ),
                    (origin, extent, usable),
                    "{name}"
                );
            }
            assert_eq!(
                screen_bounds(
                    frame(0.0, 0.0, 0.0, 900.0),
                    frame(0.0, 0.0, 0.0, 900.0),
                    PRIMARY
                ),
                None,
                "a screen with no area"
            );
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

#[cfg(any(target_os = "macos", test))]
#[forbid(unsafe_code)]
pub(crate) mod screen_cadence {
    //! The refresh period a macOS screen paces at.
    //!
    //! Three reports decide it. The rate of the display's current mode
    //! (`CGDisplayModeGetRefreshRate`) is the period where it names a
    //! cadence. CoreGraphics reports 0 Hz for a mode it cannot describe,
    //! which some built-in panels are in; the nominal period of a display
    //! link on that display
    //! (`CVDisplayLinkGetNominalOutputVideoRefreshPeriod`) stands in for it.
    //! And under Low Power Mode a built-in panel refreshes no faster than
    //! 60 Hz while its mode and its display link still say 120, so its period
    //! is held at 1/60 s or longer: frames produced at the mode's rate would
    //! only queue behind the panel.

    use flui_platform::RefreshPeriod;

    use crate::version::gate::{Gate, apple::MacOSVersion};

    /// The rate Low Power Mode holds a built-in panel to, in hertz.
    const LOW_POWER_HERTZ: f64 = 60.0;

    /// Whether a display is the machine's own panel (`CGDisplayIsBuiltin`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Panel {
        /// The panel built into the machine.
        BuiltIn,
        /// A monitor attached to it.
        External,
    }

    /// Whether the system is saving power
    /// (`NSProcessInfo.isLowPowerModeEnabled`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum PowerMode {
        /// Low Power Mode is off, or the system has none.
        Normal,
        /// Low Power Mode is on.
        LowPower,
    }

    /// `NSProcessInfo.isLowPowerModeEnabled` and
    /// `NSProcessInfoPowerStateDidChangeNotification` (macOS 12). Before them
    /// the system has no Low Power Mode, and a screen paces at what its mode
    /// says.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct ProcessPowerState;

    impl Gate for ProcessPowerState {
        type Version = MacOSVersion;
        const SINCE: u64 = MacOSVersion::new(12, 0, 0).rank();
    }

    /// A `CVTime`, laid out as CoreVideo returns it: `time_value / time_scale`
    /// seconds.
    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct CvTime {
        pub(crate) time_value: i64,
        pub(crate) time_scale: i32,
        pub(crate) flags: i32,
    }

    impl CvTime {
        /// `kCVTimeIsIndefinite`: CoreVideo does not know the time.
        const INDEFINITE: i32 = 1;

        /// The refresh period this time is, read as the time between two
        /// refreshes; `None` for an indefinite time, a value or scale that is
        /// zero or negative, and a period no [`RefreshPeriod`] holds.
        fn refresh_period(self) -> Option<RefreshPeriod> {
            if self.flags & Self::INDEFINITE != 0 {
                return None;
            }
            // `time_value / time_scale` seconds between refreshes is
            // `time_scale / time_value` hertz, which the contract's rational
            // constructor divides in integers (1001/60000 s stays exact).
            RefreshPeriod::from_ratio(
                u32::try_from(self.time_scale).ok()?,
                u32::try_from(self.time_value).ok()?,
            )
            .ok()
        }
    }

    /// The refresh period of a screen on a `panel` whose current mode reports
    /// `mode_hertz` (`None` for a display with no mode to read), under
    /// `power`.
    ///
    /// The mode's rate where it names a cadence; otherwise `link_nominal`,
    /// the nominal period of a display link on the display, which is asked
    /// only then; `None` when neither names one. A built-in panel under Low
    /// Power Mode has the period of 60 Hz when its own is shorter, and keeps
    /// a longer one.
    pub(crate) fn refresh_period(
        mode_hertz: Option<f64>,
        link_nominal: impl FnOnce() -> Option<CvTime>,
        panel: Panel,
        power: PowerMode,
    ) -> Option<RefreshPeriod> {
        let reported = mode_hertz
            .and_then(|hertz| RefreshPeriod::from_hz(hertz).ok())
            .or_else(|| link_nominal()?.refresh_period())?;
        Some(match (panel, power) {
            (Panel::BuiltIn, PowerMode::LowPower) => {
                // By the constructor a mode's rate takes, so a panel whose
                // mode says 60 Hz keeps its period to the nanosecond.
                let low_power_period = RefreshPeriod::from_hz(LOW_POWER_HERTZ)
                    .expect("BUG: 60 Hz is a cadence a RefreshPeriod holds");
                reported.max(low_power_period)
            }
            (Panel::BuiltIn, PowerMode::Normal) | (Panel::External, _) => reported,
        })
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::RefreshPeriod;

        use super::{CvTime, Panel, PowerMode, ProcessPowerState, refresh_period};
        use crate::version::gate::apple::MacOSVersion;

        fn hertz(rate: f64) -> RefreshPeriod {
            RefreshPeriod::from_hz(rate).expect("BUG: a test rate describes a cadence")
        }

        const fn link(time_value: i64, time_scale: i32) -> CvTime {
            CvTime {
                time_value,
                time_scale,
                flags: 0,
            }
        }

        /// Each row: the mode's rate, the display link's nominal period, the
        /// panel, the power mode, and the period the screen paces at.
        #[test]
        fn a_macos_screen_paces_at_its_mode_its_link_or_the_low_power_rate() {
            use Panel::{BuiltIn, External};
            use PowerMode::{LowPower, Normal};

            let rows = [
                (
                    "a ProMotion panel",
                    Some(120.0),
                    None,
                    BuiltIn,
                    Normal,
                    Some(hertz(120.0)),
                ),
                (
                    "a ProMotion panel under Low Power Mode is held at 60 Hz",
                    Some(120.0),
                    None,
                    BuiltIn,
                    LowPower,
                    Some(hertz(60.0)),
                ),
                (
                    "an external monitor is not held under Low Power Mode",
                    Some(120.0),
                    None,
                    External,
                    LowPower,
                    Some(hertz(120.0)),
                ),
                (
                    "a 60 Hz panel under Low Power Mode keeps its period",
                    Some(60.0),
                    None,
                    BuiltIn,
                    LowPower,
                    Some(hertz(60.0)),
                ),
                (
                    "a panel slower than 60 Hz under Low Power Mode keeps its period",
                    Some(48.0),
                    None,
                    BuiltIn,
                    LowPower,
                    Some(hertz(48.0)),
                ),
                (
                    "a mode that reports 0 Hz takes the link's nominal period",
                    Some(0.0),
                    Some(link(1001, 60_000)),
                    External,
                    Normal,
                    RefreshPeriod::from_ratio(60_000, 1001).ok(),
                ),
                (
                    "a display with no mode takes the link's nominal period",
                    None,
                    Some(link(1, 100)),
                    External,
                    Normal,
                    RefreshPeriod::from_ratio(100, 1).ok(),
                ),
                (
                    "a 0 Hz built-in panel under Low Power Mode is held at 60 Hz",
                    Some(0.0),
                    Some(link(1, 120)),
                    BuiltIn,
                    LowPower,
                    Some(hertz(60.0)),
                ),
                (
                    "a mode that reports 0 Hz and no link has no period",
                    Some(0.0),
                    None,
                    BuiltIn,
                    Normal,
                    None,
                ),
                (
                    "no period is not held at 60 Hz either",
                    Some(0.0),
                    None,
                    BuiltIn,
                    LowPower,
                    None,
                ),
                (
                    "an indefinite nominal period is none",
                    Some(0.0),
                    Some(CvTime {
                        time_value: 1,
                        time_scale: 60,
                        flags: 1,
                    }),
                    External,
                    Normal,
                    None,
                ),
                (
                    "a nominal period with no scale is none",
                    Some(0.0),
                    Some(link(1, 0)),
                    External,
                    Normal,
                    None,
                ),
                (
                    "a nominal period of zero is none",
                    Some(0.0),
                    Some(link(0, 60)),
                    External,
                    Normal,
                    None,
                ),
                (
                    "a negative nominal period is none",
                    Some(0.0),
                    Some(link(-1, 60)),
                    External,
                    Normal,
                    None,
                ),
                (
                    "a nominal period longer than a second is none",
                    Some(0.0),
                    Some(link(i64::MAX, 1)),
                    External,
                    Normal,
                    None,
                ),
                (
                    "a rate that is no number takes the link's nominal period",
                    Some(f64::NAN),
                    Some(link(1, 60)),
                    External,
                    Normal,
                    RefreshPeriod::from_ratio(60, 1).ok(),
                ),
            ];
            for (case, mode_hertz, nominal, panel, power, expected) in rows {
                assert_eq!(
                    refresh_period(mode_hertz, || nominal, panel, power),
                    expected,
                    "{case}"
                );
            }
        }

        /// A display link is made only for a mode that names no cadence.
        #[test]
        fn a_mode_with_a_rate_asks_no_display_link() {
            let mut asked = false;
            let period = refresh_period(
                Some(120.0),
                || {
                    asked = true;
                    Some(link(1, 60))
                },
                Panel::External,
                PowerMode::Normal,
            );
            assert_eq!(period, Some(hertz(120.0)));
            assert!(!asked, "the mode named its rate");
        }

        /// What the power-state notification does to the display source
        /// (mark it stale) reaches a consumer as a new revision when Low
        /// Power Mode changed a period, and as none when it changed nothing;
        /// a source nothing marked stale is not read again.
        #[test]
        fn a_power_mode_change_is_a_new_display_revision_only_where_the_period_changed() {
            use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size};
            use flui_platform::{DisplayDescription, DisplayKey};

            use crate::display::source::DisplaySource;

            for (panel, period_under_low_power, changes) in [
                (Panel::BuiltIn, hertz(60.0), true),
                (Panel::External, hertz(120.0), false),
            ] {
                let source = DisplaySource::new();
                let id = source
                    .mint(&DisplayKey::MacosUuid([7; 16]))
                    .expect("ids are not exhausted");
                let read = |power| {
                    source
                        .snapshot_with(|| {
                            let extent = Bounds::new(Point::new(0, 0), Size::new(3024, 1964));
                            let mut display = DisplayDescription::from_device(
                                id,
                                "panel",
                                extent,
                                extent,
                                DevicePixelRatio::try_new(2.0).expect("a positive ratio"),
                            )
                            .expect("a display with an area");
                            display.refresh_period =
                                refresh_period(Some(120.0), || None, panel, power);
                            Ok(vec![display])
                        })
                        .expect("revisions are not exhausted")
                };
                let period_of = |snapshot: &flui_platform::DisplaySnapshot| {
                    snapshot.get(id).and_then(|display| display.refresh_period)
                };
                let normal = read(PowerMode::Normal);
                assert_eq!(period_of(&normal), Some(hertz(120.0)), "{panel:?}");
                let unmarked = read(PowerMode::LowPower);
                assert_eq!(
                    unmarked.revision(),
                    normal.revision(),
                    "{panel:?}: nothing marked the source stale"
                );
                source.invalidate();
                let low_power = read(PowerMode::LowPower);
                assert_eq!(
                    period_of(&low_power),
                    Some(period_under_low_power),
                    "{panel:?}"
                );
                assert_eq!(
                    low_power.revision() != normal.revision(),
                    changes,
                    "{panel:?}: a revision for a changed period, none for the same one"
                );
            }
        }

        #[test]
        fn process_power_state_gate_table() {
            let failures = crate::version::gate::testing::row::<ProcessPowerState>(
                "ProcessPowerState",
                MacOSVersion::new(11, 99, 99),
                MacOSVersion::new(12, 0, 0),
                MacOSVersion::UNKNOWN,
            );
            assert!(failures.is_empty(), "{failures:#?}");
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
