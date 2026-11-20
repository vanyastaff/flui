//! The mouse and the pen as the Quartz mouse events the macOS lane posts, planned against what the
//! lane holds.
//!
//! macOS has no pointer device of its own for injected input: a pen is a mouse event whose subtype
//! says it came from a tablet and which carries the tablet's fields (pressure), framed by
//! proximity events when the pen comes into and leaves range. A move while a button is held is a
//! drag of that button, as AppKit reports one; a press and a release land where the pointer last
//! moved, since an event posted to a process does not move the system's cursor. Touch has no
//! Quartz event at all, so the lane does not send it.
//!
//! Positions are global display points, top-left origin, the space Quartz events take on every
//! display.
//!
//! What the lane holds is recorded once the event that changes it was posted: a press that could
//! not be posted is not released later, and a release that could not be is tried again by
//! [`Pointer::release_all`].

use flui_protocol::input::{PointerButton, PointerDevice};

use crate::error::{Error, Result};
use crate::lane::macos::safety::Refusal;

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
    /// `kCGEventTabletProximity`: the pen comes into range, or leaves it.
    Proximity {
        /// Whether it enters range.
        entering: bool,
    },
}

/// Where a mouse event comes from: its `kCGMouseEventSubtype`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Subtype {
    /// A mouse.
    Mouse,
    /// A tablet's pen, carrying its pressure.
    TabletPoint,
}

/// One mouse event to post.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MouseEvent {
    /// What it does.
    pub kind: MouseKind,
    /// Where, in global display points.
    pub at: Point,
    /// What sent it.
    pub subtype: Subtype,
    /// The pen's pressure, 0 to 1; 0 for a mouse.
    pub pressure: f64,
}

impl MouseEvent {
    fn mouse(kind: MouseKind, at: Point) -> Self {
        Self {
            kind,
            at,
            subtype: Subtype::Mouse,
            pressure: 0.0,
        }
    }

    fn pen(kind: MouseKind, at: Point, pressure: f64) -> Self {
        Self {
            kind,
            at,
            subtype: Subtype::TabletPoint,
            pressure,
        }
    }
}

/// The pressure of a pen in contact: the lane's steps carry none, so a contact is a firm one.
pub const PEN_PRESSURE: f64 = 1.0;

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

/// The mouse and pen as the OS holds them for the lane: see the [module docs](self).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pointer {
    /// Where the mouse last moved.
    mouse_at: Option<Point>,
    /// The mouse buttons down, as bits by button number.
    buttons: u32,
    /// The pen, while it is in range.
    pen: Option<Pen>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pen {
    at: Point,
    down: bool,
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

    /// Where the pointer of `device` is, if the lane placed it.
    #[must_use]
    pub fn position(&self, device: PointerDevice) -> Option<Point> {
        match device {
            PointerDevice::Pen => self.pen.map(|pen| pen.at),
            _ => self.mouse_at,
        }
    }

    /// The mouse buttons down, as bits by button number.
    #[must_use]
    pub fn buttons(&self) -> u32 {
        self.buttons
    }

    /// Whether the pen is down.
    #[must_use]
    pub fn pen_down(&self) -> bool {
        self.pen.is_some_and(|pen| pen.down)
    }

    /// The events `step` posts from this state.
    ///
    /// # Errors
    ///
    /// `not_supported` for touch, for a button a pen or a mouse event does not carry, and for a
    /// press or a release the pointer is in no state for: before it moved anywhere, of a button
    /// already down or not down.
    pub fn plan(&self, step: Step) -> Result<Vec<MouseEvent>> {
        match step {
            Step::Move {
                device: PointerDevice::Pen,
                to,
            } => Ok(match self.pen {
                None => vec![
                    MouseEvent::pen(MouseKind::Proximity { entering: true }, to, 0.0),
                    MouseEvent::pen(MouseKind::Moved, to, 0.0),
                ],
                Some(Pen { down: true, .. }) => vec![MouseEvent::pen(
                    MouseKind::Dragged(MouseButton::LEFT),
                    to,
                    PEN_PRESSURE,
                )],
                Some(Pen { down: false, .. }) => vec![MouseEvent::pen(MouseKind::Moved, to, 0.0)],
            }),
            Step::Down {
                device: PointerDevice::Pen,
                button,
            } => {
                if button != PointerButton::Primary {
                    return Err(unsent(&format!("the {button:?} button of a pen")));
                }
                let pen = self
                    .pen
                    .ok_or_else(|| unsent("a pen press with no position"))?;
                if pen.down {
                    return Err(unsent("a pen press while the pen is down"));
                }
                Ok(vec![MouseEvent::pen(
                    MouseKind::Down(MouseButton::LEFT),
                    pen.at,
                    PEN_PRESSURE,
                )])
            }
            Step::Up {
                device: PointerDevice::Pen,
                button,
            } => {
                if button != PointerButton::Primary {
                    return Err(unsent(&format!("the {button:?} button of a pen")));
                }
                let pen = self
                    .pen
                    .filter(|pen| pen.down)
                    .ok_or_else(|| unsent("a release of a pen that is not down"))?;
                Ok(vec![MouseEvent::pen(
                    MouseKind::Up(MouseButton::LEFT),
                    pen.at,
                    0.0,
                )])
            }
            Step::Move {
                device: PointerDevice::Mouse,
                to,
            } => Ok(vec![MouseEvent::mouse(self.drag_kind(), to)]),
            Step::Down {
                device: PointerDevice::Mouse,
                button,
            } => {
                let button = MouseButton::of(button)?;
                let at = self
                    .mouse_at
                    .ok_or_else(|| unsent("a mouse press before the pointer moved anywhere"))?;
                if self.buttons & button.bit() != 0 {
                    return Err(unsent("a press of a mouse button already down"));
                }
                Ok(vec![MouseEvent::mouse(MouseKind::Down(button), at)])
            }
            Step::Up {
                device: PointerDevice::Mouse,
                button,
            } => {
                let button = MouseButton::of(button)?;
                let at = self
                    .mouse_at
                    .ok_or_else(|| unsent("a mouse release before the pointer moved anywhere"))?;
                if self.buttons & button.bit() == 0 {
                    return Err(unsent("a release of a mouse button that is not down"));
                }
                Ok(vec![MouseEvent::mouse(MouseKind::Up(button), at)])
            }
            Step::Move { device, .. } | Step::Down { device, .. } | Step::Up { device, .. } => {
                Err(unsent(&format!(
                    "{} input: Quartz has no event for it",
                    device.name()
                )))
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
        match (event.subtype, event.kind) {
            (Subtype::TabletPoint, MouseKind::Proximity { entering: true }) => {
                self.pen = Some(Pen {
                    at: event.at,
                    down: false,
                });
            }
            (Subtype::TabletPoint, MouseKind::Proximity { entering: false }) => self.pen = None,
            (Subtype::TabletPoint, MouseKind::Moved | MouseKind::Dragged(_)) => {
                if let Some(pen) = &mut self.pen {
                    pen.at = event.at;
                }
            }
            (Subtype::TabletPoint, MouseKind::Down(_)) => {
                if let Some(pen) = &mut self.pen {
                    pen.down = true;
                }
            }
            (Subtype::TabletPoint, MouseKind::Up(_)) => {
                if let Some(pen) = &mut self.pen {
                    pen.down = false;
                }
            }
            (Subtype::Mouse, MouseKind::Moved | MouseKind::Dragged(_)) => {
                self.mouse_at = Some(event.at);
            }
            (Subtype::Mouse, MouseKind::Down(button)) => self.buttons |= button.bit(),
            (Subtype::Mouse, MouseKind::Up(button)) => self.buttons &= !button.bit(),
            (Subtype::Mouse, MouseKind::Proximity { .. }) => {}
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

    /// Release every mouse button held and lift the pen and take it out of range, through
    /// `post`; what could not be posted stays held.
    ///
    /// # Errors
    ///
    /// The first event that could not be posted, after every one was tried.
    pub fn release_all(&mut self, mut post: impl FnMut(&MouseEvent) -> Result<()>) -> Result<()> {
        let mut first: Option<Error> = None;
        if let Some(at) = self.mouse_at {
            for number in 0..32_u32 {
                let button = MouseButton(number);
                if self.buttons & button.bit() == 0 {
                    continue;
                }
                let event = MouseEvent::mouse(MouseKind::Up(button), at);
                match post(&event) {
                    Ok(()) => self.apply(&event),
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
        }
        if let Some(pen) = self.pen {
            if pen.down {
                let event = MouseEvent::pen(MouseKind::Up(MouseButton::LEFT), pen.at, 0.0);
                match post(&event) {
                    Ok(()) => self.apply(&event),
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
            if !self.pen_down() {
                let event = MouseEvent::pen(MouseKind::Proximity { entering: false }, pen.at, 0.0);
                match post(&event) {
                    Ok(()) => self.apply(&event),
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
        }
        first.map_or(Ok(()), Err)
    }
}

/// The wheel turns a scroll of `delta_x` rightwards and `delta_y` downwards device pixels posts,
/// as `CGEventCreateScrollWheelEvent2` takes them in pixel units: the first wheel vertical, the
/// second horizontal, positive towards the top and the left, so each is the delta negated.
///
/// # Errors
///
/// `invalid_argument` for a delta whose negation does not fit an `i32`.
pub fn scroll_wheels(delta_x: i32, delta_y: i32) -> Result<(i32, i32)> {
    let negate = |delta: i32| {
        delta.checked_neg().ok_or_else(|| {
            Error::invalid_argument(format!(
                "a scroll of {delta} pixels is past what Quartz takes"
            ))
        })
    };
    Ok((negate(delta_y)?, negate(delta_x)?))
}
