//! What an OS lane sends: an action sequence turned into device-pixel events, one at a time, with
//! every pointer move a single step to its destination followed by an explicit pause
//! (ADR-0238 §11).
//!
//! A lane never interpolates, and the OS or browser is never left to: Chrome interpolates a
//! multi-step move, which made recordings differ between runs. A move that lasts (`duration_ms`)
//! is split here into single steps, one per [`STEP`] of its duration, each followed by a pause of
//! its share, so the path is the driver's and the same on every run.
//!
//! The sources of a tick move together, each at its own speed: within one tick, the presses,
//! releases, keys and text come first, in the order the sources are listed, and then the moves
//! share one timeline. Each is split by its own duration, and the steps due within one
//! millisecond go out together before the pause to the next, so two contacts of a pinch spread at
//! once rather than one after the other, a jump stays a jump beside a stroke, and a short stroke
//! is not slowed to a long one's pace, as on the synthetic path. Every pause is whole
//! milliseconds, none shorter than one, which an OS timer can wait; a tick lasts exactly as long
//! as its longest move or pause.

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

/// The most single steps one move is split into; a longer move takes longer steps.
pub const MAX_STEPS: u32 = 1024;

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

/// A move one source makes in a tick, before it is split into steps.
struct Stroke<'a> {
    pointer: &'a str,
    device: PointerDevice,
    from: LogicalPoint,
    to: LogicalPoint,
    duration: Duration,
}

/// What one tick holds: the events that happen at once, the moves, and the longest pause.
#[derive(Default)]
struct Tick<'a> {
    instant: Vec<OsEvent>,
    strokes: Vec<Stroke<'a>>,
    pause: Duration,
}

/// The events an OS lane sends for `sequence` into the window `geometry` places: see the
/// [module docs](self).
///
/// # Errors
///
/// As [`coordinates::to_device`] for a point outside the window; [`Error::NotSupported`] for a
/// source no OS lane sends (an input-method composition, a trackpad gesture), naming it. Either
/// is answered before any event is handed over.
pub fn os_events(sequence: &ActionSequence, geometry: &WindowGeometry) -> Result<Vec<OsEvent>> {
    let mut positions: Vec<Option<LogicalPoint>> = vec![None; sequence.sources.len()];
    let mut events = Vec::new();
    for index in 0..sequence.ticks() {
        let mut tick = Tick::default();
        for (source, position) in sequence.sources.iter().zip(positions.iter_mut()) {
            source_tick(source, index, geometry, position, &mut tick)?;
        }
        tick.emit(geometry, &mut events)?;
    }
    Ok(events)
}

impl Tick<'_> {
    /// The tick's events, in order: what happens at once, then the strokes' steps on one timeline,
    /// each stroke split by its own duration, so it keeps its own speed. The steps due within one
    /// millisecond go out together, in the order the sources are listed, then a pause until the
    /// next millisecond a step is due in, or until the tick ends: at its longest stroke, its
    /// longest pause, or one [`STEP`] after a stroke that names no duration.
    fn emit(self, geometry: &WindowGeometry, events: &mut Vec<OsEvent>) -> Result<()> {
        events.extend(self.instant);
        // Every step of every stroke, at the whole millisecond it is due in from the tick's start:
        // steps due within one millisecond go out together, so no pause is shorter than one, and
        // each instant is counted from the tick's start, so the pauses add up to its length
        // exactly. One stroke's steps are at least a millisecond apart, so none of them merge.
        let mut steps: Vec<(Duration, usize, LogicalPoint)> = Vec::new();
        let mut end = self.pause;
        for (order, stroke) in self.strokes.iter().enumerate() {
            let count = split(stroke.duration);
            let share = stroke.duration.checked_div(count).unwrap_or(Duration::ZERO);
            for step in 1..=count {
                let fraction = f64::from(step) / f64::from(count);
                let at = LogicalPoint::new(
                    stroke.from.x + (stroke.to.x - stroke.from.x) * fraction,
                    stroke.from.y + (stroke.to.y - stroke.from.y) * fraction,
                );
                steps.push((whole_millis(share.saturating_mul(step - 1)), order, at));
            }
            end = end.max(if stroke.duration.is_zero() {
                STEP
            } else {
                stroke.duration
            });
        }
        // By instant, then by source: a stable order on every run.
        steps.sort_by_key(|&(due, order, _)| (due, order));
        let mut now = Duration::ZERO;
        for (index, &(due, order, at)) in steps.iter().enumerate() {
            let stroke = &self.strokes[order];
            events.push(OsEvent::PointerMove {
                pointer: stroke.pointer.to_owned(),
                device: stroke.device,
                to: coordinates::to_device(geometry, at)?,
            });
            now = due;
            let next = steps.get(index + 1).map_or(end, |&(next, ..)| next);
            let gap = next.saturating_sub(now);
            if !gap.is_zero() {
                events.push(OsEvent::Pause(gap));
                now = next;
            }
        }
        let rest = end.saturating_sub(now);
        if !rest.is_zero() {
            events.push(OsEvent::Pause(rest));
        }
        Ok(())
    }
}

fn source_tick<'a>(
    source: &'a InputSource,
    index: usize,
    geometry: &WindowGeometry,
    position: &mut Option<LogicalPoint>,
    tick: &mut Tick<'a>,
) -> Result<()> {
    let wait = |tick: &mut Tick<'a>, duration_ms: u64| {
        tick.pause = tick.pause.max(Duration::from_millis(duration_ms));
    };
    match source {
        InputSource::Pointer {
            id,
            device,
            actions,
        } => match actions.get(index) {
            Some(PointerAction::Move {
                to, duration_ms, ..
            }) => {
                tick.strokes.push(Stroke {
                    pointer: id,
                    device: *device,
                    from: position.unwrap_or(*to),
                    to: *to,
                    duration: Duration::from_millis(*duration_ms),
                });
                *position = Some(*to);
            }
            Some(PointerAction::Down { button, .. }) => tick.instant.push(OsEvent::PointerDown {
                pointer: id.clone(),
                device: *device,
                button: *button,
            }),
            Some(PointerAction::Up { button }) => tick.instant.push(OsEvent::PointerUp {
                pointer: id.clone(),
                device: *device,
                button: *button,
            }),
            Some(PointerAction::Pause { duration_ms }) => wait(tick, *duration_ms),
            Some(PointerAction::Cancel) => return Err(unsent("pointer_cancel")),
            Some(_) => return Err(unsent("pointer_action")),
            None => {}
        },
        InputSource::Wheel { actions, .. } => match actions.get(index) {
            Some(WheelAction::Scroll {
                at,
                delta_x,
                delta_y,
                ..
            }) => tick.instant.push(OsEvent::Scroll {
                at: coordinates::to_device(geometry, *at)?,
                delta_x: device_delta(*delta_x, geometry.scale)?,
                delta_y: device_delta(*delta_y, geometry.scale)?,
            }),
            Some(WheelAction::Pause { duration_ms }) => wait(tick, *duration_ms),
            _ => {}
        },
        InputSource::Keys { actions, .. } => match actions.get(index) {
            Some(KeyAction::Down { key }) => tick.instant.push(OsEvent::KeyDown(key.clone())),
            Some(KeyAction::Up { key }) => tick.instant.push(OsEvent::KeyUp(key.clone())),
            Some(KeyAction::Pause { duration_ms }) => wait(tick, *duration_ms),
            _ => {}
        },
        InputSource::Text { text } => {
            if index == 0 {
                tick.instant.push(OsEvent::Text(text.clone()));
            }
        }
        InputSource::Ime { .. } => return Err(unsent("ime_composition")),
        InputSource::Trackpad { .. } => return Err(unsent("trackpad_gesture")),
        _ => return Err(unsent("input_source")),
    }
    Ok(())
}

/// `instant` cut to the whole millisecond it falls in.
fn whole_millis(instant: Duration) -> Duration {
    Duration::new(instant.as_secs(), instant.subsec_millis() * 1_000_000)
}

/// How many single steps a move of `duration` takes: one per [`STEP`], at least one, at most
/// [`MAX_STEPS`].
fn split(duration: Duration) -> u32 {
    let steps = duration.as_millis().div_ceil(STEP.as_millis()).max(1);
    u32::try_from(steps).map_or(MAX_STEPS, |steps| steps.min(MAX_STEPS))
}

fn device_delta(logical: f64, scale: f64) -> Result<i32> {
    let device = (logical * scale).round();
    if !(device.is_finite() && device >= f64::from(i32::MIN) && device <= f64::from(i32::MAX)) {
        return Err(Error::invalid_argument(format!(
            "a scroll of {logical} logical pixels does not fit the screen's range"
        )));
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a whole number checked inside i32's range"
    )]
    Ok(device as i32)
}

fn unsent(capability: &str) -> Error {
    Error::not_supported(
        format!("os_{capability}"),
        format!("no OS lane sends {capability}; use the synthetic path"),
    )
}
