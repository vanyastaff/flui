//! Browser trackpad pinch, decoded from what each engine dispatches for it
//! (ADR-0213 §2, ADR-0217 §4): Chromium and Firefox send a trackpad pinch as
//! `wheel` events with `ctrlKey` set, WebKit as `gesturestart`,
//! `gesturechange` and `gestureend`. Cfg-free plain state, so Linux CI pins
//! it; `platforms::web` feeds it DOM values and delivers what it returns.
//!
//! # Ctrl+wheel
//!
//! The DOM does not say whether a Control-held wheel event came from a
//! pinch or from a mouse wheel with the key held, so the decision is
//! [`Availability::Emulated`](flui_platform::input_capabilities::Availability)
//! and is taken once per sequence, from its first event:
//!
//! - a pinch is a pixel-mode vertical delta that is a whole number neither
//!   of CSS pixels nor of device pixels (Chromium reports `-100 · ln(scale)`
//!   CSS pixels, never a whole number in practice); a mouse detent is a
//!   whole number of one of them, or lines: at a page zoom its CSS pixels
//!   are fractional and its device pixels whole;
//! - a sequence is the run of Control-held wheel events no further apart than
//!   [`PINCH_GAP`] on the event timeline, and every event of it is treated as
//!   its first was, so one whole-number step of a pinch stays in it and one
//!   fractional detent of a Control-held mouse wheel stays a scroll;
//! - a pinch ends at its deadline, the last event's time plus [`PINCH_GAP`]:
//!   the End is stamped with that time on the event timeline, whether the
//!   browser's timer or the next input reaches it first. A wheel event
//!   without Control ends it at that event's time;
//! - the application may refuse the sequence: when it does not prevent the
//!   default of the sequence's first event, the pinch is cancelled at once and
//!   the rest of the sequence reaches neither the application nor
//!   `preventDefault`, so the browser zooms the page as it would have without
//!   FLUI.
//!
//! The factor a step applies is `exp(-deltaY / 100)`, the inverse of
//! Chromium's encoding; the focal point is the event's position.
//!
//! # WebKit gesture events
//!
//! `GestureEvent.scale` and `.rotation` are cumulative from `gesturestart`
//! (rotation in degrees, clockwise positive); [`GestureTracker`] turns them
//! into the per-step factor and radians the shared accumulator takes, and the
//! same refusal applies to its first event.

use std::time::Duration;

use flui_platform::EventTime;

use super::pointer_assembly::{GestureComponent, GestureDelta, GesturePhase, GestureReading};

/// The longest pause between two events of one Control-held wheel sequence.
/// Chromium dispatches a pinch's steps at the display rate; a longer pause
/// is a new gesture.
pub const PINCH_GAP: Duration = Duration::from_millis(120);

/// `WheelEvent.DOM_DELTA_PIXEL`.
const DOM_DELTA_PIXEL: u32 = 0;

/// The values of one DOM `wheel` event the decision reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelStep {
    /// `ctrlKey`.
    pub ctrl: bool,
    /// `deltaMode`.
    pub mode: u32,
    /// `deltaX`.
    pub dx: f64,
    /// `deltaY`.
    pub dy: f64,
    /// `devicePixelRatio`: device pixels per CSS pixel, page zoom included.
    pub scale: f64,
    /// `timeStamp` on the event timeline.
    pub time: EventTime,
}

/// What a wheel event is, once [`CtrlWheel::route`] has read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelRoute {
    /// A scroll: delivered as `PointerEvent::Scroll`.
    Scroll,
    /// A step of a pinch: delivered as the gesture reading.
    Pinch(GestureReading),
    /// A step of a refused pinch: delivered to nobody, and its default (the
    /// browser's zoom) kept.
    Refused,
}

/// What a Control-held wheel sequence was classified as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sequence {
    Pinch,
    Scroll,
    Refused,
}

/// The open Control-held sequence and its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Open {
    kind: Sequence,
    deadline: EventTime,
}

/// The ctrl+wheel pinch decoder of one surface (see the module doc).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CtrlWheel {
    open: Option<Open>,
}

/// The End of the pinch open until `deadline`, stamped with it.
fn end_at(deadline: EventTime) -> (GestureReading, EventTime) {
    (
        GestureReading {
            component: GestureComponent::Scale,
            phase: GesturePhase::Ended,
            delta: GestureDelta::None,
        },
        deadline,
    )
}

impl CtrlWheel {
    /// When the open pinch ends unless another step arrives, for the timer
    /// that delivers its End; `None` when no pinch is open.
    #[must_use]
    pub fn deadline(&self) -> Option<EventTime> {
        self.open
            .filter(|open| open.kind == Sequence::Pinch)
            .map(|open| open.deadline)
    }

    /// End whatever sequence is due at `now`, returning a pinch's End stamped
    /// with its deadline. Call it before delivering any input, and from the
    /// deadline's timer.
    pub fn expire(&mut self, now: EventTime) -> Option<(GestureReading, EventTime)> {
        let open = self.open?;
        if now <= open.deadline {
            return None;
        }
        self.open = None;
        (open.kind == Sequence::Pinch).then(|| end_at(open.deadline))
    }

    /// End the open pinch now, at `time` (no later than its deadline): a
    /// wheel event without Control, or the surface going away.
    pub fn finish(&mut self, time: EventTime) -> Option<(GestureReading, EventTime)> {
        let open = self.open.take()?;
        (open.kind == Sequence::Pinch).then(|| end_at(time.min(open.deadline)))
    }

    /// Refuse the open pinch: the application did not prevent the default of
    /// its first event. Returns the cancellation of the pinch the first event
    /// started; the rest of the sequence is [`WheelRoute::Refused`].
    pub fn refuse(&mut self) -> Option<GestureReading> {
        let open = self.open.as_mut()?;
        if open.kind != Sequence::Pinch {
            return None;
        }
        open.kind = Sequence::Refused;
        Some(GestureReading {
            component: GestureComponent::Scale,
            phase: GesturePhase::Cancelled,
            delta: GestureDelta::None,
        })
    }

    /// Route `step`. A Control-held event within the open sequence's gap
    /// continues it; otherwise it starts one, classified by this event.
    /// Call [`Self::expire`] (or [`Self::finish`] for an event without
    /// Control) first, so a due End is delivered before it.
    pub fn route(&mut self, step: WheelStep) -> WheelRoute {
        if !step.ctrl {
            self.open = None;
            return WheelRoute::Scroll;
        }
        // The end of the timeline closes a sequence no later step can extend.
        let deadline = step
            .time
            .checked_add(PINCH_GAP)
            .unwrap_or(EventTime::from_nanos(u64::MAX));
        let continuing = self.open.filter(|open| step.time <= open.deadline);
        let kind = continuing.map_or_else(|| classify(step), |open| open.kind);
        self.open = Some(Open { kind, deadline });
        match kind {
            Sequence::Scroll => WheelRoute::Scroll,
            Sequence::Refused => WheelRoute::Refused,
            Sequence::Pinch => {
                let factor = (-step.dy / 100.0).exp();
                let delta = if factor.is_finite() && factor > 0.0 {
                    GestureDelta::Scale(factor)
                } else {
                    GestureDelta::None
                };
                WheelRoute::Pinch(GestureReading {
                    component: GestureComponent::Scale,
                    phase: if continuing.is_some() {
                        GesturePhase::Changed
                    } else {
                        GesturePhase::Began
                    },
                    delta,
                })
            }
        }
    }
}

/// How close to a whole number a delta is a whole number: CSS pixels reach
/// the page through single precision, and a detent at a page zoom is a
/// whole number of device pixels only to that precision.
const WHOLE_TOLERANCE: f64 = 1e-3;

/// Whether `value` is a whole number, to [`WHOLE_TOLERANCE`].
fn is_whole(value: f64) -> bool {
    (value - value.round()).abs() <= WHOLE_TOLERANCE
}

/// Whether the first event of a Control-held sequence is a pinch: a
/// pixel-mode vertical delta that is a whole number neither of CSS pixels
/// nor of device pixels. A mouse detent is whole in one of them: at a page
/// zoom Chromium divides it into fractional CSS pixels (100 at 110% is
/// 90.909…, 100 device pixels), and an accelerated wheel lands within single
/// precision of a whole number (4.000244).
fn classify(step: WheelStep) -> Sequence {
    let device = step.dy * step.scale;
    if step.mode == DOM_DELTA_PIXEL
        && step.dx == 0.0
        && step.dy.is_finite()
        && !is_whole(step.dy)
        && !(device.is_finite() && step.scale > 0.0 && is_whole(device))
    {
        Sequence::Pinch
    } else {
        Sequence::Scroll
    }
}

/// A WebKit gesture event's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureEventKind {
    /// `gesturestart`.
    Start,
    /// `gesturechange`.
    Change,
    /// `gestureend`.
    End,
}

/// The cumulative values of a WebKit gesture since its start, turned into
/// per-step readings (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GestureTracker {
    /// The last cumulative scale and rotation (degrees) delivered, while a
    /// gesture is open; `refused` once the application refused it.
    open: Option<(f64, f64)>,
    refused: bool,
}

impl GestureTracker {
    /// The readings one gesture event makes: scale, then rotation. A value
    /// that is not finite (or a scale that is not positive) changes nothing.
    /// Empty for an event of a refused gesture, or for a change or end with
    /// no gesture open.
    pub fn step(
        &mut self,
        kind: GestureEventKind,
        scale: f64,
        rotation_degrees: f64,
    ) -> [Option<GestureReading>; 2] {
        let phase = match kind {
            GestureEventKind::Start => {
                self.open = Some((1.0, 0.0));
                self.refused = false;
                GesturePhase::Began
            }
            GestureEventKind::Change => GesturePhase::Changed,
            GestureEventKind::End => GesturePhase::Ended,
        };
        let Some((last_scale, last_rotation)) = self.open else {
            return [None, None];
        };
        if kind == GestureEventKind::End {
            self.open = None;
        }
        if self.refused {
            return [None, None];
        }
        let scale_ok = scale.is_finite() && scale > 0.0;
        let rotation_ok = rotation_degrees.is_finite();
        let (next_scale, next_rotation) = (
            if scale_ok { scale } else { last_scale },
            if rotation_ok {
                rotation_degrees
            } else {
                last_rotation
            },
        );
        if kind != GestureEventKind::End {
            self.open = Some((next_scale, next_rotation));
        }
        let factor = next_scale / last_scale;
        let scale_delta = if scale_ok && factor.is_finite() && factor > 0.0 {
            GestureDelta::Scale(factor)
        } else {
            GestureDelta::None
        };
        let turn = (next_rotation - last_rotation).to_radians();
        let rotation_delta = if rotation_ok && turn.is_finite() {
            GestureDelta::Rotation(turn)
        } else {
            GestureDelta::None
        };
        [
            Some(GestureReading {
                component: GestureComponent::Scale,
                phase,
                delta: scale_delta,
            }),
            Some(GestureReading {
                component: GestureComponent::Rotation,
                phase,
                delta: rotation_delta,
            }),
        ]
    }

    /// Refuse the open gesture: the cancellation of what its first event
    /// started. The rest of it is delivered to nobody.
    pub fn refuse(&mut self) -> Option<GestureReading> {
        self.open?;
        self.refused = true;
        Some(GestureReading {
            component: GestureComponent::Scale,
            phase: GesturePhase::Cancelled,
            delta: GestureDelta::None,
        })
    }

    /// Whether the open gesture was refused.
    #[must_use]
    pub const fn is_refused(&self) -> bool {
        self.refused
    }
}
