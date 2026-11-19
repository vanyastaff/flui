//! What an OS lane sends: an action sequence turned into device-pixel events, one at a time, with
//! every pointer move a single step to its destination followed by an explicit pause
//! (ADR-0238 §11).
//!
//! A lane never interpolates, and the OS or browser is never left to: Chrome interpolates a
//! multi-step move, which made recordings differ between runs. A move that lasts (`duration_ms`)
//! is split here into single steps, one per [`STEP`] of its duration, each followed by a pause of
//! its share, so the path is the driver's and the same on every run.

use std::time::Duration;

use flui_protocol::geometry::LogicalPoint;
use flui_protocol::input::{
    ActionSequence, InputSource, KeyAction, PointerAction, PointerButton, PointerDevice,
    WheelAction,
};

use crate::coordinates::{self, DevicePoint, WindowGeometry};
use crate::error::{Error, Result};

/// How long one step of a split move lasts, and the pause after a move that names no duration:
/// one frame at 60 Hz, so the app's frames see each step.
pub const STEP: Duration = Duration::from_millis(16);

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
    /// Wait this long before the next event.
    Pause(Duration),
}

impl OsEvent {
    /// Whether this event is input; a pause is not.
    #[must_use]
    pub fn is_input(&self) -> bool {
        !matches!(self, Self::Pause(_))
    }
}

/// The events an OS lane sends for `sequence` into the window `geometry` places: tick by tick,
/// each source's action of a tick in the order the sources are listed.
///
/// # Errors
///
/// As [`coordinates::to_device`] for a point outside the window; [`Error::NotSupported`] for a
/// source no OS lane sends (an input-method composition, a trackpad gesture), naming it, before
/// any event is sent.
pub fn os_events(sequence: &ActionSequence, geometry: &WindowGeometry) -> Result<Vec<OsEvent>> {
    let ticks = sequence.ticks();
    let mut positions: Vec<Option<LogicalPoint>> = vec![None; sequence.sources.len()];
    let mut events = Vec::new();
    for tick in 0..ticks {
        for (index, source) in sequence.sources.iter().enumerate() {
            source_tick(source, tick, geometry, &mut positions[index], &mut events)?;
        }
    }
    Ok(events)
}

fn source_tick(
    source: &InputSource,
    tick: usize,
    geometry: &WindowGeometry,
    position: &mut Option<LogicalPoint>,
    events: &mut Vec<OsEvent>,
) -> Result<()> {
    match source {
        InputSource::Pointer {
            id,
            device,
            actions,
        } => {
            let Some(action) = actions.get(tick) else {
                return Ok(());
            };
            pointer_action(id, *device, action, geometry, position, events)
        }
        InputSource::Wheel { actions, .. } => match actions.get(tick) {
            Some(WheelAction::Scroll {
                at,
                delta_x,
                delta_y,
                ..
            }) => {
                let at = coordinates::to_device(geometry, *at)?;
                events.push(OsEvent::Scroll {
                    at,
                    delta_x: device_delta(*delta_x, geometry.scale)?,
                    delta_y: device_delta(*delta_y, geometry.scale)?,
                });
                Ok(())
            }
            Some(WheelAction::Pause { duration_ms }) => {
                events.push(OsEvent::Pause(Duration::from_millis(*duration_ms)));
                Ok(())
            }
            _ => Ok(()),
        },
        InputSource::Keys { actions, .. } => {
            match actions.get(tick) {
                Some(KeyAction::Down { key }) => events.push(OsEvent::KeyDown(key.clone())),
                Some(KeyAction::Up { key }) => events.push(OsEvent::KeyUp(key.clone())),
                Some(KeyAction::Pause { duration_ms }) => {
                    events.push(OsEvent::Pause(Duration::from_millis(*duration_ms)));
                }
                _ => {}
            }
            Ok(())
        }
        InputSource::Text { text } => {
            if tick == 0 {
                events.push(OsEvent::Text(text.clone()));
            }
            Ok(())
        }
        InputSource::Ime { .. } => Err(unsent("ime_composition")),
        InputSource::Trackpad { .. } => Err(unsent("trackpad_gesture")),
        _ => Err(unsent("input_source")),
    }
}

fn pointer_action(
    id: &str,
    device: PointerDevice,
    action: &PointerAction,
    geometry: &WindowGeometry,
    position: &mut Option<LogicalPoint>,
    events: &mut Vec<OsEvent>,
) -> Result<()> {
    match action {
        PointerAction::Move {
            to, duration_ms, ..
        } => {
            let duration = Duration::from_millis(*duration_ms);
            let from = position.unwrap_or(*to);
            let steps = split(duration);
            let pause = duration
                .checked_div(steps)
                .unwrap_or(STEP)
                .max(Duration::from_millis(1));
            for step in 1..=steps {
                let fraction = f64::from(step) / f64::from(steps);
                let at = LogicalPoint::new(
                    from.x + (to.x - from.x) * fraction,
                    from.y + (to.y - from.y) * fraction,
                );
                events.push(OsEvent::PointerMove {
                    pointer: id.to_owned(),
                    device,
                    to: coordinates::to_device(geometry, at)?,
                });
                events.push(OsEvent::Pause(if duration.is_zero() {
                    STEP
                } else {
                    pause
                }));
            }
            *position = Some(*to);
        }
        PointerAction::Down { button, .. } => events.push(OsEvent::PointerDown {
            pointer: id.to_owned(),
            device,
            button: *button,
        }),
        PointerAction::Up { button } => events.push(OsEvent::PointerUp {
            pointer: id.to_owned(),
            device,
            button: *button,
        }),
        PointerAction::Pause { duration_ms } => {
            events.push(OsEvent::Pause(Duration::from_millis(*duration_ms)));
        }
        PointerAction::Cancel => return Err(unsent("pointer_cancel")),
        _ => return Err(unsent("pointer_action")),
    }
    Ok(())
}

/// The most single steps one move is split into; a longer move takes longer steps.
pub const MAX_STEPS: u32 = 1024;

/// How many single steps a move of `duration` takes: one per [`STEP`], at least one, at most
/// [`MAX_STEPS`].
fn split(duration: Duration) -> u32 {
    let steps = duration.as_millis().div_ceil(STEP.as_millis()).max(1);
    u32::try_from(steps).map_or(MAX_STEPS, |steps| steps.min(MAX_STEPS))
}

fn device_delta(logical: f64, scale: f64) -> Result<i32> {
    let device = (logical * scale).round();
    if !(device.is_finite() && device >= f64::from(i32::MIN) && device <= f64::from(i32::MAX)) {
        return Err(Error::InvalidArgument {
            message: format!(
                "a scroll of {logical} logical pixels does not fit the screen's range"
            ),
        });
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a whole number checked inside i32's range"
    )]
    Ok(device as i32)
}

fn unsent(capability: &str) -> Error {
    Error::NotSupported {
        capability: Some(format!("os_{capability}")),
        message: format!("no OS lane sends {capability}; use the synthetic path"),
    }
}
