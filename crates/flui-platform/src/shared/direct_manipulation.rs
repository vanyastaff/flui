//! A precision touchpad as DirectManipulation reports it (ADR-0217 §4).
//!
//! A Win32 viewport reports two things: its status (`Running` while fingers
//! manipulate it, `Inertia` while it coasts, `Ready` once it stopped) and the
//! content transform, which accumulates the manipulation since the viewport
//! last stood at the origin. [`ManipulationDecoder`] turns that stream into
//! the owned grammar every backend shares: a two-finger pan is a phased
//! scroll whose coasting is `ScrollPhase::Momentum*`, a pinch is a
//! [`GestureReading`] on the trackpad contact. [`TouchpadAssembler`] adds
//! the cursor position, clock and identity and returns finished
//! [`PlatformInput`]s.
//!
//! Everything here is plain data and cfg-free: the Win32 backend copies each
//! COM callback into a [`ViewportReading`], and Linux CI replays recordings of
//! them through the same code.
//!
//! One manipulation is a pan or a pinch, decided by whichever passes its slop
//! first; the translation of a pinch is the centroid moving, and is not a
//! scroll. A viewport that returns to `Ready` asks to be reset to the origin
//! ([`Delivered::reset`]); the content update that reset causes arrives while
//! no manipulation is open and is not input.

use flui_foundation::geometry::Point;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{
    PointerEvent, PointerPosition, ScrollDelta, ScrollEvent, ScrollPrecision, ScrollUnit,
};
use smallvec::SmallVec;

use super::events::primary_mouse_info;
use super::pointer_assembly::{
    ClockChoice, ContactRegistry, DecodeError, GestureComponent, GestureDelta, GesturePhase,
    GestureReading, NativeContact, NativeReading, NativeSurface, ReadingAction, ReadingTime,
    ScrollPhases, ScrollReading, decode,
};
use crate::traits::PlatformInput;

/// Translation, in device pixels, a pan must travel before it is a pan.
pub const PAN_SLOP: f64 = 2.0;

/// Relative scale change a pinch must reach before it is a pinch.
pub const PINCH_SLOP: f64 = 0.02;

/// The trackpad contact every pinch of a window reads as: outside the 32-bit
/// range of `WM_POINTER` ids.
pub const PINCH_CONTACT: NativeContact = NativeContact(1 << 33);

/// `DIRECTMANIPULATION_STATUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ViewportStatus {
    /// Being built.
    Building,
    /// Enabled and waiting for a contact.
    Enabled,
    /// Disabled; no input reaches it.
    Disabled,
    /// Fingers are manipulating it.
    Running,
    /// Coasting after the fingers lifted.
    Inertia,
    /// Stopped.
    Ready,
    /// Suspended by the system.
    Suspended,
}

impl ViewportStatus {
    /// The status a native `DIRECTMANIPULATION_STATUS` number names.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Option<Self> {
        Some(match raw {
            0 => Self::Building,
            1 => Self::Enabled,
            2 => Self::Disabled,
            3 => Self::Running,
            4 => Self::Inertia,
            5 => Self::Ready,
            6 => Self::Suspended,
            _ => return None,
        })
    }

    /// Whether the viewport still moves, so its owner must keep updating it.
    #[must_use]
    pub const fn is_animating(self) -> bool {
        matches!(self, Self::Running | Self::Inertia)
    }
}

/// A viewport's content transform: a uniform scale and an offset in device
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentTransform {
    scale: f64,
    x: f64,
    y: f64,
}

impl ContentTransform {
    const IDENTITY: Self = Self {
        scale: 1.0,
        x: 0.0,
        y: 0.0,
    };

    /// A transform; `None` when a component is not finite or `scale` is not
    /// positive.
    #[must_use]
    pub fn try_new(scale: f64, x: f64, y: f64) -> Option<Self> {
        (scale.is_finite() && scale > 0.0 && x.is_finite() && y.is_finite()).then_some(Self {
            scale,
            x,
            y,
        })
    }

    /// The transform of the six-number matrix `GetContentTransform` fills
    /// (`[scale, 0, 0, scale, x, y]`); `None` when it is shorter or not a
    /// finite positive scale with a finite offset.
    #[must_use]
    pub fn from_matrix(matrix: &[f32]) -> Option<Self> {
        let [scale, _, _, _, x, y] = *matrix.first_chunk::<6>()?;
        Self::try_new(f64::from(scale), f64::from(x), f64::from(y))
    }

    /// The uniform scale.
    #[must_use]
    pub const fn scale(&self) -> f64 {
        self.scale
    }

    /// The horizontal offset, in device pixels.
    #[must_use]
    pub const fn x(&self) -> f64 {
        self.x
    }

    /// The vertical offset, in device pixels.
    #[must_use]
    pub const fn y(&self) -> f64 {
        self.y
    }
}

/// One callback of a viewport's event handler, as plain data.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ViewportReading {
    /// `OnViewportStatusChanged`: the viewport's new status.
    Status(ViewportStatus),
    /// `OnContentUpdated`: the content's transform.
    Content(ContentTransform),
}

/// What the decoder publishes for a reading.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Manipulation {
    /// A scroll step: `dx` and `dy` are device pixels, positive when the
    /// scroll offset grows (content moves up and left under the fingers).
    Scroll {
        /// The native phases.
        reading: ScrollReading,
        /// Horizontal distance.
        dx: f64,
        /// Vertical distance.
        dy: f64,
    },
    /// A pinch component.
    Pinch(GestureReading),
    /// The viewport has stopped: return its content to the origin.
    ResetViewport,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    /// No manipulation is open.
    Idle,
    /// Fingers are down and have not passed a slop yet.
    Undecided,
    Panning,
    Pinching,
    /// The viewport coasts.
    Momentum,
}

/// Turns a viewport's statuses and content transforms into scrolls, momentum
/// and pinches.
#[derive(Debug, Clone, Copy)]
pub struct ManipulationDecoder {
    state: State,
    /// The last transform seen: where the next delta is measured from.
    anchor: ContentTransform,
}

impl Default for ManipulationDecoder {
    fn default() -> Self {
        Self {
            state: State::Idle,
            anchor: ContentTransform::IDENTITY,
        }
    }
}

fn scroll(
    fingers: Option<GesturePhase>,
    momentum: Option<GesturePhase>,
    dx: f64,
    dy: f64,
) -> Manipulation {
    Manipulation::Scroll {
        reading: ScrollReading {
            fingers,
            momentum,
            precise: true,
        },
        dx,
        dy,
    }
}

fn pinch(phase: GesturePhase, delta: GestureDelta) -> Manipulation {
    Manipulation::Pinch(GestureReading {
        component: GestureComponent::Scale,
        phase,
        delta,
    })
}

impl ManipulationDecoder {
    /// Whether a manipulation is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// Apply `reading` and return what it publishes, in order.
    pub fn apply(&mut self, reading: ViewportReading) -> SmallVec<[Manipulation; 3]> {
        match reading {
            ViewportReading::Status(status) => self.status(status),
            ViewportReading::Content(transform) => self.content(transform),
        }
    }

    /// End whatever is open as cancelled: the viewport was lost.
    pub fn abandon(&mut self) -> SmallVec<[Manipulation; 3]> {
        self.status(ViewportStatus::Disabled)
    }

    fn status(&mut self, status: ViewportStatus) -> SmallVec<[Manipulation; 3]> {
        use GesturePhase::{Began, Cancelled, Ended};
        let mut out = SmallVec::new();
        let state = self.state;
        match status {
            ViewportStatus::Running => {
                if state == State::Momentum {
                    out.push(scroll(None, Some(Ended), 0.0, 0.0));
                }
                if matches!(state, State::Idle | State::Momentum) {
                    self.state = State::Undecided;
                }
            }
            ViewportStatus::Inertia => match state {
                State::Panning => {
                    out.push(scroll(Some(Ended), None, 0.0, 0.0));
                    out.push(scroll(None, Some(Began), 0.0, 0.0));
                    self.state = State::Momentum;
                }
                State::Pinching => {
                    out.push(pinch(Ended, GestureDelta::None));
                    self.state = State::Idle;
                }
                State::Undecided | State::Idle => {
                    out.push(scroll(None, Some(Began), 0.0, 0.0));
                    self.state = State::Momentum;
                }
                State::Momentum => {}
            },
            ViewportStatus::Ready => {
                self.finish(&mut out, Ended, Ended);
            }
            ViewportStatus::Building
            | ViewportStatus::Enabled
            | ViewportStatus::Disabled
            | ViewportStatus::Suspended => {
                self.finish(&mut out, Cancelled, Ended);
            }
        }
        out
    }

    /// Close the open manipulation with `fingers` for a pan or pinch and
    /// `momentum` for coasting, and ask for a reset.
    fn finish(
        &mut self,
        out: &mut SmallVec<[Manipulation; 3]>,
        fingers: GesturePhase,
        momentum: GesturePhase,
    ) {
        match self.state {
            State::Idle => return,
            State::Undecided => {}
            State::Panning => out.push(scroll(Some(fingers), None, 0.0, 0.0)),
            State::Pinching => out.push(pinch(fingers, GestureDelta::None)),
            State::Momentum => out.push(scroll(None, Some(momentum), 0.0, 0.0)),
        }
        self.state = State::Idle;
        out.push(Manipulation::ResetViewport);
    }

    fn content(&mut self, transform: ContentTransform) -> SmallVec<[Manipulation; 3]> {
        use GesturePhase::{Began, Changed};
        let mut out = SmallVec::new();
        let moved = (transform.x - self.anchor.x, transform.y - self.anchor.y);
        let zoom = transform.scale / self.anchor.scale;
        // `0.0 -` also turns a negative zero into zero.
        let (dx, dy) = (0.0 - moved.0, 0.0 - moved.1);
        match self.state {
            State::Idle => self.anchor = transform,
            State::Undecided => {
                if (zoom - 1.0).abs() >= PINCH_SLOP {
                    out.push(pinch(Began, GestureDelta::Scale(zoom)));
                    self.state = State::Pinching;
                    self.anchor = transform;
                } else if moved.0.hypot(moved.1) >= PAN_SLOP {
                    out.push(scroll(Some(Began), None, dx, dy));
                    self.state = State::Panning;
                    self.anchor = transform;
                }
            }
            State::Panning => {
                if moved != (0.0, 0.0) {
                    out.push(scroll(Some(Changed), None, dx, dy));
                }
                self.anchor = transform;
            }
            State::Pinching => {
                if zoom != 1.0 {
                    out.push(pinch(Changed, GestureDelta::Scale(zoom)));
                }
                self.anchor = transform;
            }
            State::Momentum => {
                if moved != (0.0, 0.0) {
                    out.push(scroll(None, Some(Changed), dx, dy));
                }
                self.anchor = transform;
            }
        }
        out
    }
}

/// Where and when a viewport reading happened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchpadFrame {
    /// The reading's clocks; the performance counter stamps it when present.
    pub time: ReadingTime,
    /// The cursor, in device pixels of the window's client area: a touchpad
    /// manipulates what is under the cursor.
    pub position: (f64, f64),
    /// Device pixels per logical pixel.
    pub scale: f64,
    /// The modifiers held.
    pub modifiers: Modifiers,
}

/// The input a viewport reading delivers.
#[derive(Debug, Default)]
pub struct Delivered {
    /// Scrolls and pinch phases, in order.
    pub inputs: Vec<PlatformInput>,
    /// Whether the viewport stopped and should return to the origin.
    pub reset: bool,
}

/// A window's precision-touchpad input: the decoder plus the phase grammar.
#[derive(Debug)]
pub struct TouchpadAssembler {
    surface: NativeSurface,
    decoder: ManipulationDecoder,
    scrolling: ScrollPhases,
}

impl TouchpadAssembler {
    /// An assembler for `surface`.
    #[must_use]
    pub fn new(surface: NativeSurface) -> Self {
        Self {
            surface,
            decoder: ManipulationDecoder::default(),
            scrolling: ScrollPhases::default(),
        }
    }

    /// Whether a manipulation is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.decoder.is_open()
    }

    /// Decode `reading` at `frame`, assembling pinches against `contacts`.
    pub fn assemble(
        &mut self,
        contacts: &mut ContactRegistry,
        reading: ViewportReading,
        frame: TouchpadFrame,
    ) -> Delivered {
        let steps = self.decoder.apply(reading);
        self.deliver(contacts, &steps, frame)
    }

    /// End whatever is open as cancelled: the window or its viewport is
    /// going away.
    pub fn abandon(&mut self, contacts: &mut ContactRegistry, frame: TouchpadFrame) -> Delivered {
        let steps = self.decoder.abandon();
        let delivered = self.deliver(contacts, &steps, frame);
        self.scrolling.cancel_surface(self.surface);
        delivered
    }

    fn deliver(
        &mut self,
        contacts: &mut ContactRegistry,
        steps: &[Manipulation],
        frame: TouchpadFrame,
    ) -> Delivered {
        let mut delivered = Delivered::default();
        for step in steps {
            match *step {
                Manipulation::ResetViewport => delivered.reset = true,
                Manipulation::Scroll { reading, dx, dy } => {
                    delivered
                        .inputs
                        .extend(self.scroll(contacts, reading, (dx, dy), frame));
                }
                Manipulation::Pinch(gesture) => {
                    let (x, y) = frame.position;
                    let native = NativeReading::new(
                        PINCH_CONTACT,
                        flui_platform_api::pointer::PointerKind::Trackpad,
                        ReadingAction::Gesture(gesture),
                        frame.time,
                    )
                    .on(self.surface)
                    .with_primary(true)
                    .with_modifiers(frame.modifiers)
                    .at(x, y, frame.scale);
                    delivered
                        .inputs
                        .extend(decode(contacts, &native).unwrap_or_else(DecodeError::into_inputs));
                }
            }
        }
        delivered
    }

    fn scroll(
        &mut self,
        contacts: &ContactRegistry,
        reading: ScrollReading,
        (dx, dy): (f64, f64),
        frame: TouchpadFrame,
    ) -> Option<PlatformInput> {
        let phase = self.scrolling.phase(self.surface, None, reading)?;
        let time = contacts.clock().stamp(frame.time, ClockChoice::Counter)?;
        let scale = frame.scale;
        let position = PointerPosition::try_new(Point::new(
            frame.position.0 / scale,
            frame.position.1 / scale,
        ))
        .ok()?;
        let delta = ScrollDelta::try_new(ScrollUnit::Pixels, dx / scale, dy / scale)
            .unwrap_or_else(|_| ScrollDelta::zero(ScrollUnit::Pixels));
        Some(PlatformInput::Pointer(PointerEvent::Scroll(
            ScrollEvent::new(primary_mouse_info(), time, position, delta)
                .with_precision(ScrollPrecision::Precise)
                .with_phase(phase)
                .with_modifiers(frame.modifiers),
        )))
    }
}
