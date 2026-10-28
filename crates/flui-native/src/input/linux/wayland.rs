//! The Wayland seats' input: keyboards through XKB, pointers with their
//! axis frames, touchscreens, tablet tools (`tablet-v2`), a touchpad's pinch
//! (`pointer-gestures`) and the cursor each window shows (`cursor-shape`),
//! assembled through the shared pointer assembly.
//!
//! Two halves:
//!
//! - the pure ones, compiled and tested on every host: [`wire`], which turns
//!   what the seats' objects report, copied into plain wire events, into
//!   owned input through one [`ContactRegistry`](crate::input::assembly::ContactRegistry);
//!   [`repeat`], the client-side key repeat a Wayland compositor leaves to
//!   the client; and [`cursor`], a cursor's `cursor-shape` value;
//! - `dispatch`, the live one: `SeatInput`, the loop's input hook
//!   (`owner::linux::wayland::WaylandInput`), which binds the input globals
//!   as the compositor announces them, implements the dispatch of their
//!   objects on the owner's queue (recording, never delivering), and on each
//!   turn delivers what was recorded to the windows, with no lock held.
//!
//! # What the translation decides
//!
//! - **One device per object.** Every `wl_pointer`, `wl_touch`, tablet tool
//!   and pinch object gets a device label of its own, minted when it is
//!   made: a seat's mouse, touchscreen and pens are separate devices of known
//!   kind.
//! - **The compositor's millisecond stamp, when it is on our clock.** The
//!   protocol leaves the base of its stamps undefined; every compositor in
//!   use stamps on `CLOCK_MONOTONIC`. A stamp within a second of the
//!   monotonic clock read on arrival is placed on the event timeline through
//!   it (wrap included); any other, and an event that carries none, is
//!   stamped on arrival.
//! - **A pointer frame is one scroll.** Axis events are gathered until the
//!   frame: a wheel's high-resolution `value120` (or its discrete steps) is a
//!   step in detents, a finger's values are a phased precise scroll whose
//!   `axis_stop` ends it, a continuous source scrolls precisely without
//!   phases. Positions are surface-local logical coordinates: readings carry
//!   them with a scale of one.
//! - **A leave ends a press.** A compositor that takes the pointer while a
//!   button is held (a grab, a drag) sends no release: the mouse's sequence
//!   is cancelled at the leave, as is a finger scroll still open.
//! - **A touch frame is one pointer frame.** The touches a `wl_touch.frame`
//!   closes are assembled together, with the contact size each touch's
//!   `shape` gave in that frame.
//! - **A tablet frame is one reading per change.** A tool's axes and its tip
//!   arrive as separate events closed by a frame that carries the time: the
//!   frame becomes the tool's enter, its press or move, its buttons, its lift
//!   and its leave, in that order, with the axes it holds.
//! - **A pinch is one pan-zoom.** `pointer-gestures` reports a pinch's scale
//!   relative to its start and its rotation relative to the last update; the
//!   assembly takes each update's own factor and clockwise radians, about the
//!   cursor moved by the update's translation. Swipes and holds are the
//!   compositor's.

pub mod wire;

pub mod repeat {
    //! The key repeat a Wayland compositor leaves to the client
    //! (`wl_keyboard.repeat_info`): a held key that repeats does so after the
    //! delay, then at the rate, until it is released, another key is
    //! pressed, or the keyboard leaves the window.

    #![forbid(unsafe_code)]

    use std::time::Duration;

    use web_time::Instant;

    /// The held key's repeat.
    #[derive(Debug, Clone, Default)]
    pub struct KeyRepeat {
        /// The delay before the first repeat and the interval after it;
        /// `None` when the compositor turned repeat off.
        timing: Option<(Duration, Duration)>,
        /// The key held and when it next repeats.
        held: Option<(u32, Instant)>,
    }

    impl KeyRepeat {
        /// The compositor's settings: `rate` repeats a second after `delay`
        /// milliseconds. A rate of zero or less turns repeat off, and stops
        /// the key held.
        pub fn configure(&mut self, rate: i32, delay: i32) {
            let rate = u32::try_from(rate).ok().filter(|rate| *rate > 0);
            let delay = u64::try_from(delay).unwrap_or(0);
            self.timing =
                rate.map(|rate| (Duration::from_millis(delay), Duration::from_secs(1) / rate));
            if self.timing.is_none() {
                self.held = None;
            }
        }

        /// The key `code` went down at `now`: it repeats when `repeats` (the
        /// keymap says so) and repeat is on. Any other key held stops.
        pub fn press(&mut self, code: u32, repeats: bool, now: Instant) {
            self.held = match self.timing {
                Some((delay, _)) if repeats => Some((code, now + delay)),
                _ => None,
            };
        }

        /// The key `code` came up: it stops if it was the one repeating.
        pub fn release(&mut self, code: u32) {
            if self.held.is_some_and(|(held, _)| held == code) {
                self.held = None;
            }
        }

        /// Stop: the keyboard left the window, or its keymap changed.
        pub fn stop(&mut self) {
            self.held = None;
        }

        /// When the held key next repeats.
        #[must_use]
        pub fn due(&self) -> Option<Instant> {
            self.held.map(|(_, at)| at)
        }

        /// The key that repeats at `now`, if one is due. A loop that ran late
        /// repeats once, not once per interval it missed.
        pub fn take_due(&mut self, now: Instant) -> Option<u32> {
            let (_, interval) = self.timing?;
            let (code, at) = self.held.as_mut()?;
            if *at > now {
                return None;
            }
            *at += interval;
            if *at <= now {
                *at = now + interval;
            }
            Some(*code)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A held key repeats after the delay and then at the rate; a late
        /// loop repeats it once; its release, another key or repeat turned
        /// off stops it.
        #[test]
        fn a_held_key_repeats_after_its_delay_at_its_rate() {
            let start = Instant::now();
            let ms = Duration::from_millis;
            let mut repeat = KeyRepeat::default();
            repeat.configure(25, 600);
            repeat.press(30, true, start);
            assert_eq!(repeat.due(), Some(start + ms(600)));
            assert_eq!(repeat.take_due(start + ms(599)), None, "before the delay");
            assert_eq!(repeat.take_due(start + ms(600)), Some(30));
            assert_eq!(repeat.due(), Some(start + ms(640)), "then every 40 ms");
            assert_eq!(repeat.take_due(start + ms(900)), Some(30), "late");
            assert_eq!(repeat.due(), Some(start + ms(940)), "once, not seven times");
            repeat.release(31);
            assert!(repeat.due().is_some(), "another key's release");
            repeat.release(30);
            assert_eq!(repeat.due(), None, "its own release");

            repeat.press(30, false, start);
            assert_eq!(repeat.due(), None, "a key the keymap does not repeat");
            repeat.press(30, true, start);
            repeat.press(31, false, start);
            assert_eq!(repeat.due(), None, "another key pressed");
            repeat.press(30, true, start);
            repeat.configure(0, 600);
            assert_eq!(repeat.due(), None, "repeat turned off");
            repeat.press(30, true, start);
            assert_eq!(repeat.due(), None, "off until turned on");
        }
    }
}

pub mod cursor {
    //! The `cursor-shape` value of a cursor.

    #![forbid(unsafe_code)]

    use cursor_icon::CursorIcon;

    /// The `wp_cursor_shape_device_v1.shape` value that shows `icon` on a
    /// device of `version`: the protocol names the CSS cursors the icon
    /// names, in the order it lists them; version 1 lacks `dnd-ask` and
    /// `all-resize`, which show as `copy` and `move` there.
    #[must_use]
    pub fn shape(icon: CursorIcon, version: u32) -> u32 {
        match icon {
            CursorIcon::DndAsk if version >= 2 => 35,
            CursorIcon::AllResize if version >= 2 => 36,
            CursorIcon::ContextMenu => 2,
            CursorIcon::Help => 3,
            CursorIcon::Pointer => 4,
            CursorIcon::Progress => 5,
            CursorIcon::Wait => 6,
            CursorIcon::Cell => 7,
            CursorIcon::Crosshair => 8,
            CursorIcon::Text => 9,
            CursorIcon::VerticalText => 10,
            CursorIcon::Alias => 11,
            CursorIcon::Copy | CursorIcon::DndAsk => 12,
            CursorIcon::Move | CursorIcon::AllResize => 13,
            CursorIcon::NoDrop => 14,
            CursorIcon::NotAllowed => 15,
            CursorIcon::Grab => 16,
            CursorIcon::Grabbing => 17,
            CursorIcon::EResize => 18,
            CursorIcon::NResize => 19,
            CursorIcon::NeResize => 20,
            CursorIcon::NwResize => 21,
            CursorIcon::SResize => 22,
            CursorIcon::SeResize => 23,
            CursorIcon::SwResize => 24,
            CursorIcon::WResize => 25,
            CursorIcon::EwResize => 26,
            CursorIcon::NsResize => 27,
            CursorIcon::NeswResize => 28,
            CursorIcon::NwseResize => 29,
            CursorIcon::ColResize => 30,
            CursorIcon::RowResize => 31,
            CursorIcon::AllScroll => 32,
            CursorIcon::ZoomIn => 33,
            CursorIcon::ZoomOut => 34,
            // `Default`, and an icon the protocol has no name for yet.
            _ => 1,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Each cursor shows as the shape the protocol names it by, and the
        /// ones version 1 lacks as their nearest.
        #[test]
        fn a_cursor_shows_as_its_protocol_shape() {
            for (icon, version, expected) in [
                (CursorIcon::Default, 1, 1),
                (CursorIcon::Pointer, 1, 4),
                (CursorIcon::Text, 1, 9),
                (CursorIcon::NotAllowed, 1, 15),
                (CursorIcon::EwResize, 1, 26),
                (CursorIcon::ZoomOut, 1, 34),
                (CursorIcon::DndAsk, 2, 35),
                (CursorIcon::DndAsk, 1, 12),
                (CursorIcon::AllResize, 2, 36),
                (CursorIcon::AllResize, 1, 13),
            ] {
                assert_eq!(
                    shape(icon, version),
                    expected,
                    "{icon:?} on version {version}"
                );
            }
        }
    }
}

#[cfg(wayland_host)]
pub(crate) use dispatch::SeatInput;

#[cfg(wayland_host)]
mod dispatch;
