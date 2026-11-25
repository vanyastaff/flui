//! The macOS lane's safety rules, as pure functions over what the lane read of the desktop, each
//! refusal with its wire code ([`Refusal::into_error`]).
//!
//! Before every event ([`check_event`]):
//!
//! 1. the target window still exists;
//! 2. it is the frontmost window and its app is the one keys go to;
//! 3. the point a pointer event lands on is inside it, as the window server stacks windows: being
//!    in front does not make the app the window under the point (a floating panel, the Dock or the
//!    menu bar takes the press);
//! 4. no physical input is held: a mouse button, modifier or key someone holds would join the
//!    lane's gesture or chord. The lane posts its events to the app's process from a private
//!    event source, so nothing it sends shows in the hardware state these rules read, and
//!    anything held there is someone else's.
//!
//! Before a sequence's first event (`held::HeldKeys::system_shortcut`, over [`press_takes`] and
//! [`ModifierTaps`]): a shortcut the system takes before the app sees it (the app switcher,
//! Spotlight, Force Quit, Mission Control and the Spaces, the screenshot tool, VoiceOver and the
//! other accessibility toggles, the input-source switch, Siri, the lock screen) is refused, since
//! no foreground check holds for it. The rule reads the key edges the lane will send, typed text's
//! Tab and Return included.
//!
//! What the lane needs from the person first, the system's privacy permissions, is refused by
//! name ([`Permission`]): the lane asks the system whether it has each one before the call that
//! needs it, and never waits on a prompt.

use flui_protocol::session::Isolation;

use crate::coordinates::DevicePoint;
use crate::error::Error;
use crate::lane::macos::keys::{self, KeyCode, Modifier};

/// The lane's name, which its refusals carry.
pub(crate) const LANE: &str = "macos";

/// A privacy permission the lane needs, granted under System Settings, Privacy & Security.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Permission {
    /// Accessibility: posting events to another app and reading its accessibility tree.
    Accessibility,
    /// Input Monitoring: the event tap the user-activity guard listens with.
    InputMonitoring,
    /// Screen Recording: capturing another app's window.
    ScreenRecording,
}

impl Permission {
    /// The permission's name in the capability a refusal names: `macos_permission_<name>`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Accessibility => "accessibility",
            Self::InputMonitoring => "input_monitoring",
            Self::ScreenRecording => "screen_recording",
        }
    }

    /// The pane of System Settings, Privacy & Security, that grants it.
    #[must_use]
    pub const fn pane(self) -> &'static str {
        match self {
            Self::Accessibility => "Accessibility",
            Self::InputMonitoring => "Input Monitoring",
            Self::ScreenRecording => "Screen Recording",
        }
    }
}

/// Whether the lane may make a call that needs `permission`, the system having answered
/// `granted`: asked before every event, every release and every read, since the person can take
/// a permission back while a run goes on, and an event posted without Accessibility is dropped
/// with no word.
///
/// # Errors
///
/// [`Refusal::MissingPermission`] naming `permission` when it is not granted.
pub fn permitted(permission: Permission, granted: bool) -> Result<(), Refusal> {
    if granted {
        Ok(())
    } else {
        Err(Refusal::MissingPermission { permission })
    }
}

/// Why the lane refuses an event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The process running the driver lacks a privacy permission the call needs.
    MissingPermission {
        /// The permission.
        permission: Permission,
    },
    /// The app's window is gone.
    WindowGone,
    /// Another window is in front, or another app takes the keys.
    NotForeground {
        /// What is in front, for people.
        foreground: Option<String>,
    },
    /// The point is not on the app's window.
    Covered {
        /// The point, in screen device pixels.
        point: DevicePoint,
        /// The window there instead, for people.
        covered_by: Option<String>,
    },
    /// Physical input someone else holds.
    InputHeld {
        /// What is held, for people.
        what: String,
    },
    /// The system takes the shortcut before the app sees it.
    SystemShortcut {
        /// What takes it.
        handler: &'static str,
    },
    /// A key the lane has no code for.
    UnknownKey {
        /// The key, as named.
        name: String,
    },
    /// A device or button the lane does not send.
    UnsentInput {
        /// What it is.
        what: String,
    },
    /// A macOS isolation mode no run has shown keeps injected input out of the host session.
    UnprovenIsolation {
        /// The mode.
        isolation: Isolation,
    },
    /// An isolation mode of another platform.
    ForeignIsolation {
        /// The mode.
        isolation: Isolation,
    },
}

/// Whether the lane may run in `isolation`: the shared desktop, with its guard, is the one mode
/// built. A second user session and a virtual machine run once a run on a Mac shows that the
/// lane's input stays out of the host session there; until then they are refused, so no run
/// claims an isolation it does not have.
///
/// # Errors
///
/// [`Refusal::UnprovenIsolation`] for a second session and a virtual machine;
/// [`Refusal::ForeignIsolation`] for any other platform's mode, and for no OS at all.
pub fn admit_isolation(isolation: Isolation) -> Result<(), Refusal> {
    match isolation {
        Isolation::Shared => Ok(()),
        Isolation::SecondSession | Isolation::VirtualMachine => {
            Err(Refusal::UnprovenIsolation { isolation })
        }
        _ => Err(Refusal::ForeignIsolation { isolation }),
    }
}

impl Refusal {
    /// The error the refusal answers with.
    #[must_use]
    pub fn into_error(self) -> Error {
        match self {
            Self::MissingPermission { permission } => Error::not_supported(
                format!("macos_permission_{}", permission.name()),
                format!(
                    "the process running the driver has no {} permission; grant it under System \
                     Settings > Privacy & Security > {}, then start the process again",
                    permission.pane(),
                    permission.pane()
                ),
            ),
            Self::WindowGone => Error::gone(
                flui_protocol::error::HandleKind::Window,
                None,
                None,
                "the app's window is gone",
            ),
            Self::NotForeground { foreground } => Error::NotForeground {
                message: format!(
                    "the app's window is not in front{}: the input is not sent there",
                    foreground.map_or_default(|window| format!(" ({window} is)"))
                ),
                retry: flui_protocol::error::ErrorCode::NotForeground.default_retry(),
            },
            Self::Covered { point, covered_by } => Error::outside_target(format!(
                "({}, {}) does not reach the app's window{}: the event is not sent there",
                point.x,
                point.y,
                covered_by.map_or_default(|window| format!(": {window} is there"))
            )),
            Self::InputHeld { what } => Error::InputHeld {
                message: format!("{what}; the lane sends nothing that would join it"),
                effect: None,
                retry: flui_protocol::error::ErrorCode::InputHeld.default_retry(),
            },
            Self::SystemShortcut { handler } => Error::not_supported(
                "os_shell_hotkey",
                format!(
                    "{handler} takes the shortcut before the app's window sees it, so no \
                     foreground check holds for it; it is refused"
                ),
            ),
            Self::UnknownKey { name } => Error::not_supported(
                "os_key",
                format!(
                    "the {LANE} lane has no key named {name:?}; a character no ANSI key types is \
                     typed as text"
                ),
            ),
            Self::UnsentInput { what } => Error::not_supported(
                "os_input_device",
                format!("the {LANE} lane does not send {what}"),
            ),
            Self::UnprovenIsolation { isolation } => Error::not_supported(
                format!("{LANE}_isolation_{}", isolation.name()),
                format!(
                    "the {LANE} lane does not run in {} yet: no run on a Mac has shown that its \
                     input stays out of the host session there; use the shared desktop, \
                     announced and guarded",
                    isolation.name()
                ),
            ),
            Self::ForeignIsolation { isolation } => Error::invalid_argument(format!(
                "{} is no isolation mode of a macOS desktop",
                isolation.name()
            )),
        }
    }
}

/// Mouse buttons as bits: 1 left, 2 right, 4 the middle or another button.
pub type ButtonBits = u8;

/// What the lane read of the desktop just before an event: a snapshot whose fields are what the
/// rules read, built field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desk {
    /// Whether the app's window still exists.
    pub window_alive: bool,
    /// Whether the app's window is the frontmost one and its app takes the keys; `foreground`
    /// names what is in front otherwise.
    pub in_front: bool,
    /// What is in front, for people.
    pub foreground: Option<String>,
    /// The mouse buttons someone holds.
    pub buttons: ButtonBits,
    /// The modifiers someone holds.
    pub modifiers: Vec<Modifier>,
    /// A key someone holds that is no modifier, by its key code.
    pub other_key: Option<u16>,
}

impl Desk {
    /// A desktop as an app in front, with nothing held, finds it.
    #[must_use]
    pub fn quiet() -> Self {
        Self {
            window_alive: true,
            in_front: true,
            foreground: None,
            buttons: 0,
            modifiers: Vec::new(),
            other_key: None,
        }
    }
}

/// Where a pointer event lands, and whether the window server stacks the app's window there.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Landing {
    /// The point, in screen device pixels.
    pub point: DevicePoint,
    /// Whether the topmost window under it is the app's.
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

/// Whether the lane may send its next event, given `desk` and where a pointer event lands (`None`
/// for a key or a pause): see the [module docs](self).
///
/// # Errors
///
/// The first rule that does not hold, in the order the module docs list them.
pub fn check_event(desk: &Desk, landing: Option<&Landing>) -> Result<(), Refusal> {
    if !desk.window_alive {
        return Err(Refusal::WindowGone);
    }
    if !desk.in_front {
        return Err(Refusal::NotForeground {
            foreground: desk.foreground.clone(),
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
    if desk.buttons != 0 {
        return Err(Refusal::InputHeld {
            what: format!("a mouse button is held (buttons {:#05b})", desk.buttons),
        });
    }
    if let Some(modifier) = desk.modifiers.first() {
        return Err(Refusal::InputHeld {
            what: format!("{modifier:?} is held"),
        });
    }
    if let Some(code) = desk.other_key {
        return Err(Refusal::InputHeld {
            what: format!("a key (key code {code:#04x}) is held"),
        });
    }
    Ok(())
}

/// One edge of a key in a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyEdge {
    /// Pressed.
    Down(KeyCode),
    /// Released.
    Up(KeyCode),
}

/// What Sticky Keys is told by name when it is refused: System Settings, Accessibility, Keyboard
/// offers "Press the Shift key five times to toggle Sticky Keys".
pub const STICKY_KEYS: &str = "the accessibility shortcut (Sticky Keys)";

/// What Mouse Keys is told by name when it is refused: System Settings, Accessibility, Pointer
/// Control offers "Press the Option key five times to toggle Mouse Keys".
pub const MOUSE_KEYS: &str = "the accessibility shortcut (Mouse Keys)";

/// What Siri is told by name when it is refused: one of its keyboard shortcuts is either Command
/// key pressed twice.
pub const SIRI: &str = "Siri";

/// How many lone taps in a row of a modifier the system takes as a shortcut, if it does.
const fn taps_to_toggle(modifier: Modifier) -> Option<(u8, &'static str)> {
    match modifier {
        Modifier::Shift => Some((5, STICKY_KEYS)),
        Modifier::Option => Some((5, MOUSE_KEYS)),
        Modifier::Command => Some((2, SIRI)),
        Modifier::Control | Modifier::Function => None,
    }
}

fn has(held: &[u16], modifier: Modifier) -> bool {
    held.iter()
        .any(|&code| Modifier::of(KeyCode(code)) == Some(modifier))
}

/// What takes a press of `key` while the keys `held` are down, if the system does. macOS's
/// default shortcuts (System Settings, Keyboard, Keyboard Shortcuts), and the keys whose state
/// every app shares.
#[must_use]
pub fn press_takes(key: KeyCode, held: &[u16]) -> Option<&'static str> {
    let (shift, control, option, command) = (
        has(held, Modifier::Shift),
        has(held, Modifier::Control),
        has(held, Modifier::Option),
        has(held, Modifier::Command),
    );
    if key == keys::FUNCTION {
        return Some("the Globe key (the input source, the emoji viewer, dictation)");
    }
    if key == keys::CAPS_LOCK {
        return Some("the keyboard's lock state, shared by every app");
    }
    if keys::VOLUME.contains(&key) {
        return Some("the system's volume controls");
    }
    if command {
        let taken = match key {
            keys::TAB => Some("the app switcher"),
            keys::GRAVE => Some("the window switcher"),
            keys::SPACE if control => Some("the character viewer"),
            keys::SPACE if option => Some("Finder's search"),
            keys::SPACE => Some("Spotlight"),
            keys::ESCAPE if option => Some("Force Quit"),
            keys::Q if control => Some("the lock screen"),
            keys::Q if shift => Some("log out"),
            keys::D if option => Some("the Dock"),
            keys::F5 => Some("VoiceOver"),
            keys::EIGHT if option => Some("Zoom and the colour filters"),
            keys::EQUAL | keys::MINUS if option => Some("Zoom"),
            _ if shift && keys::DIGITS_1_TO_9[2..6].contains(&key) => Some("the screenshot tool"),
            _ => None,
        };
        if taken.is_some() {
            return taken;
        }
    }
    if control {
        if matches!(key, keys::UP | keys::DOWN) {
            return Some("Mission Control");
        }
        if matches!(key, keys::LEFT | keys::RIGHT) || keys::DIGITS_1_TO_9.contains(&key) {
            return Some("the switch between Spaces");
        }
        if key == keys::SPACE {
            return Some("the input-source switch");
        }
        if keys::F1_TO_F8.contains(&key) {
            return Some("the system's keyboard navigation");
        }
    }
    if key == keys::F11 && !(shift || control || option || command) {
        return Some("Show Desktop");
    }
    None
}

/// The counts the system keeps across key presses for its shortcuts that no single press makes: a
/// modifier tapped alone several times in a row (Sticky Keys, Mouse Keys, Siri). It owns no copy of
/// what is held: each call is told the held keys by their one owner (`held::HeldKeys`), which feeds
/// it every key edge the lane posted, releases included.
///
/// A tap is the modifier pressed with nothing else held and released with nothing pressed in
/// between; another key, or another modifier, starts the count again. Apple documents neither a
/// time window nor what pointer input between the taps does, so neither a pause nor pointer input
/// starts the count again here: what the system might forget is still counted, which refuses
/// rather than risks the shortcut.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModifierTaps {
    /// A modifier pressed with nothing else held, and nothing pressed since.
    lone: Option<Modifier>,
    /// The modifier tapped alone last, and how many times in a row.
    run: Option<(Modifier, u8)>,
}

impl ModifierTaps {
    /// A press of `key`, the keys `held` down before it; what it takes, if anything.
    pub(crate) fn press(&mut self, key: KeyCode, held: &[u16]) -> Option<&'static str> {
        let taken = press_takes(key, held);
        match Modifier::of(key) {
            // A run of another modifier stays: the release and `pending` count only a run of the
            // modifier they read, so the next tap of this one starts at one either way.
            Some(modifier) if held.iter().all(|&code| code == key.0) => {
                self.lone = Some(modifier);
            }
            _ => {
                self.lone = None;
                self.run = None;
            }
        }
        taken
    }

    /// A release of `key`; what it completes, if anything.
    pub(crate) fn release(&mut self, key: KeyCode) -> Option<&'static str> {
        let modifier = Modifier::of(key);
        let mut completed = None;
        if let Some(modifier) = modifier
            && self.lone == Some(modifier)
        {
            let count = match self.run {
                Some((tapped, count)) if tapped == modifier => count.saturating_add(1),
                _ => 1,
            };
            self.run = Some((modifier, count));
            if let Some((taps, handler)) = taps_to_toggle(modifier)
                && count >= taps
            {
                completed = Some(handler);
            }
        }
        self.lone = None;
        completed
    }

    /// The shortcut a release of the modifier held alone now would complete, with no other key
    /// pressed first.
    #[must_use]
    pub(crate) fn pending(&self) -> Option<&'static str> {
        let modifier = self.lone?;
        let (taps, handler) = taps_to_toggle(modifier)?;
        let count = match self.run {
            Some((tapped, count)) if tapped == modifier => count,
            _ => 0,
        };
        (count.saturating_add(1) >= taps).then_some(handler)
    }
}
