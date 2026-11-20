//! The mouse as the Quartz mouse events the macOS lane posts, planned against what the lane holds,
//! and its wheel.
//!
//! A move while a button is held is a drag of that button, as AppKit reports one; a press and a
//! release land where the pointer last moved, since an event posted to a process does not move
//! the system's cursor. The mouse is the one pointer the lane sends: Quartz has no touch event,
//! and a pen goes out as tablet mouse events, which FLUI's AppKit host reads as the mouse (the
//! `pen-and-stylus` row of `crates/flui-native/platform-parity.toml`), so the lane refuses touch
//! and the pen rather than send input the app would read as another device.
//!
//! Positions are global display points, top-left origin, the space Quartz events take on every
//! display.
//!
//! What the lane holds is recorded once the event that changes it was posted: a press that could
//! not be posted is not released later, and a release that could not be is tried again by
//! [`Pointer::release_all`].

use flui_protocol::input::{PointerButton, PointerDevice};

use crate::error::{Error, Result};
use crate::lane::macos::safety::{LANE, Refusal};

/// A point in global display points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Rightwards.
    pub x: f64,
    /// Downwards.
    pub y: f64,
}

impl Point {
    /// The point `(x, y)`.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A Quartz mouse button: `kCGMouseButtonLeft` 0, right 1, then the others by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseButton(pub u32);

impl MouseButton {
    /// The left button.
    pub const LEFT: Self = Self(0);
    /// The right button.
    pub const RIGHT: Self = Self(1);

    /// The button `button` is on a mouse whose primary button is its left one: macOS swaps the
    /// buttons before an app sees them, so the event names the primary one as left.
    ///
    /// # Errors
    ///
    /// `not_supported` naming the button for one no mouse event carries.
    pub fn of(button: PointerButton) -> Result<Self> {
        match button {
            PointerButton::Primary => Ok(Self::LEFT),
            PointerButton::Secondary => Ok(Self::RIGHT),
            PointerButton::Middle => Ok(Self(2)),
            PointerButton::Back => Ok(Self(3)),
            PointerButton::Forward => Ok(Self(4)),
            _ => Err(Refusal::UnsentInput {
                what: format!("the mouse button {button:?}"),
            }
            .into_error()),
        }
    }

    fn bit(self) -> u32 {
        1 << self.0.min(31)
    }
}

/// What a mouse event does: its `CGEventType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseKind {
    /// `kCGEventMouseMoved`.
    Moved,
    /// A drag of the button: `kCGEventLeftMouseDragged`, right or other.
    Dragged(MouseButton),
    /// A press: `kCGEventLeftMouseDown`, right or other.
    Down(MouseButton),
    /// A release: `kCGEventLeftMouseUp`, right or other.
    Up(MouseButton),
}

/// One mouse event to post.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MouseEvent {
    /// What it does.
    pub kind: MouseKind,
    /// Where, in global display points.
    pub at: Point,
}

impl MouseEvent {
    const fn new(kind: MouseKind, at: Point) -> Self {
        Self { kind, at }
    }
}

/// What one pointer step does, in the lane's terms.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Step {
    /// Move the pointer of `device` to `to`.
    Move {
        /// The device.
        device: PointerDevice,
        /// Where.
        to: Point,
    },
    /// Press `button` of `device`.
    Down {
        /// The device.
        device: PointerDevice,
        /// The button.
        button: PointerButton,
    },
    /// Release `button` of `device`.
    Up {
        /// The device.
        device: PointerDevice,
        /// The button.
        button: PointerButton,
    },
}

/// The mouse as the OS holds it for the lane: see the [module docs](self).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pointer {
    /// Where the mouse last moved.
    at: Option<Point>,
    /// The mouse buttons down, as bits by button number.
    buttons: u32,
}

fn unsent(what: &str) -> Error {
    Refusal::UnsentInput {
        what: what.to_owned(),
    }
    .into_error()
}

impl Pointer {
    /// Nothing moved, nothing held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the mouse is, if the lane placed it.
    #[must_use]
    pub fn position(&self) -> Option<Point> {
        self.at
    }

    /// The mouse buttons down, as bits by button number.
    #[must_use]
    pub fn buttons(&self) -> u32 {
        self.buttons
    }

    /// The events `step` posts from this state.
    ///
    /// # Errors
    ///
    /// `not_supported` for touch and the pen (see the [module docs](self)), for a button no mouse
    /// event carries, and for a press or a release the mouse is in no state for: before it moved
    /// anywhere, of a button already down or not down.
    pub fn plan(&self, step: Step) -> Result<Vec<MouseEvent>> {
        let device = match step {
            Step::Move { device, .. } | Step::Down { device, .. } | Step::Up { device, .. } => {
                device
            }
        };
        if device != PointerDevice::Mouse {
            return Err(unsent(&format!(
                "{} input: Quartz has no touch event, and FLUI's AppKit host reads a pen's \
                 tablet events as the mouse",
                device.name()
            )));
        }
        match step {
            Step::Move { to, .. } => Ok(vec![MouseEvent::new(self.drag_kind(), to)]),
            Step::Down { button, .. } => {
                let button = MouseButton::of(button)?;
                let at = self
                    .at
                    .ok_or_else(|| unsent("a mouse press before the pointer moved anywhere"))?;
                if self.buttons & button.bit() != 0 {
                    return Err(unsent("a press of a mouse button already down"));
                }
                Ok(vec![MouseEvent::new(MouseKind::Down(button), at)])
            }
            Step::Up { button, .. } => {
                let button = MouseButton::of(button)?;
                let at = self
                    .at
                    .ok_or_else(|| unsent("a mouse release before the pointer moved anywhere"))?;
                if self.buttons & button.bit() == 0 {
                    return Err(unsent("a release of a mouse button that is not down"));
                }
                Ok(vec![MouseEvent::new(MouseKind::Up(button), at)])
            }
        }
    }

    /// The kind of a mouse move from this state: a drag of the lowest button held.
    fn drag_kind(&self) -> MouseKind {
        if self.buttons == 0 {
            MouseKind::Moved
        } else {
            MouseKind::Dragged(MouseButton(self.buttons.trailing_zeros()))
        }
    }

    /// Record `event`, which the lane posted.
    pub fn apply(&mut self, event: &MouseEvent) {
        match event.kind {
            MouseKind::Moved | MouseKind::Dragged(_) => self.at = Some(event.at),
            MouseKind::Down(button) => self.buttons |= button.bit(),
            MouseKind::Up(button) => self.buttons &= !button.bit(),
        }
    }

    /// Post each of `events` through `post`, recording each once it went out.
    ///
    /// # Errors
    ///
    /// The first event `post` could not post; the ones before it are recorded.
    pub fn deliver(
        &mut self,
        events: &[MouseEvent],
        mut post: impl FnMut(&MouseEvent) -> Result<()>,
    ) -> Result<()> {
        for event in events {
            post(event)?;
            self.apply(event);
        }
        Ok(())
    }

    /// Release every mouse button held, through `post`; what could not be posted stays held.
    ///
    /// # Errors
    ///
    /// After every button was tried: [`Error::ReleaseFailed`] naming the buttons still held and
    /// the first release that could not be posted, or that release's error when nothing stayed
    /// held.
    pub fn release_all(&mut self, mut post: impl FnMut(&MouseEvent) -> Result<()>) -> Result<()> {
        let mut first: Option<Error> = None;
        if let Some(at) = self.at {
            for number in 0..32_u32 {
                let button = MouseButton(number);
                if self.buttons & button.bit() == 0 {
                    continue;
                }
                let event = MouseEvent::new(MouseKind::Up(button), at);
                match post(&event) {
                    Ok(()) => self.apply(&event),
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
        }
        let held = self.held();
        match first {
            None => Ok(()),
            Some(first) if held.is_empty() => Err(first),
            Some(first) => Err(Error::ReleaseFailed {
                lane: LANE.to_owned(),
                held,
                message: first.to_string(),
            }),
        }
    }

    /// The buttons the lane still holds, by name, for people.
    fn held(&self) -> Vec<String> {
        (0..32_u32)
            .filter(|&number| self.buttons & MouseButton(number).bit() != 0)
            .map(|number| match number {
                0 => "the left mouse button".to_owned(),
                1 => "the right mouse button".to_owned(),
                number => format!("mouse button {number}"),
            })
            .collect()
    }
}

/// What the wheel turns for scrolls of whole device pixels, in the points a Quartz pixel-unit
/// scroll event counts (`kCGScrollEventUnitPixel` deltas are points, as AppKit's
/// `scrollingDeltaX` reads them), carrying the part of a point each turn leaves into the next, so
/// that small scrolls add up and a scroll that adds up to no whole point yet turns nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Wheel {
    /// The part of a point not turned yet, rightwards and downwards.
    carried: (f64, f64),
}

impl Wheel {
    /// A wheel that has carried nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The turn of a scroll of `delta_x` rightwards and `delta_y` downwards device pixels at
    /// `scale`, as `CGEventCreateScrollWheelEvent2` takes it: the first wheel vertical, the
    /// second horizontal, each positive towards the top and the left, so each is the scroll in
    /// points negated. The part of a point left over is carried into the next turn.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a scale that is not positive and finite, or a turn past what an
    /// `i32` counts; nothing is carried then.
    pub fn turn(&mut self, delta_x: i32, delta_y: i32, scale: f64) -> Result<(i32, i32)> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(Error::invalid_argument(format!(
                "the window's scale {scale} is not a positive number"
            )));
        }
        let axis = |delta: i32, carried: f64| -> Result<(i32, f64)> {
            let total = f64::from(delta) / scale + carried;
            let whole = total.trunc();
            if !(whole.is_finite() && whole > f64::from(i32::MIN) && whole <= f64::from(i32::MAX)) {
                return Err(Error::invalid_argument(format!(
                    "a scroll of {delta} pixels at scale {scale} is past what Quartz takes"
                )));
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a whole number checked inside i32's range, its negation included"
            )]
            let points = whole as i32;
            Ok((points, total - whole))
        };
        let (right, carried_x) = axis(delta_x, self.carried.0)?;
        let (down, carried_y) = axis(delta_y, self.carried.1)?;
        self.carried = (carried_x, carried_y);
        Ok((-down, -right))
    }
}
