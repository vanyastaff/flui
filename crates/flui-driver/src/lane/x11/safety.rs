//! The X11 lane's safety rules, as pure functions over what the lane read of the X server, each
//! refusal with its wire code ([`Refusal::into_error`]).
//!
//! Before every event ([`check_event`]):
//!
//! 1. the target window is still the app's and mapped;
//! 2. it has the keyboard focus, so no key lands in another client (the lane gives it the focus
//!    once when another window has it, as a window manager does on a click);
//! 3. the point a pointer event lands on is inside it, as the server's stacking says: having the
//!    focus does not make the app the window under the point;
//! 4. no physical input is held: a mouse button or key the lane did not press would join its
//!    gesture or chord.
//!
//! Before a sequence's first event (`held::HeldInput::shell_hotkey`, over [`press_takes`] and
//! [`ShellShortcuts`]): a chord the desktop takes before the app's window sees it (the Super key,
//! the virtual-terminal switch, the X server's terminate key, the window manager's switcher, the
//! accessibility shortcut) is refused, since no focus check holds for it. The rule reads the keys
//! the lane will press, typed text's Return and Tab included.

use flui_protocol::session::Isolation;

use crate::coordinates::DevicePoint;
use crate::error::Error;
use crate::lane::x11::keys::{Keysym, Modifier};

/// The lane's name, which its refusals carry.
pub(crate) const LANE: &str = "x11";

/// A key of the X server's keyboard, by its keycode (8 to 255).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Keycode(pub u8);

/// Why the lane refuses an event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The app's window is gone or unmapped.
    WindowGone,
    /// Another window has the keyboard focus.
    NotFocused {
        /// The window that has it, for people.
        focus: Option<String>,
    },
    /// The point is not on the app's window.
    Covered {
        /// The point, in screen pixels.
        point: DevicePoint,
        /// The window there instead, for people.
        covered_by: Option<String>,
    },
    /// Physical input the lane did not press is held.
    InputHeld {
        /// What is held, for people.
        what: String,
    },
    /// The desktop takes the chord before the app sees it.
    ShellHotkey {
        /// What takes it.
        handler: &'static str,
    },
    /// A key the server's keymap has no key for, and no character the lane could map a key to.
    UnknownKey {
        /// The key, as named.
        name: String,
    },
    /// A device, button or text the lane does not send.
    UnsentInput {
        /// What it is.
        what: String,
    },
    /// The keymap has no key free for a character it carries on no key.
    NoSpareKey {
        /// The characters, for people.
        text: String,
    },
    /// An isolation mode that is no X server's.
    ForeignIsolation {
        /// The mode.
        isolation: Isolation,
    },
}

/// Whether the lane may run in `isolation`: on a desktop's own X server, announced and guarded,
/// or on an X virtual framebuffer the run started, where its guard still watches.
///
/// # Errors
///
/// [`Refusal::ForeignIsolation`] for any other mode.
pub fn admit_isolation(isolation: Isolation) -> Result<(), Refusal> {
    match isolation {
        Isolation::Shared | Isolation::Xvfb => Ok(()),
        _ => Err(Refusal::ForeignIsolation { isolation }),
    }
}

impl Refusal {
    /// The error the refusal answers with.
    #[must_use]
    pub fn into_error(self) -> Error {
        match self {
            Self::WindowGone => Error::gone(
                flui_protocol::error::HandleKind::Window,
                None,
                None,
                "the app's window is gone or unmapped",
            ),
            Self::NotFocused { focus } => Error::NotForeground {
                message: format!(
                    "the app's window does not have the keyboard focus{}; no input was sent",
                    focus.map_or_default(|window| format!(" ({window} has it)"))
                ),
                retry: flui_protocol::error::ErrorCode::NotForeground.default_retry(),
            },
            Self::Covered { point, covered_by } => Error::outside_target(format!(
                "({}, {}) does not reach the app's window{}; nothing was sent there",
                point.x,
                point.y,
                covered_by.map_or_default(|window| format!(": {window} is there"))
            )),
            Self::InputHeld { what } => Error::InputHeld {
                message: format!("{what}; the lane sends nothing that would join it"),
                retry: flui_protocol::error::ErrorCode::InputHeld.default_retry(),
            },
            Self::ShellHotkey { handler } => Error::not_supported(
                "os_shell_hotkey",
                format!(
                    "{handler} takes the chord before the app's window sees it, so no focus check \
                     holds for it; it is refused"
                ),
            ),
            Self::UnknownKey { name } => {
                Error::not_supported("os_key", format!("the x11 lane has no key named {name:?}"))
            }
            Self::UnsentInput { what } => Error::not_supported(
                "os_input_device",
                format!("the {LANE} lane does not send {what}"),
            ),
            Self::NoSpareKey { text } => Error::not_supported(
                "os_key",
                format!(
                    "the X server's keymap carries {text:?} on no key and has no key free to map \
                     it to"
                ),
            ),
            Self::ForeignIsolation { isolation } => Error::invalid_argument(format!(
                "{} is no isolation mode of an X server",
                isolation.name()
            )),
        }
    }
}

/// Mouse buttons as bits, button `n` (1 to 9) at bit `n - 1`.
pub type ButtonBits = u16;

/// What the lane read of the X server just before an event: a snapshot whose fields are what the
/// rules read, built field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desk {
    /// Whether the app's window is still the app's and mapped.
    pub window_alive: bool,
    /// Whether the app's window, or a window inside it, has the keyboard focus.
    pub focused: bool,
    /// The window that has the focus otherwise, for people.
    pub focus: Option<String>,
    /// The mouse buttons down.
    pub buttons: ButtonBits,
    /// The keys down, by keycode, the lane's own included.
    pub keys: Vec<Keycode>,
}

impl Desk {
    /// A server as an app with the focus, with nothing held, finds it.
    #[must_use]
    pub fn quiet() -> Self {
        Self {
            window_alive: true,
            focused: true,
            focus: None,
            buttons: 0,
            keys: Vec::new(),
        }
    }
}

/// Where a pointer event lands, and whether the server stacks the app's window there.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Landing {
    /// The point.
    pub point: DevicePoint,
    /// Whether the top-level window under it is the app's.
    pub on_target: bool,
    /// The window there, for people.
    pub window: Option<String>,
}

impl Landing {
    /// `point`, on the app's window when `on_target`, else on `window`.
    #[must_use]
    pub fn new(point: DevicePoint, on_target: bool, window: Option<String>) -> Self {
        Self {
            point,
            on_target,
            window,
        }
    }
}

/// What the lane itself holds down.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    /// The mouse buttons it pressed.
    pub buttons: ButtonBits,
    /// The keys it pressed.
    pub keys: Vec<Keycode>,
}

/// Whether the lane may send its next event, given `desk`, where a pointer event lands (`None`
/// for a key or a pause) and what the lane holds: see the [module docs](self).
///
/// # Errors
///
/// The first rule that does not hold, in the order the module docs list them.
pub fn check_event(desk: &Desk, landing: Option<&Landing>, held: &Held) -> Result<(), Refusal> {
    if !desk.window_alive {
        return Err(Refusal::WindowGone);
    }
    if !desk.focused {
        return Err(Refusal::NotFocused {
            focus: desk.focus.clone(),
        });
    }
    if let Some(landing) = landing
        && !landing.on_target
    {
        return Err(Refusal::Covered {
            point: landing.point,
            covered_by: landing.window.clone(),
        });
    }
    let foreign_buttons = desk.buttons & !held.buttons;
    if foreign_buttons != 0 {
        return Err(Refusal::InputHeld {
            what: format!(
                "a mouse button the lane did not press is held (buttons {foreign_buttons:#011b})"
            ),
        });
    }
    if let Some(key) = desk.keys.iter().find(|key| !held.keys.contains(key)) {
        return Err(Refusal::InputHeld {
            what: format!("a key (keycode {}) the lane did not press is held", key.0),
        });
    }
    Ok(())
}

/// How many times in a row Shift pressed and released alone turns StickyKeys on, where the
/// server's AccessX keyboard controls take it (the XKB protocol, "AccessX": five presses of
/// Shift).
pub const STICKY_KEYS_TAPS: u8 = 5;

/// What the StickyKeys shortcut is told by name when it is refused.
pub const STICKY_KEYS: &str = "the accessibility shortcut (StickyKeys)";

/// What takes a press of the key typing `keysym` while the modifiers `held` are down, if the
/// desktop does rather than the app's window.
#[must_use]
pub fn press_takes(keysym: Keysym, held: &[Modifier]) -> Option<&'static str> {
    let control = held.contains(&Modifier::Control);
    let alt = held.contains(&Modifier::Alt);
    if Modifier::of(keysym) == Some(Modifier::Super) || held.contains(&Modifier::Super) {
        return Some("the desktop shell (the Super key)");
    }
    match keysym.0 {
        // Caps, Num and Scroll Lock change a state every window shares.
        0xFFE5 | 0xFF7F | 0xFF14 => Some("the keyboard's lock state, shared by every window"),
        0xFF61 => Some("the screenshot tool"),
        0x1008_FF11..=0x1008_FF17 => Some("the system's volume and media controls"),
        // XF86Switch_VT_1 to _12 and XF86Terminate_Server, wherever a keymap puts them.
        0x1008_FE01..=0x1008_FE0C => Some("the virtual-terminal switch"),
        0xFED5 => Some("the X server's terminate key"),
        // F1 to F12 under Control and Alt: the virtual-terminal switch.
        0xFFBE..=0xFFC9 if control && alt => Some("the virtual-terminal switch"),
        0xFF08 if control && alt => Some("the X server's terminate key"),
        0xFFFF if control && alt => Some("the session's log-out dialog"),
        0xFF09 | 0xFF1B if alt => Some("the window manager's switcher"),
        0xFFBE..=0xFFC9 if alt => Some("the window manager's keys"),
        _ => None,
    }
}

/// The count the X server keeps across key presses for the shortcut no single press makes:
/// StickyKeys, five presses of Shift alone. Another key between starts the count again; the XKB
/// protocol says nothing about a pause or pointer input between the presses, so neither does here,
/// which refuses rather than risks the shortcut. It owns no copy of what is held: each call is
/// told the held modifiers by their one owner (`held::HeldInput`), which feeds it every key edge
/// the server took, releases included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellShortcuts {
    /// Shift pressed with nothing else held, and nothing pressed since.
    lone_shift: bool,
    /// Shift taps alone in a row.
    shift_taps: u8,
}

impl ShellShortcuts {
    /// A press of the key typing `keysym`, the modifiers `held` down before it and `alone` when
    /// no key at all was; what it completes, if anything.
    pub(crate) fn press(
        &mut self,
        keysym: Keysym,
        held: &[Modifier],
        alone: bool,
    ) -> Option<&'static str> {
        let refused = press_takes(keysym, held);
        if Modifier::of(keysym) == Some(Modifier::Shift) {
            self.lone_shift = alone;
        } else {
            self.lone_shift = false;
            self.shift_taps = 0;
        }
        refused
    }

    /// A release of the key typing `keysym`; what it completes, if anything.
    pub(crate) fn release(&mut self, keysym: Keysym) -> Option<&'static str> {
        let mut completed = None;
        if Modifier::of(keysym) == Some(Modifier::Shift) && self.lone_shift {
            self.shift_taps = self.shift_taps.saturating_add(1);
            if self.shift_taps >= STICKY_KEYS_TAPS {
                completed = Some(STICKY_KEYS);
            }
        }
        self.lone_shift = false;
        completed
    }
}
