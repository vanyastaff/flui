//! What `getevent -l` prints, read as untrusted input, and the rule that tells the person's input
//! from the lane's own: the pure half of the lane's user-activity guard on a device.
//!
//! Run without a device argument, `getevent` names each input device as it opens it (`add device
//! N: /dev/input/eventK`, then `  name:     "…"`) and prefixes each event with the device's path:
//! `/dev/input/event4: EV_KEY       BTN_TOUCH            DOWN`. A device added later (the lane's
//! own `uinput` stylus) is named the same way, and a removed one says `remove device`.
//!
//! - **The lane's own input** comes from no device node when it goes through `input` (the
//!   dispatcher takes it directly), and from a `uinput` device the lane names
//!   ([`super::uinput::STYLUS_NAME`], [`super::uinput::PROBE_NAME`]); every other device's input
//!   is someone else's.
//! - **What counts** is what a person makes: a key or button pressed or released (`EV_KEY` `DOWN`
//!   or `UP`, not a repeat), a touch or pen contact moving (`ABS_MT_*`, which only touch devices
//!   report), a mouse moving or its wheel turning (`EV_REL`). An accelerometer reporting
//!   `ABS_X`/`ABS_Y`/`ABS_Z`, a switch (`EV_SW`), a sync or a `MSC` event is no one's input.
//! - **Bounded.** At most [`MAX_DEVICES`] devices are remembered; past that a new device is taken
//!   as someone else's, which ends a run rather than letting its input pass.

use super::uinput::{PROBE_NAME, STYLUS_NAME};

/// The most devices the watch remembers.
pub const MAX_DEVICES: usize = 64;

/// The line that runs the watch, on a terminal so each line comes as it is printed.
pub const WATCH: &str = "getevent -l";

/// One line of `getevent -l`, read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Line {
    /// `add device N: <path>`.
    Added(String),
    /// `  name:     "<name>"`, naming the device added last.
    Name(String),
    /// `remove device N: <path>`.
    Removed(String),
    /// An event of the device at `path`.
    Event {
        /// The device's path.
        path: String,
        /// The event type's label: `EV_KEY`, `EV_ABS`.
        kind: String,
        /// The code's label: `BTN_TOUCH`, `ABS_MT_POSITION_X`.
        code: String,
        /// The value's label or hex digits: `DOWN`, `0000021c`.
        value: String,
    },
    /// Anything else: a line the watch does not read.
    Other,
}

/// `line` of `getevent -l`, read.
#[must_use]
pub fn parse_line(line: &str) -> Line {
    if let Some(rest) = line.strip_prefix("add device ") {
        return rest
            .split_once(": ")
            .map_or(Line::Other, |(_, path)| Line::Added(path.trim().to_owned()));
    }
    if let Some(rest) = line.strip_prefix("remove device ") {
        return rest.split_once(": ").map_or(Line::Other, |(_, path)| {
            Line::Removed(path.trim().to_owned())
        });
    }
    if let Some(name) = line.trim_start().strip_prefix("name:") {
        let name = name.trim();
        let name = name
            .strip_prefix('"')
            .and_then(|name| name.strip_suffix('"'))
            .unwrap_or(name);
        return Line::Name(name.to_owned());
    }
    let Some((path, event)) = line.split_once(": ") else {
        return Line::Other;
    };
    if !path.starts_with('/') {
        return Line::Other;
    }
    let mut words = event.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some(kind), Some(code), Some(value)) => Line::Event {
            path: path.to_owned(),
            kind: kind.to_owned(),
            code: code.to_owned(),
            value: value.to_owned(),
        },
        _ => Line::Other,
    }
}

/// Whether an event of `kind`, `code` and `value` is what a person makes: see the
/// [module docs](self).
#[must_use]
pub fn is_input(kind: &str, code: &str, value: &str) -> bool {
    match kind {
        "EV_KEY" => value != "REPEAT",
        "EV_ABS" => code.starts_with("ABS_MT_"),
        "EV_REL" => true,
        _ => false,
    }
}

/// The guard's watch over `getevent`'s lines: which device is whose, and the first input of
/// someone else's since it was armed.
#[derive(Debug, Clone, Default)]
pub struct Watch {
    /// Each device's path and whether it is one of the lane's own.
    devices: Vec<(String, bool)>,
    /// The device added last, waiting for its name.
    naming: Option<String>,
    armed: bool,
    foreign: Option<String>,
}

impl Watch {
    /// A watch that has read nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether it has seen any device: `getevent` has started.
    #[must_use]
    pub fn started(&self) -> bool {
        !self.devices.is_empty() || self.naming.is_some()
    }

    /// Report input from now on; what came before is forgotten.
    pub fn arm(&mut self) {
        self.armed = true;
        self.foreign = None;
    }

    /// Stop reporting.
    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// Read `line`.
    pub fn read(&mut self, line: &str) {
        match parse_line(line) {
            Line::Added(path) => {
                self.devices.retain(|(known, _)| *known != path);
                self.naming = Some(path);
            }
            Line::Name(name) => {
                if let Some(path) = self.naming.take()
                    && self.devices.len() < MAX_DEVICES
                {
                    let own = name == STYLUS_NAME || name == PROBE_NAME;
                    self.devices.push((path, own));
                }
            }
            Line::Removed(path) => self.devices.retain(|(known, _)| *known != path),
            Line::Event {
                path,
                kind,
                code,
                value,
            } => {
                if !self.armed || self.foreign.is_some() || !is_input(&kind, &code, &value) {
                    return;
                }
                let own = self
                    .devices
                    .iter()
                    .find(|(known, _)| *known == path)
                    .is_some_and(|(_, own)| *own);
                if !own {
                    let name = path.rsplit('/').next().unwrap_or(&path);
                    self.foreign = Some(format!("{kind} {code} {value} from {name}"));
                }
            }
            Line::Other => {}
        }
    }

    /// The first input of someone else's since [`Self::arm`], if any came.
    #[must_use]
    pub fn foreign(&self) -> Option<&str> {
        self.foreign.as_deref()
    }
}
