//! The Android lane's safety rules, as pure functions over what the lane read of the device, each
//! refusal with its wire code ([`Refusal::into_error`]).
//!
//! The lane refuses whatever leaves the app or changes the device past the run:
//!
//! - **keys** that do (`keys::SYSTEM_KEYS`: the launcher, recent apps, back, power, volume,
//!   brightness, the shade, settings, media), and a lone tap of Meta, which opens the app drawer;
//! - **chords** the window manager takes before the app ([`system_chord`]): any with Meta, Alt and
//!   Tab, Control or Meta and Space;
//! - **gestures**: a contact put down within [`GESTURE_EDGE_DP`] of an edge of the display
//!   ([`gesture_edge`]), where the system reads a swipe as its own (the shade and quick settings
//!   from the top, home and recents from the bottom, back from the sides). The rule reads where
//!   the contact goes down, since that is where the system decides.
//!
//! Before every press, key and text the app's window must hold the input focus
//! ([`check_focus`]): a press with the shade, the lock screen or another app in front lands
//! there.

use flui_protocol::error::{ErrorCode, HandleKind};
use flui_protocol::session::Isolation;

use super::LANE;
use super::keys::{self, Keycode};
use crate::coordinates::DevicePoint;
use crate::error::Error;

/// How far from an edge of the display, in density-independent pixels, a contact going down is
/// taken for the system's gesture: AOSP's `navigation_bar_height`, held at every edge. A device
/// whose bars or back-gesture inset are wider is for a run on it to find.
pub const GESTURE_EDGE_DP: u32 = 48;

/// Why the lane refuses an event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// Another window has the input focus.
    NotForeground {
        /// The window that has it, for people; `None` when none does.
        focus: Option<String>,
    },
    /// The app's window is not among what the device shows.
    WindowGone,
    /// A key that leaves the app or changes the device.
    SystemKey {
        /// The key, as named.
        name: String,
        /// What it does instead.
        effect: &'static str,
    },
    /// A chord the window manager takes.
    SystemChord {
        /// The chord, for people.
        chord: String,
        /// What takes it.
        effect: &'static str,
    },
    /// A contact going down where the system reads a gesture.
    SystemGesture {
        /// Where, in display pixels.
        point: DevicePoint,
        /// The gesture of that edge.
        gesture: &'static str,
    },
    /// A key name the lane has no key code for.
    UnknownKey {
        /// The key, as named.
        name: String,
    },
    /// A character the device's virtual keyboard cannot type.
    UnsentText {
        /// The character.
        character: char,
    },
    /// A device, button or combination the lane does not send.
    UnsentInput {
        /// What it is.
        what: String,
    },
    /// Input only a `uinput` device sends, on a device whose `uinput` the probe found closed.
    NeedsUinput {
        /// What it is.
        what: String,
    },
    /// A command the device's `input` does not have.
    NeedsCommand {
        /// The command.
        command: &'static str,
        /// What needed it.
        what: String,
    },
    /// An isolation mode that is no Android device's.
    ForeignIsolation {
        /// The mode.
        isolation: Isolation,
    },
}

/// The capability a device without root `uinput` lacks, which its pen and touchpad rows name.
pub const UINPUT_DEVICE: &str = "uinput_device";

impl Refusal {
    /// The error the refusal answers with.
    #[must_use]
    pub fn into_error(self) -> Error {
        match self {
            Self::NotForeground { focus } => Error::NotForeground {
                message: format!(
                    "the app's window does not hold the input focus ({}); nothing was sent",
                    focus.as_deref().unwrap_or("no window holds it")
                ),
                retry: ErrorCode::NotForeground.default_retry(),
            },
            Self::WindowGone => Error::gone(
                HandleKind::Window,
                None,
                None,
                "the device does not show the app's window",
            ),
            Self::SystemKey { name, effect } => Error::not_supported(
                "os_shell_hotkey",
                format!(
                    "{name} {effect}, which leaves the app or changes the device; it is refused"
                ),
            ),
            Self::SystemChord { chord, effect } => Error::not_supported(
                "os_shell_hotkey",
                format!(
                    "{effect} takes {chord} before the app sees it, so no focus check holds for \
                     it; it is refused"
                ),
            ),
            Self::SystemGesture { point, gesture } => Error::not_supported(
                "os_system_gesture",
                format!(
                    "a contact going down at ({}, {}) is {gesture}; the lane puts nothing down \
                     within {GESTURE_EDGE_DP} dp of the display's edge",
                    point.x, point.y
                ),
            ),
            Self::UnknownKey { name } => Error::not_supported(
                "os_key",
                format!(
                    "the {LANE} lane has no key named {name:?}; a shifted character is typed as \
                     text"
                ),
            ),
            Self::UnsentText { character } => Error::not_supported(
                "os_text",
                format!(
                    "`input text` types through the virtual keyboard's character map, which has \
                     no {character:?}; only printable ASCII, line breaks and tabs are typed"
                ),
            ),
            Self::UnsentInput { what } => Error::not_supported(
                "os_input_device",
                format!("the {LANE} lane does not send {what}"),
            ),
            Self::NeedsUinput { what } => Error::not_supported(
                UINPUT_DEVICE,
                format!(
                    "{what} needs a `uinput` device, and this device does not let the shell \
                     register one"
                ),
            ),
            Self::NeedsCommand { command, what } => Error::not_supported(
                format!("android_input_{command}"),
                format!(
                    "{what} needs `input {command}`, which this device's `input` does not have"
                ),
            ),
            Self::ForeignIsolation { isolation } => Error::invalid_argument(format!(
                "{} is no isolation mode of an Android device: it is an emulator or a device",
                isolation.name()
            )),
        }
    }
}

/// Whether the lane may run in `isolation`: an emulator, isolated by construction, or a device,
/// whose input is guarded.
///
/// # Errors
///
/// [`Refusal::ForeignIsolation`] for any other mode.
pub fn admit_isolation(isolation: Isolation) -> Result<(), Refusal> {
    match isolation {
        Isolation::Emulator | Isolation::Device => Ok(()),
        _ => Err(Refusal::ForeignIsolation { isolation }),
    }
}

/// What takes the chord of `held` modifiers and `key`, when the window manager does.
#[must_use]
pub fn system_chord(held: &[Keycode], key: Keycode) -> Option<&'static str> {
    let holds = |modifier: Keycode| held.contains(&modifier);
    if holds(keys::META) {
        return Some("Android's window manager, whose system shortcuts all hold Meta,");
    }
    if holds(keys::ALT) && key == keys::TAB {
        return Some("the app switcher");
    }
    if holds(keys::CONTROL) && key == keys::SPACE {
        return Some("the keyboard layout switch");
    }
    None
}

/// The chord of `held` and `key`, for people.
#[must_use]
pub fn chord_name(held: &[Keycode], key: Keycode) -> String {
    held.iter()
        .chain(std::iter::once(&key))
        .map(|code| code.name.trim_start_matches("KEYCODE_"))
        .collect::<Vec<_>>()
        .join("+")
}

/// The display, as the lane maps a contact against its edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Display {
    /// Width in pixels, as the display is turned now.
    pub width: i32,
    /// Height in pixels, as the display is turned now.
    pub height: i32,
    /// Its density: pixels per 160th of an inch.
    pub density: u32,
}

impl Display {
    /// [`GESTURE_EDGE_DP`] in this display's pixels, rounded up.
    #[must_use]
    pub fn gesture_edge(&self) -> i32 {
        let pixels = (u64::from(GESTURE_EDGE_DP) * u64::from(self.density)).div_ceil(160);
        i32::try_from(pixels).unwrap_or(i32::MAX)
    }
}

/// The system gesture a contact going down at `point` starts, when it starts one.
#[must_use]
pub fn gesture_edge(point: DevicePoint, display: &Display) -> Option<&'static str> {
    let edge = display.gesture_edge();
    if point.y < edge {
        Some("the swipe that opens the notification shade and quick settings")
    } else if point.y >= display.height.saturating_sub(edge) {
        Some("the swipe to home and the recent apps")
    } else if point.x < edge || point.x >= display.width.saturating_sub(edge) {
        Some("the back gesture")
    } else {
        None
    }
}

/// Whether the window `focus` names (a `dumpsys window` focus line's window, `None` when no
/// window holds the focus) is the app `package`'s.
///
/// # Errors
///
/// [`Refusal::NotForeground`] naming the window that holds it.
pub fn check_focus(focus: Option<&str>, package: &str) -> Result<(), Refusal> {
    let owner = focus.and_then(|window| window.split('/').next());
    if owner == Some(package) {
        return Ok(());
    }
    Err(Refusal::NotForeground {
        focus: focus.map(str::to_owned),
    })
}
