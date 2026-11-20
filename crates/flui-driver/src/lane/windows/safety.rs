//! The Windows lane's safety rules, ported from `desktop-mcp`, as pure functions over what the
//! lane read of the desktop, each refusal with its wire code ([`Refusal::into_error`]).
//!
//! Before every event ([`check_event`]):
//!
//! 1. the lane's desktop is the one taking the user's input: a locked workstation, a disconnected
//!    session or another desktop switched in is a lane the host cannot run (cannot verify), and
//!    nothing is sent there;
//! 2. the target window still exists and is the app's;
//! 3. it is the foreground window, so no key lands in another application;
//! 4. the point a pointer event lands on is inside it, as the OS hit-tests: being in front does
//!    not make the app the window under the point (a non-activating overlay takes the press);
//! 5. no physical input is held: a mouse button, modifier or key the lane did not press would
//!    join its gesture or chord.
//!
//! Before a sequence's first event (`held::HeldKeys::shell_hotkey`, over [`press_takes`]): a
//! press the OS shell takes before the foreground window sees it (the Windows key, the task
//! switcher, Task Manager, a lock key) is refused, since no foreground check can hold for it. The
//! rule reads the key edges the lane will send, typed text's Tab and Enter included. A release
//! that completes a shortcut (the input-language switch, Sticky Keys), which [`ShellShortcuts`]
//! counts, is masked instead (`held::MASK`).

use flui_protocol::session::Isolation;

use crate::coordinates::DevicePoint;
use crate::error::Error;
use crate::lane::windows::keys::{Modifier, VirtualKey};

/// The lane's name, which its refusals carry.
pub(crate) const LANE: &str = "windows";

/// Why the lane refuses an event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The lane's desktop does not take the user's input: locked, disconnected or switched away.
    OffInputDesktop,
    /// The app's window is gone.
    WindowGone,
    /// Another window is in front.
    NotForeground {
        /// The window in front, for people.
        foreground: Option<String>,
    },
    /// The point is not on the app's window.
    Covered {
        /// The point, in screen device pixels.
        point: DevicePoint,
        /// The window there instead, for people.
        covered_by: Option<String>,
    },
    /// Physical input the lane did not press is held.
    InputHeld {
        /// What is held, for people.
        what: String,
    },
    /// The OS shell takes the chord before the app sees it.
    ShellHotkey {
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
    /// A Windows isolation mode whose spike has not shown that injected input stays out of the
    /// host session and that the GPU presents there.
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
/// built. Windows Sandbox and a second session (a loopback RDP session is one) run once their
/// spikes pass on a Windows host; until then they are refused, so no run claims an isolation it
/// does not have.
///
/// # Errors
///
/// [`Refusal::UnprovenIsolation`] for Windows Sandbox and a second session;
/// [`Refusal::ForeignIsolation`] for any other platform's mode, and for no OS at all.
pub fn admit_isolation(isolation: Isolation) -> Result<(), Refusal> {
    match isolation {
        Isolation::Shared => Ok(()),
        Isolation::Sandbox | Isolation::SecondSession => {
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
            Self::OffInputDesktop => Error::Lane {
                lane: LANE.to_owned(),
                unavailable: true,
                message: "the lane's desktop is not taking the user's input (the workstation is \
                          locked, the session is disconnected, or another desktop is switched in), \
                          so no OS input is sent there"
                    .to_owned(),
            },
            Self::WindowGone => Error::gone(
                flui_protocol::error::HandleKind::Window,
                None,
                None,
                "the app's window is gone",
            ),
            Self::NotForeground { foreground } => Error::NotForeground {
                message: format!(
                    "the app's window is not in front{}; no input was sent",
                    foreground.map_or_default(|window| format!(" ({window} is)"))
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
                    "{handler} takes the chord before the app's window sees it, so no foreground \
                     check holds for it; it is refused"
                ),
            ),
            Self::UnknownKey { name } => Error::not_supported(
                "os_key",
                format!("the windows lane has no key named {name:?}"),
            ),
            Self::UnsentInput { what } => Error::not_supported(
                "os_input_device",
                format!("the windows lane does not send {what}"),
            ),
            Self::UnprovenIsolation { isolation } => Error::not_supported(
                format!("windows_isolation_{}", isolation.name()),
                format!(
                    "the windows lane does not run in {} yet: no run on a Windows host has shown \
                     that injected input stays out of the host session there and that the GPU \
                     presents; use the shared desktop, announced and guarded",
                    isolation.name()
                ),
            ),
            Self::ForeignIsolation { isolation } => Error::invalid_argument(format!(
                "{} is no isolation mode of a Windows desktop",
                isolation.name()
            )),
        }
    }
}

/// Mouse buttons as `GetAsyncKeyState` sees them, as bits: 1 left, 2 right, 4 middle, 8 and 16
/// the two extra buttons. Physical buttons: a swapped mouse's primary button is its right one.
pub type ButtonBits = u8;

/// What the lane read of the desktop just before an event: a snapshot whose fields are what the
/// rules read, built field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desk {
    /// Whether the lane's desktop takes the user's input.
    pub input_desktop: bool,
    /// Whether the app's window still exists and is the app's.
    pub window_alive: bool,
    /// Whether the app's window is in front; `foreground` names the window that is otherwise.
    pub in_front: bool,
    /// The window in front, for people.
    pub foreground: Option<String>,
    /// The mouse buttons down.
    pub buttons: ButtonBits,
    /// The modifiers down.
    pub modifiers: Vec<Modifier>,
    /// A key down that is neither a modifier nor a mouse button, by its virtual-key code.
    pub other_key: Option<u16>,
}

impl Desk {
    /// A desktop as an app in front, with nothing held, finds it.
    #[must_use]
    pub fn quiet() -> Self {
        Self {
            input_desktop: true,
            window_alive: true,
            in_front: true,
            foreground: None,
            buttons: 0,
            modifiers: Vec::new(),
            other_key: None,
        }
    }
}

/// Where a pointer event lands, and whether the OS hit-tests the app's window there.
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
    /// The keys it pressed, by virtual-key code, modifiers included.
    pub keys: Vec<u16>,
}

impl Held {
    fn holds_modifier(&self, modifier: Modifier) -> bool {
        self.keys.iter().any(|&code| {
            Modifier::of(VirtualKey {
                code,
                extended: false,
            }) == Some(modifier)
        })
    }
}

/// Whether the lane may send its next event, given `desk`, where a pointer event lands (`None`
/// for a key or a pause) and what the lane holds: see the [module docs](self).
///
/// # Errors
///
/// The first rule that does not hold, in the order the module docs list them.
pub fn check_event(desk: &Desk, landing: Option<&Landing>, held: &Held) -> Result<(), Refusal> {
    if !desk.input_desktop {
        return Err(Refusal::OffInputDesktop);
    }
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
    let foreign_buttons = desk.buttons & !held.buttons;
    if foreign_buttons != 0 {
        return Err(Refusal::InputHeld {
            what: format!(
                "a mouse button the lane did not press is held (buttons {foreign_buttons:#07b})"
            ),
        });
    }
    if let Some(modifier) = desk
        .modifiers
        .iter()
        .find(|modifier| !held.holds_modifier(**modifier))
    {
        return Err(Refusal::InputHeld {
            what: format!("{modifier:?} is held, and the lane did not press it"),
        });
    }
    if let Some(code) = desk.other_key.filter(|code| !held.keys.contains(code)) {
        return Err(Refusal::InputHeld {
            what: format!("a key (virtual key {code:#04x}) the lane did not press is held"),
        });
    }
    Ok(())
}

/// One edge of a key in a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyEdge {
    /// Pressed.
    Down(VirtualKey),
    /// Released.
    Up(VirtualKey),
}

/// How many times in a row Shift pressed and released alone turns on the Sticky Keys shortcut
/// (Microsoft Support, "Make your keyboard easier to use": press Shift five times).
pub const STICKY_KEYS_TAPS: u8 = 5;

/// The Sticky Keys shortcut by name, as a release that would complete it reports it.
pub const STICKY_KEYS: &str = "the accessibility shortcut (Sticky Keys)";

/// The input-language switch by name, as a release that would complete it reports it.
pub const LANGUAGE_SWITCH: &str = "the Windows input-language switch";

fn has(held: &[u16], modifier: Modifier) -> bool {
    held.iter().any(|&code| {
        Modifier::of(VirtualKey {
            code,
            extended: false,
        }) == Some(modifier)
    })
}

/// What takes a press of `key` while the keys `held` are down, if the OS shell does.
#[must_use]
pub fn press_takes(key: VirtualKey, held: &[u16]) -> Option<&'static str> {
    let (shift, control, alt) = (
        has(held, Modifier::Shift),
        has(held, Modifier::Control),
        has(held, Modifier::Alt),
    );
    if Modifier::of(key) == Some(Modifier::Meta) {
        return Some("the Windows shell (the Windows key)");
    }
    match key.code {
        // Caps, Num and Scroll Lock change a state every window shares.
        0x14 | 0x90 | 0x91 => Some("the keyboard's lock state, shared by every window"),
        0x2C => Some("the screenshot tool"),
        0xAD..=0xB3 => Some("the system's volume and media controls"),
        0x09 | 0x1B if alt => Some("the Windows task switcher"),
        0x1B if control && shift => Some("Task Manager"),
        0x1B if control => Some("the Start menu"),
        0x2E if control && alt => Some("the Windows secure attention sequence"),
        _ => None,
    }
}

/// The counts the OS keeps across key presses for its shortcuts that no single press makes: the
/// input-language switch and Sticky Keys. It owns no copy of what is held: each call is told
/// the held keys by their one owner (`held::HeldKeys`), which feeds it every key edge the OS saw,
/// releases included.
///
/// Sticky Keys counts Shift pressed and released alone; another key between starts the count
/// again. Microsoft documents the shortcut as five presses of Shift and nothing else about it: no
/// time window, and nothing about a mouse button, wheel turn or contact between the presses. So
/// neither a pause nor pointer input starts the count again here, and neither breaks a chord of
/// modifiers: what Windows might forget is still counted. A release these counts say would
/// complete a shortcut is masked (`held::MASK`) rather than refused, so the app still gets it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellShortcuts {
    /// Shift and Alt or Control held together with no other key pressed since: releasing them
    /// switches the input language (the default "Left Alt+Shift" and "Ctrl+Shift" hotkeys of
    /// Windows' advanced keyboard settings).
    modifier_chord: bool,
    /// Shift pressed with nothing else held, and nothing pressed since.
    lone_shift: bool,
    /// Shift taps alone in a row.
    shift_taps: u8,
}

impl ShellShortcuts {
    /// A press of `key`, the keys `held` down before it; what it completes, if anything.
    pub(crate) fn press(&mut self, key: VirtualKey, held: &[u16]) -> Option<&'static str> {
        let refused = press_takes(key, held);
        let (shift, control, alt) = (
            has(held, Modifier::Shift),
            has(held, Modifier::Control),
            has(held, Modifier::Alt),
        );
        match Modifier::of(key) {
            Some(Modifier::Shift) => {
                self.lone_shift = held.iter().all(|code| *code == key.code);
                self.modifier_chord |= alt || control;
            }
            Some(Modifier::Alt | Modifier::Control) => {
                self.lone_shift = false;
                self.shift_taps = 0;
                self.modifier_chord |= shift;
            }
            _ => self.other_key(),
        }
        refused
    }

    /// A release of `key`, the keys `held` down after it; what it completes, if anything.
    pub(crate) fn release(&mut self, key: VirtualKey, held: &[u16]) -> Option<&'static str> {
        let modifier = Modifier::of(key);
        let mut completed = None;
        if modifier == Some(Modifier::Shift) && self.lone_shift {
            self.shift_taps = self.shift_taps.saturating_add(1);
            if self.shift_taps >= STICKY_KEYS_TAPS {
                completed = Some(STICKY_KEYS);
            }
        } else if matches!(
            modifier,
            Some(Modifier::Shift | Modifier::Alt | Modifier::Control)
        ) && self.modifier_chord
        {
            completed = Some(LANGUAGE_SWITCH);
        }
        self.lone_shift = false;
        if held.is_empty() {
            self.modifier_chord = false;
        }
        completed
    }

    /// A press of a key that is no modifier: no chord of modifiers and no run of Shift taps goes
    /// on across it.
    pub(crate) fn other_key(&mut self) {
        self.lone_shift = false;
        self.shift_taps = 0;
        self.modifier_chord = false;
    }
}
