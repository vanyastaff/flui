//! What an OS lane sends: an action sequence's plan ([`flui_protocol::input::plan`]) turned into
//! device-pixel events, one at a time, with every pointer move a single step to its destination
//! followed by an explicit pause (ADR-0238 §11, ADR-0400 §2). Only the OS lane plays that plan so
//! far: the runtime's agent lowers `input.inject` with a planner of its own.
//!
//! A lane never interpolates, and the OS or browser is never left to: Chrome interpolates a
//! multi-step move, which made recordings differ between runs. The plan splits a move that lasts
//! (`duration_ms`) into single steps, one per [`STEP`](flui_protocol::input::STEP) of its duration, each at the whole
//! millisecond it is due in, so the path is the driver's and the same on every run.
//!
//! The events go out in the plan's order, and the time between two instants is an explicit pause:
//! so two contacts of a pinch spread at once rather than one after the other, a jump stays a jump
//! beside a stroke, and a short stroke is not slowed to a long one's pace. Every pause is whole
//! milliseconds, none shorter than one, which an OS timer can wait; the events end with the pause
//! to the plan's end.

use std::time::Duration;

use flui_protocol::input::{
    ActionSequence, ImeAction, PlannedInput, PointerButton, PointerDevice, ScrollUnit, SourceKind,
};

use crate::coordinates::{self, DevicePoint, WindowGeometry};
use crate::error::{Error, Result};

/// One event an OS lane sends.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OsEvent {
    /// Move the pointer `pointer` of `device` to `to`, in one step.
    PointerMove {
        /// The source's id in the sequence.
        pointer: String,
        /// What kind of pointer.
        device: PointerDevice,
        /// Where, in screen device pixels.
        to: DevicePoint,
    },
    /// Press `button` of `pointer`, or put the contact down.
    PointerDown {
        /// The source's id.
        pointer: String,
        /// What kind of pointer.
        device: PointerDevice,
        /// The button.
        button: PointerButton,
    },
    /// Release `button` of `pointer`, or lift the contact.
    PointerUp {
        /// The source's id.
        pointer: String,
        /// What kind of pointer.
        device: PointerDevice,
        /// The button.
        button: PointerButton,
    },
    /// Scroll at `at` by whole device pixels.
    Scroll {
        /// Where, in screen device pixels.
        at: DevicePoint,
        /// Rightwards.
        delta_x: i32,
        /// Downwards.
        delta_y: i32,
    },
    /// Press a key, in the platform input vocabulary's spelling.
    KeyDown(String),
    /// Release a key.
    KeyUp(String),
    /// Commit text.
    Text(String),
    /// One step of an input-method composition: only a lane whose capabilities say it composes
    /// (`LaneCapabilities::compositions`) sends it, and `lane::send_guarded` refuses a sequence
    /// holding one before any event for every other lane.
    Composition(ImeAction),
    /// Wait this long before the next event.
    Pause(Duration),
}

/// The events an OS lane sends for `sequence` into the window `geometry` places: see the
/// [module docs](self).
///
/// # Errors
///
/// [`Error::InvalidArgument`] for a sequence the plan refuses (its bounds and ranges); as
/// [`coordinates::to_device`] for a point outside the window; [`Error::NotSupported`] for input no
/// OS lane sends (a trackpad gesture, a pointer's cancel, a virtual pointer, a scroll by lines),
/// naming it. Each is answered before any event is handed over. An input-method composition
/// becomes its steps ([`OsEvent::Composition`]), one per tick, which only a lane that composes
/// takes.
pub fn os_events(sequence: &ActionSequence, geometry: &WindowGeometry) -> Result<Vec<OsEvent>> {
    let plan = flui_protocol::input::plan(sequence)
        .map_err(|error| Error::invalid_argument(error.to_string()))?;
    let sources = plan.sources();
    for source in sources {
        match source.kind {
            SourceKind::Trackpad => return Err(unsent("trackpad_gesture")),
            SourceKind::Pointer(PointerDevice::Virtual) => return Err(unsent("virtual_pointer")),
            _ => {}
        }
    }
    let mut states: Vec<SourceState> = sources.iter().map(|_| SourceState::default()).collect();
    let mut events = Vec::new();
    let mut now = Duration::ZERO;
    for planned in plan.inputs() {
        if let Some(pause) = planned.at.checked_sub(now).filter(|pause| !pause.is_zero()) {
            events.push(OsEvent::Pause(pause));
            now = planned.at;
        }
        let source = &sources[planned.source];
        let pointer = || source.id.clone().unwrap_or_default();
        let device = match source.kind {
            SourceKind::Pointer(device) => device,
            _ => PointerDevice::Mouse,
        };
        let event = match &planned.input {
            PlannedInput::PointerMove { to, .. } => Some(OsEvent::PointerMove {
                pointer: pointer(),
                device,
                to: coordinates::to_device(geometry, *to)?,
            }),
            PlannedInput::PointerDown { button, .. } => Some(OsEvent::PointerDown {
                pointer: pointer(),
                device,
                button: *button,
            }),
            PlannedInput::PointerUp { button, .. } => Some(OsEvent::PointerUp {
                pointer: pointer(),
                device,
                button: *button,
            }),
            PlannedInput::PointerCancel { .. } => return Err(unsent("pointer_cancel")),
            PlannedInput::Scroll {
                at,
                delta_x,
                delta_y,
                unit,
            } => {
                // An OS event scrolls by device pixels, and no lane knows the line height the app
                // scrolls a line by: a line scroll goes the synthetic path, which the app sizes.
                if *unit != ScrollUnit::Pixel {
                    return Err(unsent(&format!("{}_scroll", unit.name())));
                }
                let at = coordinates::to_device(geometry, *at)?;
                let state = &mut states[planned.source];
                let (asked_x, asked_y) = &mut state.scrolled;
                let (sent_x, sent_y) = &mut state.sent;
                let delta_x = SourceState::delta(asked_x, sent_x, *delta_x, geometry.scale)?;
                let delta_y = SourceState::delta(asked_y, sent_y, *delta_y, geometry.scale)?;
                // A scroll that adds up to no whole pixel yet is carried, not sent.
                (delta_x != 0 || delta_y != 0).then_some(OsEvent::Scroll {
                    at,
                    delta_x,
                    delta_y,
                })
            }
            PlannedInput::KeyDown(key) => Some(OsEvent::KeyDown(key.clone())),
            PlannedInput::KeyUp(key) => Some(OsEvent::KeyUp(key.clone())),
            PlannedInput::Text(text) => Some(OsEvent::Text(text.clone())),
            PlannedInput::Ime(action) => Some(OsEvent::Composition(action.clone())),
            _ => return Err(unsent("input")),
        };
        events.extend(event);
    }
    let rest = plan.span().saturating_sub(now);
    if !rest.is_zero() {
        events.push(OsEvent::Pause(rest));
    }
    Ok(events)
}

/// How far one wheel source has turned in
/// device pixels, asked for and sent, so the part of a pixel one scroll leaves is carried into the
/// next and small scrolls add up instead of each rounding to nothing.
#[derive(Default)]
struct SourceState {
    scrolled: (f64, f64),
    sent: (i64, i64),
}

impl SourceState {
    /// The whole device pixels to send for a scroll of `logical` at `scale` on the axis `asked`
    /// and `sent` keep: what the total asked for rounds to, less what went out before.
    fn delta(asked: &mut f64, sent: &mut i64, logical: f64, scale: f64) -> Result<i32> {
        let total = *asked + logical * scale;
        let rounded = total.round();
        let fits =
            rounded.is_finite() && rounded >= f64::from(i32::MIN) && rounded <= f64::from(i32::MAX);
        if !fits {
            return Err(Error::invalid_argument(format!(
                "a scroll of {logical} logical pixels does not fit the screen's range"
            )));
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number checked inside i32's range"
        )]
        let rounded = rounded as i64;
        let delta = i32::try_from(rounded - *sent).map_err(|_| {
            Error::invalid_argument(format!(
                "a scroll of {logical} logical pixels does not fit the screen's range"
            ))
        })?;
        *asked = total;
        *sent = rounded;
        Ok(delta)
    }
}

fn unsent(capability: &str) -> Error {
    Error::not_supported(
        format!("os_{capability}"),
        format!("no OS lane sends {capability}; use the synthetic path"),
    )
}
