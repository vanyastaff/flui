//! Scripted input, replayed on the virtual clock (ADR-0215 §6).
//!
//! A gesture is a *shape in time*: a long press is a contact that outlives a
//! deadline, a fling is a contact whose samples are close enough together to
//! build velocity, a double tap is two taps inside a window. A
//! [`PointerScript`] is therefore authored data with explicit virtual-time
//! offsets, never wall-clock stamps, and replaying it advances a virtual
//! clock to each offset.
//!
//! Scripts are built per device, with equal standing: [`TouchScript`],
//! [`MouseScript`], [`PenScript`], [`TrackpadScript`] and [`VirtualScript`],
//! each a consuming builder whose [`build`](TouchScript::build) checks that
//! every contact that went down came up (or was cancelled). A frame is a step
//! of its own ([`TouchScript::frame`]), so a script can put several events in
//! one frame, or an event between two vsyncs.
//!
//! A script's output is an owned list of [`HostInput`]
//! ([`PointerScript::inputs`]), the same input a recording holds; it prints as
//! a readable timeline ([`Display`](std::fmt::Display)) and writes as a
//! recording ([`PointerScript::write_recording`]).
//! [`HeadlessHost::run_script`](crate::HeadlessHost::run_script) delivers it to a
//! UI runtime, frames included; [`HeadlessBinding::replay`] drives the raw
//! owners, pumping a frame before each event.
//!
//! ```
//! use std::time::Duration;
//! use flui_foundation::geometry::Offset;
//! use flui_testing::replay::{MouseScript, PointerScript, TouchScript};
//!
//! let swipe: PointerScript = TouchScript::new("swipe")
//!     .down(1, Offset::new(10.0, 10.0))
//!     .wait(Duration::from_millis(8))
//!     .move_to(1, Offset::new(60.0, 10.0))
//!     .up(1)
//!     .build()?;
//! assert_eq!(swipe.inputs().len(), 3);
//!
//! let held = MouseScript::new("held").press(Offset::new(1.0, 1.0)).build();
//! assert!(held.is_err(), "a press that never releases is an authoring mistake");
//! # Ok::<(), flui_testing::replay::ScriptError>(())
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::io::Write;
use std::{num::NonZeroU64, time::Duration};

use flui_foundation::geometry::{Offset, Point};
use flui_interaction::events::{
    PointerKind, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    make_up_event_for_id,
};
use flui_interaction::{HitTestResult, PointerEvent, PointerId};
use flui_platform_api::pointer::{
    CancelReason, DeviceId, PanZoomEvent, PanZoomPhase, PanZoomTransform, PenTool, PointerButton,
    PointerButtons, PointerCancel, PointerInfo, PointerMove, PointerPosition, PointerPress,
    PointerRelease, PointerRole, PointerSample, Pressure, ScrollDelta, ScrollEvent, ScrollPhase,
    ScrollPrecision, ScrollUnit,
};
use flui_platform_api::{EventTime, PlatformInput, WindowId};
use flui_runtime::HostInput;

use crate::HeadlessBinding;

/// Which pointer phase a scripted event carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    /// A contact begins.
    Down,
    /// A tracked contact moves.
    Move,
    /// A contact ends normally.
    Up,
    /// A contact is taken away by the system (a system gesture, a lost window).
    Cancel,
    /// A pointer that is not in contact moves: a hovering mouse or pen.
    Hover,
}

/// One scripted pointer event, at an explicit offset from the script's start.
///
/// The offset is *virtual* time. Nothing here samples a wall clock: a script is
/// authored, so replaying it twice produces the same timing on any machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptedPointer {
    /// Virtual time from the start of the script at which this event is
    /// dispatched.
    pub at: Duration,
    /// Which contact this event belongs to. Distinct contacts must use
    /// distinct ids — a platform never recycles an id into a still-tracked
    /// gesture, and neither should a script.
    pub pointer: PointerId,
    /// The phase.
    pub phase: PointerPhase,
    /// Position in logical pixels.
    pub position: Offset<f64>,
    /// Device kind. Slop thresholds and velocity policy differ per kind, so a
    /// script that means "touch" must say so.
    pub device: PointerKind,
    /// The button a `Down` presses or an `Up` releases, and the one a `Move`
    /// holds.
    pub button: PointerButton,
    /// The contact's pressure, for a device that reports one.
    pub pressure: Option<f32>,
    /// The hardware behind the contact, when the script names it.
    pub hardware: Option<DeviceId>,
}

impl ScriptedPointer {
    /// A scripted event with the touch device kind.
    #[must_use]
    pub fn new(
        at: Duration,
        pointer: PointerId,
        phase: PointerPhase,
        position: Offset<f64>,
    ) -> Self {
        Self {
            at,
            pointer,
            phase,
            position,
            device: PointerKind::Touch,
            button: PointerButton::PRIMARY,
            pressure: None,
            hardware: None,
        }
    }

    /// Same event, attributed to a different device kind.
    #[must_use]
    pub fn with_device(mut self, device: PointerKind) -> Self {
        self.device = device;
        self
    }

    /// The production [`PointerEvent`] this entry scripts, stamped at
    /// `EventTime` zero like every synthetic event built through
    /// `flui_interaction`'s constructors; [`to_event_at`](Self::to_event_at)
    /// stamps the script's own time.
    #[must_use]
    pub fn to_event(self) -> PointerEvent {
        match self.phase {
            PointerPhase::Down => make_down_event_for_id(self.pointer, self.position, self.device)
                .expect("scripted pointer positions must be finite"),
            PointerPhase::Move | PointerPhase::Hover => {
                make_move_event_for_id(self.pointer, self.position, self.device)
                    .expect("scripted pointer positions must be finite")
            }
            PointerPhase::Up => make_up_event_for_id(self.pointer, self.position, self.device)
                .expect("scripted pointer positions must be finite"),
            PointerPhase::Cancel => make_cancel_event_for_id(self.pointer, self.device),
        }
    }

    /// The event this entry scripts, stamped `base + at` on the event
    /// timeline, with its button, pressure and role.
    ///
    /// # Panics
    ///
    /// If the position is not finite or the pressure is out of range.
    #[must_use]
    pub fn to_event_at(self, base: EventTime) -> PointerEvent {
        let time = base + self.at;
        let mut info = PointerInfo::new(self.pointer, self.device).with_role(PointerRole::Primary);
        if let Some(hardware) = self.hardware {
            info = info.with_device(hardware);
        }
        let mut sample = PointerSample::new(
            time,
            PointerPosition::try_new(Point::new(self.position.dx, self.position.dy))
                .expect("scripted pointer positions must be finite"),
        );
        if let Some(pressure) = self.pressure {
            sample = sample
                .with_pressure(Pressure::try_new(pressure).expect("scripted pressure in 0..=1"));
        }
        match self.phase {
            PointerPhase::Down => PointerEvent::Down(PointerPress::new(
                info,
                self.button,
                PointerButtons::NONE,
                sample,
            )),
            PointerPhase::Move => PointerEvent::Move(PointerMove::new(
                info,
                PointerButtons::only(self.button),
                sample,
            )),
            PointerPhase::Hover => {
                PointerEvent::Move(PointerMove::new(info, PointerButtons::NONE, sample))
            }
            PointerPhase::Up => PointerEvent::Up(PointerRelease::new(
                info,
                self.button,
                PointerButtons::only(self.button),
                sample,
            )),
            PointerPhase::Cancel => {
                PointerEvent::Cancel(PointerCancel::new(info, time, CancelReason::Platform))
            }
        }
    }
}

/// A scrolled wheel notch or a trackpad pan step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptedScroll {
    /// Virtual time from the start of the script.
    pub at: Duration,
    /// The scrolling device.
    pub pointer: PointerId,
    /// Its kind.
    pub device: PointerKind,
    /// Where the pointer is.
    pub position: Offset<f64>,
    /// The scroll, in `unit`.
    pub delta: Offset<f64>,
    /// The delta's unit.
    pub unit: ScrollUnit,
    /// Notched or precise.
    pub precision: ScrollPrecision,
    /// Where the step sits in a phased gesture; `None` for a wheel notch.
    pub phase: Option<ScrollPhase>,
}

/// A trackpad pinch or rotation step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptedPanZoom {
    /// Virtual time from the start of the script.
    pub at: Duration,
    /// The gesture's pointer.
    pub pointer: PointerId,
    /// The focal point.
    pub position: Offset<f64>,
    /// Where the gesture is.
    pub phase: PinchPhase,
}

/// Where a scripted trackpad pinch is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PinchPhase {
    /// The pinch begins.
    Start,
    /// The cumulative transform is now `scale` and `rotation` (radians).
    Update {
        /// The cumulative scale.
        scale: f64,
        /// The cumulative rotation, in radians.
        rotation: f64,
    },
    /// The pinch ends.
    End,
}

/// One step of a [`PointerScript`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ScriptStep {
    /// A pointer event.
    Pointer(ScriptedPointer),
    /// A wheel or trackpad scroll.
    Scroll(ScriptedScroll),
    /// A trackpad pinch or rotation.
    PanZoom(ScriptedPanZoom),
    /// A frame, its vsync at `at`.
    Frame {
        /// Virtual time from the start of the script.
        at: Duration,
    },
}

impl ScriptStep {
    /// When the step happens, from the script's start.
    #[must_use]
    pub fn at(&self) -> Duration {
        match self {
            Self::Pointer(step) => step.at,
            Self::Scroll(step) => step.at,
            Self::PanZoom(step) => step.at,
            Self::Frame { at } => *at,
        }
    }

    /// The host input this step delivers, its times counted from `base`.
    ///
    /// # Panics
    ///
    /// If a scripted value is not finite.
    #[must_use]
    pub fn to_input(&self, base: EventTime) -> HostInput {
        let position = |offset: Offset<f64>| {
            PointerPosition::try_new(Point::new(offset.dx, offset.dy))
                .expect("scripted positions must be finite")
        };
        match *self {
            Self::Pointer(step) => {
                HostInput::Platform(PlatformInput::Pointer(step.to_event_at(base)))
            }
            Self::Scroll(step) => {
                let mut event = ScrollEvent::new(
                    PointerInfo::new(step.pointer, step.device).with_role(PointerRole::Primary),
                    base + step.at,
                    position(step.position),
                    ScrollDelta::try_new(step.unit, step.delta.dx, step.delta.dy)
                        .expect("scripted scroll deltas must be finite"),
                )
                .with_precision(step.precision);
                if let Some(phase) = step.phase {
                    event = event.with_phase(phase);
                }
                HostInput::Platform(PlatformInput::Pointer(PointerEvent::Scroll(event)))
            }
            Self::PanZoom(step) => {
                let phase = match step.phase {
                    PinchPhase::Start => PanZoomPhase::Start,
                    PinchPhase::Update { scale, rotation } => PanZoomPhase::Update(
                        PanZoomTransform::try_new(scale, rotation)
                            .expect("scripted transforms must be finite and positive"),
                    ),
                    PinchPhase::End => PanZoomPhase::End,
                };
                HostInput::Platform(PlatformInput::Pointer(PointerEvent::PanZoom(
                    PanZoomEvent::new(
                        PointerInfo::new(step.pointer, PointerKind::Trackpad)
                            .with_role(PointerRole::Primary),
                        base + step.at,
                        position(step.position),
                        phase,
                    ),
                )))
            }
            Self::Frame { at } => HostInput::Frame { vsync: base + at },
        }
    }
}

/// A script that does not describe input a device can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScriptError {
    /// A contact went down and never came up or was cancelled.
    #[error("contact {pointer:?} goes down and never comes up")]
    UnpairedDown {
        /// The contact.
        pointer: PointerId,
    },
    /// A contact moved, came up or was cancelled without going down.
    #[error("contact {pointer:?} moves or ends without going down")]
    NotDown {
        /// The contact.
        pointer: PointerId,
    },
    /// A contact went down while it was already down.
    #[error("contact {pointer:?} goes down twice")]
    AlreadyDown {
        /// The contact.
        pointer: PointerId,
    },
    /// A trackpad gesture step outside a started gesture, or a gesture never
    /// ended.
    #[error("a trackpad pinch step is outside its start and end")]
    UnpairedPinch,
}

/// A time-ordered script of host input, built by a per-device builder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointerScript {
    name: String,
    steps: Vec<ScriptStep>,
}

impl PointerScript {
    /// This script's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The steps, in time order.
    #[must_use]
    pub fn steps(&self) -> &[ScriptStep] {
        &self.steps
    }

    /// The pointer events, in time order.
    #[must_use]
    pub fn pointer_events(&self) -> Vec<ScriptedPointer> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                ScriptStep::Pointer(event) => Some(*event),
                _ => None,
            })
            .collect()
    }

    /// The host input this script delivers, on virtual time from zero.
    #[must_use]
    pub fn inputs(&self) -> Vec<HostInput> {
        self.inputs_from(EventTime::from_nanos(0))
    }

    /// The host input this script delivers, its times counted from `base`.
    #[must_use]
    pub fn inputs_from(&self, base: EventTime) -> Vec<HostInput> {
        self.steps.iter().map(|step| step.to_input(base)).collect()
    }

    /// Number of steps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the script has no steps.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Virtual time spanned, i.e. the last step's offset.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.steps.last().map_or(Duration::ZERO, ScriptStep::at)
    }

    /// Write this script as a recording addressed to `window`: one input row
    /// per step, arriving at its own time, and the `End` row.
    ///
    /// # Errors
    ///
    /// The writer's [`RecordingError`](crate::recording::RecordingError).
    pub fn write_recording<W: Write>(
        &self,
        out: W,
        window: WindowId,
    ) -> Result<W, crate::recording::RecordingError> {
        use crate::recording::{RecordedWindow, RecordingClock, RecordingHeader, RecordingWriter};
        use flui_runtime::{InputRecord, InputRow, owner::InputOutcome};

        let mut header = RecordingHeader::new(RecordingClock::Virtual)
            .with_window(RecordedWindow::new(window, 1.0));
        header.privacy = crate::recording::Privacy::KeepText;
        let mut writer = RecordingWriter::new(out, &header)?;
        for step in &self.steps {
            let t = EventTime::from_nanos(0) + step.at();
            writer.write(&InputRecord::Input(InputRow::new(
                window,
                t,
                t,
                step.to_input(EventTime::from_nanos(0)),
                InputOutcome::Handled,
            )))?;
        }
        writer.write(&InputRecord::End { lost_rows: 0 })?;
        writer.finish()
    }

    // ---------------------------------------------------------------- presets

    /// Down then up at `position`, 50 ms apart — inside any tap window.
    #[must_use]
    pub fn tap(position: Offset<f64>) -> Self {
        TouchScript::new("tap")
            .down(1, position)
            .wait(Duration::from_millis(50))
            .up(1)
            .build()
            .expect("BUG: the tap preset pairs its contact")
    }

    /// Two taps at `position` separated by `gap` between the first up and the
    /// second down, the second on a contact of its own.
    #[must_use]
    pub fn double_tap(position: Offset<f64>, gap: Duration) -> Self {
        TouchScript::new("double_tap")
            .down(1, position)
            .wait(Duration::from_millis(50))
            .up(1)
            .wait(gap)
            .down(2, position)
            .wait(Duration::from_millis(50))
            .up(2)
            .build()
            .expect("BUG: the double-tap preset pairs its contacts")
    }

    /// A stationary contact held for `hold` before lifting.
    #[must_use]
    pub fn long_press(position: Offset<f64>, hold: Duration) -> Self {
        TouchScript::new("long_press")
            .down(1, position)
            .wait(hold)
            .up(1)
            .build()
            .expect("BUG: the long-press preset pairs its contact")
    }

    /// A drag from `start` to `end` in `steps` linearly-interpolated moves,
    /// each `sample` apart; `sample` sets the velocity the gesture carries.
    ///
    /// # Panics
    ///
    /// If `steps` is zero — a drag with no move samples is a tap with a
    /// misleading name.
    #[must_use]
    pub fn drag(start: Offset<f64>, end: Offset<f64>, steps: usize, sample: Duration) -> Self {
        assert!(steps > 0, "a drag needs at least one move sample");
        let mut script = TouchScript::new("drag").down(1, start);
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            script = script.wait(sample).move_to(
                1,
                Offset::new(
                    start.dx + (end.dx - start.dx) * t,
                    start.dy + (end.dy - start.dy) * t,
                ),
            );
        }
        script
            .wait(sample)
            .up(1)
            .build()
            .expect("BUG: the drag preset pairs its contact")
    }

    /// A drag sampled at hardware cadence (8 ms), fast enough to build the
    /// velocity a fling recognizer needs.
    #[must_use]
    pub fn fling(start: Offset<f64>, end: Offset<f64>) -> Self {
        Self::drag(start, end, 6, Duration::from_millis(8)).named("fling")
    }

    /// Alias for [`fling`](Self::fling) under the name a UI test usually gives
    /// it.
    #[must_use]
    pub fn swipe(start: Offset<f64>, end: Offset<f64>) -> Self {
        Self::fling(start, end).named("swipe")
    }

    /// A two-finger pinch about `center`, from `start_distance` to
    /// `end_distance` along the horizontal axis in `steps` samples.
    ///
    /// # Panics
    ///
    /// If `steps` is zero.
    #[must_use]
    pub fn pinch(
        center: Offset<f64>,
        start_distance: f64,
        end_distance: f64,
        steps: usize,
    ) -> Self {
        assert!(steps > 0, "a pinch needs at least one move sample");
        let sample = Duration::from_millis(8);
        let pair = |distance: f64| {
            let half = distance / 2.0;
            (
                Offset::new(center.dx - half, center.dy),
                Offset::new(center.dx + half, center.dy),
            )
        };
        let (left, right) = pair(start_distance);
        // The second finger lands after the first: two simultaneous downs is
        // a shape no real digitizer produces.
        let mut script = TouchScript::new("pinch")
            .down(1, left)
            .wait(sample)
            .down(2, right);
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let (left, right) = pair(start_distance + (end_distance - start_distance) * t);
            script = script.wait(sample).move_to(1, left).move_to(2, right);
        }
        script
            .wait(sample)
            .up(1)
            .up(2)
            .build()
            .expect("BUG: the pinch preset pairs its contacts")
    }

    fn named(mut self, name: &str) -> Self {
        name.clone_into(&mut self.name);
        self
    }
}

impl fmt::Display for PointerScript {
    /// The script as a readable timeline, one step per line:
    ///
    /// ```text
    /// script tap
    ///      0.000 ms  touch #1 down at (10, 10)
    ///     50.000 ms  touch #1 up at (10, 10)
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "script {}", self.name)?;
        for step in &self.steps {
            let ms = step.at().as_secs_f64() * 1000.0;
            write!(f, "{ms:>10.3} ms  ")?;
            match step {
                ScriptStep::Pointer(event) => writeln!(
                    f,
                    "{} #{} {} at ({}, {})",
                    kind_name(event.device),
                    event.pointer.get(),
                    match event.phase {
                        PointerPhase::Down => "down",
                        PointerPhase::Move => "move",
                        PointerPhase::Up => "up",
                        PointerPhase::Cancel => "cancel",
                        PointerPhase::Hover => "hover",
                    },
                    event.position.dx,
                    event.position.dy,
                )?,
                ScriptStep::Scroll(scroll) => writeln!(
                    f,
                    "{} #{} scroll ({}, {}) {:?}{} at ({}, {})",
                    kind_name(scroll.device),
                    scroll.pointer.get(),
                    scroll.delta.dx,
                    scroll.delta.dy,
                    scroll.unit,
                    scroll
                        .phase
                        .map_or_else(String::new, |phase| format!(" {phase:?}")),
                    scroll.position.dx,
                    scroll.position.dy,
                )?,
                ScriptStep::PanZoom(pinch) => match pinch.phase {
                    PinchPhase::Start => writeln!(
                        f,
                        "trackpad pinch start at ({}, {})",
                        pinch.position.dx, pinch.position.dy
                    )?,
                    PinchPhase::Update { scale, rotation } => {
                        writeln!(f, "trackpad pinch scale {scale} rotation {rotation}")?;
                    }
                    PinchPhase::End => writeln!(f, "trackpad pinch end")?,
                },
                ScriptStep::Frame { .. } => writeln!(f, "frame")?,
            }
        }
        Ok(())
    }
}

fn kind_name(kind: PointerKind) -> &'static str {
    match kind {
        PointerKind::Mouse => "mouse",
        PointerKind::Touch => "touch",
        PointerKind::Pen {
            tool: PenTool::Eraser,
        } => "eraser",
        PointerKind::Pen { .. } => "pen",
        PointerKind::Trackpad => "trackpad",
        PointerKind::Virtual => "virtual",
        _ => "pointer",
    }
}

/// The timeline and pairing every per-device builder shares.
#[derive(Debug, Clone)]
struct Timeline {
    name: String,
    now: Duration,
    steps: Vec<ScriptStep>,
    down: BTreeSet<PointerId>,
    pinching: bool,
    error: Option<ScriptError>,
}

impl Timeline {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            now: Duration::ZERO,
            steps: Vec::new(),
            down: BTreeSet::new(),
            pinching: false,
            error: None,
        }
    }

    fn fail(&mut self, error: ScriptError) {
        self.error.get_or_insert(error);
    }

    fn pointer(&mut self, mut event: ScriptedPointer) {
        event.at = self.now;
        match event.phase {
            PointerPhase::Down => {
                if !self.down.insert(event.pointer) {
                    self.fail(ScriptError::AlreadyDown {
                        pointer: event.pointer,
                    });
                }
            }
            PointerPhase::Move => {
                if !self.down.contains(&event.pointer) {
                    self.fail(ScriptError::NotDown {
                        pointer: event.pointer,
                    });
                }
            }
            PointerPhase::Up | PointerPhase::Cancel => {
                if !self.down.remove(&event.pointer) {
                    self.fail(ScriptError::NotDown {
                        pointer: event.pointer,
                    });
                }
            }
            PointerPhase::Hover => {}
        }
        self.steps.push(ScriptStep::Pointer(event));
    }

    fn last_position(&self, pointer: PointerId) -> Offset<f64> {
        self.steps
            .iter()
            .rev()
            .find_map(|step| match step {
                ScriptStep::Pointer(event) if event.pointer == pointer => Some(event.position),
                _ => None,
            })
            .unwrap_or(Offset::new(0.0, 0.0))
    }

    fn last_button(&self, pointer: PointerId) -> PointerButton {
        self.steps
            .iter()
            .rev()
            .find_map(|step| match step {
                ScriptStep::Pointer(event)
                    if event.pointer == pointer && event.phase == PointerPhase::Down =>
                {
                    Some(event.button)
                }
                _ => None,
            })
            .unwrap_or(PointerButton::PRIMARY)
    }

    fn frame(&mut self) {
        self.steps.push(ScriptStep::Frame { at: self.now });
    }

    fn build(mut self) -> Result<PointerScript, ScriptError> {
        if let Some(pointer) = self.down.first() {
            self.fail(ScriptError::UnpairedDown { pointer: *pointer });
        }
        if self.pinching {
            self.fail(ScriptError::UnpairedPinch);
        }
        match self.error {
            Some(error) => Err(error),
            None => Ok(PointerScript {
                name: self.name,
                steps: self.steps,
            }),
        }
    }
}

fn contact(id: u64) -> PointerId {
    PointerId::new(NonZeroU64::new(id).unwrap_or(NonZeroU64::MIN))
}

fn event(
    pointer: PointerId,
    phase: PointerPhase,
    at: Offset<f64>,
    device: PointerKind,
) -> ScriptedPointer {
    ScriptedPointer::new(Duration::ZERO, pointer, phase, at).with_device(device)
}

macro_rules! timing {
    () => {
        /// Let `duration` of virtual time pass before the next step.
        pub fn wait(mut self, duration: Duration) -> Self {
            self.timeline.now += duration;
            self
        }

        /// A frame at the current time: its vsync paces resampled motion and
        /// fires due deadlines. Steps between two frames share one frame.
        pub fn frame(mut self) -> Self {
            self.timeline.frame();
            self
        }

        /// The script, once every contact that went down came up.
        ///
        /// # Errors
        ///
        /// The first [`ScriptError`] the steps made.
        pub fn build(self) -> Result<PointerScript, ScriptError> {
            self.timeline.build()
        }
    };
}

/// Fingers on a touch screen. Fingers are numbered; each number is one
/// contact while it is down.
#[derive(Debug, Clone)]
#[must_use]
pub struct TouchScript {
    timeline: Timeline,
}

impl TouchScript {
    /// An empty touch script under `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            timeline: Timeline::new(name),
        }
    }

    /// Finger `finger` touches at `at`.
    pub fn down(mut self, finger: u64, at: Offset<f64>) -> Self {
        self.timeline.pointer(event(
            contact(finger),
            PointerPhase::Down,
            at,
            PointerKind::Touch,
        ));
        self
    }

    /// Finger `finger` moves to `at`.
    pub fn move_to(mut self, finger: u64, at: Offset<f64>) -> Self {
        self.timeline.pointer(event(
            contact(finger),
            PointerPhase::Move,
            at,
            PointerKind::Touch,
        ));
        self
    }

    /// Finger `finger` lifts where it is.
    pub fn up(mut self, finger: u64) -> Self {
        let at = self.timeline.last_position(contact(finger));
        self.timeline.pointer(event(
            contact(finger),
            PointerPhase::Up,
            at,
            PointerKind::Touch,
        ));
        self
    }

    /// The system takes finger `finger` away.
    pub fn cancel(mut self, finger: u64) -> Self {
        let at = self.timeline.last_position(contact(finger));
        self.timeline.pointer(event(
            contact(finger),
            PointerPhase::Cancel,
            at,
            PointerKind::Touch,
        ));
        self
    }

    timing!();
}

/// The mouse: one cursor, its buttons and its wheel.
#[derive(Debug, Clone)]
#[must_use]
pub struct MouseScript {
    timeline: Timeline,
    kind: PointerKind,
    device: Option<DeviceId>,
}

/// The mouse's contact id in a script.
const CURSOR: u64 = 1;

impl MouseScript {
    /// An empty mouse script under `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            timeline: Timeline::new(name),
            kind: PointerKind::Mouse,
            device: None,
        }
    }

    fn pressed(&self) -> bool {
        self.timeline.down.contains(&contact(CURSOR))
    }

    /// The cursor moves to `at`, dragging when a button is held.
    pub fn move_to(mut self, at: Offset<f64>) -> Self {
        let phase = if self.pressed() {
            PointerPhase::Move
        } else {
            PointerPhase::Hover
        };
        let button = self.timeline.last_button(contact(CURSOR));
        let mut step = self.event(phase, at);
        step.button = button;
        self.timeline.pointer(step);
        self
    }

    fn event(&self, phase: PointerPhase, at: Offset<f64>) -> ScriptedPointer {
        let mut step = event(contact(CURSOR), phase, at, self.kind);
        step.hardware = self.device;
        step
    }

    /// The primary button goes down at `at`.
    pub fn press(self, at: Offset<f64>) -> Self {
        self.press_button(at, PointerButton::PRIMARY)
    }

    /// `button` goes down at `at`.
    pub fn press_button(mut self, at: Offset<f64>, button: PointerButton) -> Self {
        let mut step = self.event(PointerPhase::Down, at);
        step.button = button;
        self.timeline.pointer(step);
        self
    }

    /// The held button comes up where the cursor is.
    pub fn release(mut self) -> Self {
        let at = self.timeline.last_position(contact(CURSOR));
        let button = self.timeline.last_button(contact(CURSOR));
        let mut step = self.event(PointerPhase::Up, at);
        step.button = button;
        self.timeline.pointer(step);
        self
    }

    /// A primary click at `at`: press and release with no time between.
    pub fn click(self, at: Offset<f64>) -> Self {
        self.press(at).release()
    }

    /// One wheel notch of `lines` at the cursor (positive scrolls down).
    pub fn wheel(mut self, lines: f64) -> Self {
        let position = self.timeline.last_position(contact(CURSOR));
        let at = self.timeline.now;
        let pointer = contact(CURSOR);
        self.timeline.steps.push(ScriptStep::Scroll(ScriptedScroll {
            at,
            pointer,
            device: self.kind,
            position,
            delta: Offset::new(0.0, lines),
            unit: ScrollUnit::Lines,
            precision: ScrollPrecision::Notched,
            phase: None,
        }));
        self
    }

    timing!();
}

/// A pointer with no physical contact — a gamepad cursor, UI automation —
/// scripted like a mouse, with its own device id.
#[derive(Debug, Clone)]
#[must_use]
pub struct VirtualScript {
    mouse: MouseScript,
}

impl VirtualScript {
    /// An empty virtual-pointer script under `name`, for device `device`.
    pub fn new(name: impl Into<String>, device: DeviceId) -> Self {
        let mut mouse = MouseScript::new(name);
        mouse.kind = PointerKind::Virtual;
        mouse.device = Some(device);
        Self { mouse }
    }

    /// The device this script's pointer belongs to.
    #[must_use]
    pub fn device(&self) -> Option<DeviceId> {
        self.mouse.device
    }

    /// The pointer moves to `at`.
    pub fn move_to(self, at: Offset<f64>) -> Self {
        Self {
            mouse: self.mouse.move_to(at),
        }
    }

    /// The pointer activates at `at`.
    pub fn press(self, at: Offset<f64>) -> Self {
        Self {
            mouse: self.mouse.press(at),
        }
    }

    /// The pointer activates `button` at `at`: a dwell device's right click
    /// is [`PointerButton::SECONDARY`].
    pub fn press_button(self, at: Offset<f64>, button: PointerButton) -> Self {
        Self {
            mouse: self.mouse.press_button(at, button),
        }
    }

    /// The pointer deactivates where it is.
    pub fn release(self) -> Self {
        Self {
            mouse: self.mouse.release(),
        }
    }

    /// Let `duration` of virtual time pass before the next step.
    pub fn wait(self, duration: Duration) -> Self {
        Self {
            mouse: self.mouse.wait(duration),
        }
    }

    /// A frame at the current time.
    pub fn frame(self) -> Self {
        Self {
            mouse: self.mouse.frame(),
        }
    }

    /// The script, once the pointer released what it pressed.
    ///
    /// # Errors
    ///
    /// The first [`ScriptError`] the steps made.
    pub fn build(self) -> Result<PointerScript, ScriptError> {
        self.mouse.build()
    }
}

/// A pen: its tip or eraser hovers, touches with a pressure and lifts.
#[derive(Debug, Clone)]
#[must_use]
pub struct PenScript {
    timeline: Timeline,
    kind: PointerKind,
}

impl PenScript {
    /// An empty pen script under `name`, writing with the tip.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            timeline: Timeline::new(name),
            kind: PointerKind::Pen { tool: PenTool::Tip },
        }
    }

    /// The pen uses its eraser end from now on.
    pub fn eraser(mut self) -> Self {
        self.kind = PointerKind::Pen {
            tool: PenTool::Eraser,
        };
        self
    }

    /// The pen hovers at `at`, out of contact.
    pub fn hover(mut self, at: Offset<f64>) -> Self {
        self.timeline
            .pointer(event(contact(CURSOR), PointerPhase::Hover, at, self.kind));
        self
    }

    /// The pen touches at `at` with `pressure` (0 to 1).
    pub fn down(mut self, at: Offset<f64>, pressure: f32) -> Self {
        let mut step = event(contact(CURSOR), PointerPhase::Down, at, self.kind);
        step.pressure = Some(pressure);
        self.timeline.pointer(step);
        self
    }

    /// The pen moves to `at` in contact with `pressure`.
    pub fn move_to(mut self, at: Offset<f64>, pressure: f32) -> Self {
        let mut step = event(contact(CURSOR), PointerPhase::Move, at, self.kind);
        step.pressure = Some(pressure);
        self.timeline.pointer(step);
        self
    }

    /// The pen lifts where it is.
    pub fn up(mut self) -> Self {
        let at = self.timeline.last_position(contact(CURSOR));
        self.timeline
            .pointer(event(contact(CURSOR), PointerPhase::Up, at, self.kind));
        self
    }

    timing!();
}

/// A trackpad: two-finger pans (phased scrolls) and pinches.
#[derive(Debug, Clone)]
#[must_use]
pub struct TrackpadScript {
    timeline: Timeline,
    at: Offset<f64>,
}

impl TrackpadScript {
    /// An empty trackpad script under `name`, its gestures focused on `at`.
    pub fn new(name: impl Into<String>, at: Offset<f64>) -> Self {
        Self {
            timeline: Timeline::new(name),
            at,
        }
    }

    fn scroll(mut self, delta: Offset<f64>, phase: ScrollPhase) -> Self {
        let at = self.timeline.now;
        self.timeline.steps.push(ScriptStep::Scroll(ScriptedScroll {
            at,
            pointer: contact(CURSOR),
            device: PointerKind::Trackpad,
            position: self.at,
            delta,
            unit: ScrollUnit::Pixels,
            precision: ScrollPrecision::Precise,
            phase: Some(phase),
        }));
        self
    }

    /// A two-finger pan of `delta` in `steps` phased scroll events, `sample`
    /// apart, between its `Began` and `Ended` events.
    pub fn pan(self, delta: Offset<f64>, steps: u32, sample: Duration) -> Self {
        self.unended_pan(delta, steps, sample)
            .scroll(Offset::new(0.0, 0.0), ScrollPhase::Ended)
    }

    /// A pan as [`pan`](Self::pan) whose `Ended` event never comes: a
    /// platform that may begin a phased scroll without ending it (Wayland).
    pub fn unended_pan(mut self, delta: Offset<f64>, steps: u32, sample: Duration) -> Self {
        self = self.scroll(Offset::new(0.0, 0.0), ScrollPhase::Began);
        let steps = steps.max(1);
        let each = Offset::new(delta.dx / f64::from(steps), delta.dy / f64::from(steps));
        for _ in 0..steps {
            self = self.wait(sample).scroll(each, ScrollPhase::Changed);
        }
        self
    }

    /// The platform's inertia after a pan: `delta` in `steps` momentum
    /// events, `sample` apart, between its `MomentumBegan` and
    /// `MomentumEnded` events.
    pub fn momentum(mut self, delta: Offset<f64>, steps: u32, sample: Duration) -> Self {
        self = self.scroll(Offset::new(0.0, 0.0), ScrollPhase::MomentumBegan);
        let steps = steps.max(1);
        let each = Offset::new(delta.dx / f64::from(steps), delta.dy / f64::from(steps));
        for _ in 0..steps {
            self = self.wait(sample).scroll(each, ScrollPhase::MomentumChanged);
        }
        self.scroll(Offset::new(0.0, 0.0), ScrollPhase::MomentumEnded)
    }

    /// The pinch begins.
    pub fn pinch_start(mut self) -> Self {
        if self.timeline.pinching {
            self.timeline.fail(ScriptError::UnpairedPinch);
        }
        self.timeline.pinching = true;
        self.pan_zoom(PinchPhase::Start)
    }

    /// The pinch's cumulative transform becomes `scale` and `rotation`.
    pub fn pinch_to(mut self, scale: f64, rotation: f64) -> Self {
        if !self.timeline.pinching {
            self.timeline.fail(ScriptError::UnpairedPinch);
        }
        self.pan_zoom(PinchPhase::Update { scale, rotation })
    }

    /// The pinch ends.
    pub fn pinch_end(mut self) -> Self {
        if !self.timeline.pinching {
            self.timeline.fail(ScriptError::UnpairedPinch);
        }
        self.timeline.pinching = false;
        self.pan_zoom(PinchPhase::End)
    }

    fn pan_zoom(mut self, phase: PinchPhase) -> Self {
        let at = self.timeline.now;
        self.timeline
            .steps
            .push(ScriptStep::PanZoom(ScriptedPanZoom {
                at,
                pointer: contact(CURSOR),
                position: self.at,
                phase,
            }));
        self
    }

    timing!();
}

macro_rules! into_script {
    ($($builder:ty),*) => {
        $(
            impl TryFrom<$builder> for PointerScript {
                type Error = ScriptError;

                fn try_from(builder: $builder) -> Result<Self, ScriptError> {
                    builder.build()
                }
            }
        )*
    };
}

into_script!(
    TouchScript,
    MouseScript,
    PenScript,
    TrackpadScript,
    VirtualScript
);

impl HeadlessBinding {
    /// Replay `script` against this binding's own tree.
    ///
    /// See [`replay_with`](Self::replay_with) for the mechanics — this is that
    /// method over [`hit_test`](Self::hit_test).
    pub fn replay(&mut self, script: &PointerScript) {
        self.replay_with(script, Self::hit_test);
    }

    /// [`replay`](Self::replay) with a caller-supplied hit-test source, for a
    /// harness that hit-tests through its own view of the render tree.
    ///
    /// For each step, in order, a frame is pumped by the gap since the
    /// previous step, so virtual time genuinely passes — deadlines fire,
    /// controllers tick, and the tree rebuilds — then the step's event is
    /// dispatched through the same `GestureBinding` pipeline production
    /// routes through, stamped with the binding's virtual time. A frame step
    /// pumps only. The frame *after* the last event is left to the caller.
    ///
    /// # Panics
    ///
    /// If pointer resampling is enabled on this binding: its sampling window
    /// is not on the script's virtual time.
    pub fn replay_with(
        &mut self,
        script: &PointerScript,
        mut hit_test: impl FnMut(&Self, Offset<f64>, PointerKind) -> HitTestResult,
    ) {
        assert!(
            !self.gestures().is_resampling_enabled(),
            "replay requires pointer resampling to be disabled: the resampler's \
             sampling window is not on the script's virtual time",
        );
        let base = EventTime::from_nanos(
            u64::try_from(self.clock().elapsed().as_nanos())
                .expect("BUG: a headless test's virtual time fits the event timeline"),
        );
        let mut previous = Duration::ZERO;
        for step in script.steps() {
            self.pump_frame(step.at().saturating_sub(previous));
            previous = step.at();
            let HostInput::Platform(PlatformInput::Pointer(event)) = step.to_input(base) else {
                continue;
            };
            let binding: &Self = self;
            let kind = flui_interaction::PointerEventExt::pointer_kind(&event)
                .unwrap_or(PointerKind::Unknown);
            binding.dispatch_pointer(&event, |position| hit_test(binding, position, kind));
        }
    }

    /// Replay one scripted event: a frame by `event.at`, then the event,
    /// stamped with the binding's virtual time.
    pub fn replay_event(&mut self, event: ScriptedPointer) {
        self.pump_frame(event.at);
        let base = EventTime::from_nanos(
            u64::try_from(self.clock().elapsed().as_nanos())
                .expect("BUG: a headless test's virtual time fits the event timeline"),
        );
        let event = ScriptedPointer {
            at: Duration::ZERO,
            ..event
        }
        .to_event_at(base);
        let binding: &Self = self;
        let kind =
            flui_interaction::PointerEventExt::pointer_kind(&event).unwrap_or(PointerKind::Unknown);
        binding.dispatch_pointer(&event, |position| binding.hit_test(position, kind));
    }

    /// Hit-test `position` against this binding's bound render tree for a
    /// pointer of `kind`.
    ///
    /// An empty result on a gesture-only binding: there is no tree to hit.
    #[must_use]
    pub fn hit_test(&self, position: Offset<f64>, kind: PointerKind) -> HitTestResult {
        let mut result = HitTestResult::new();
        if let Some(pipeline_owner) = self.pipeline_owner() {
            pipeline_owner.with(|owner| owner.hit_test(position, kind, &mut result));
        }
        result
    }
}
