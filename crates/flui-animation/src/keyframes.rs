//! Keyframe tracks: a value as a pure function of elapsed time.
//!
//! A [`Keyframes<T>`] track starts at a value and moves through segments laid
//! end to end from time zero, each with a [`Duration`]:
//!
//! - [`to`](KeyframesBuilder::to) eases toward a value along a [`Curve`] that
//!   belongs to that segment;
//! - [`cubic`](KeyframesBuilder::cubic) passes through the value on a
//!   Catmull-Rom spline whose knots are the keyframe *times*;
//! - [`hold`](KeyframesBuilder::hold) keeps the current value;
//! - [`jump`](KeyframesBuilder::jump) changes the value instantly.
//!
//! After the last segment the track holds its final value until
//! [`total`](Keyframes::total). A delay is a leading `hold`; tracks that share
//! one `total` and one controller form a group that reads one clock; a loop
//! is a repeating controller, or [`value_at_looped`](Keyframes::value_at_looped).
//! [`Stagger`](crate::Stagger) shifts the read time per index.
//!
//! # Evaluation
//!
//! - **Right-continuous.** At a segment boundary the track returns the
//!   keyframe value exactly (a clone, not an interpolation); at a `jump` it
//!   returns the value *after* the jump (WAAPI §5.3.4).
//! - **Pure.** No state: any order of queries, a repeated query and a query
//!   back in time give the same answer.
//! - **Finite.** Keyframe values are checked when the track is built. A
//!   curve that returns NaN or infinity, or interpolation that overflows,
//!   publishes the segment's start value instead of a non-finite sample.
//!
//! `Matrix4` is not a keyframe type: animate rotation, scale and translation
//! as separate tracks.

use std::fmt;
use std::time::Duration;

use flui_foundation::geometry::Lerp;

use crate::curve::{ArcCurve, Curve};
use crate::spring::TwoWayConverter;
use crate::tween_types::Animatable;

/// A track of keyframes timed by [`Duration`]; see the
/// [module documentation](self) for the evaluation rules.
///
/// Build one with [`Keyframes::builder`]. The track is an immutable value:
/// clone it freely, sample it from any thread when `T` is `Send + Sync`.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use flui_animation::{Curves, Keyframes};
///
/// let ms = Duration::from_millis;
/// // 90° in 300 ms, a pause, then another 90°.
/// let turn = Keyframes::builder(0.0, ms(1200))
///     .to(90.0, ms(300), Curves::EaseOut)
///     .hold(ms(300))
///     .to(180.0, ms(300), Curves::EaseOut)
///     .build()
///     .expect("the segments fit in 1200 ms");
///
/// assert_eq!(turn.value_at(ms(300)), 90.0);
/// assert_eq!(turn.value_at(ms(450)), 90.0);
/// assert_eq!(turn.value_at(ms(1000)), 180.0);
/// assert_eq!(turn.value_at_looped(ms(1500)), 90.0);
/// ```
pub struct Keyframes<T: TwoWayConverter> {
    start: T,
    total: Duration,
    segments: Box<[Segment<T>]>,
}

/// One placed segment: it runs from `start` to `end` and moves `from` → `to`.
struct Segment<T: TwoWayConverter> {
    start: Duration,
    end: Duration,
    from: T,
    to: T,
    motion: Motion<T::Vector>,
}

/// How a segment moves between its two values.
enum Motion<V> {
    /// `from.lerp_to(to, curve(s))`.
    Eased(ArcCurve),
    /// A cubic Hermite segment with end velocities in units per second.
    Cubic { start_velocity: V, end_velocity: V },
    /// `from` throughout (`from == to`).
    Hold,
    /// A zero-length segment; the track reads `to` from its time on.
    Jump,
}

/// A segment as the builder records it, before times and velocities exist.
enum Pending<T> {
    Eased {
        value: T,
        over: Duration,
        curve: ArcCurve,
    },
    Cubic {
        value: T,
        over: Duration,
    },
    Hold {
        over: Duration,
    },
    Jump {
        value: T,
    },
}

/// Builds a [`Keyframes`] track; see [`Keyframes::builder`].
///
/// Each method appends one segment after the previous one. Nothing is
/// validated until [`build`](Self::build), which reports the first problem.
#[must_use = "a builder does nothing until `build` is called"]
pub struct KeyframesBuilder<T: TwoWayConverter> {
    start: T,
    total: Duration,
    segments: Vec<Pending<T>>,
}

/// Why [`KeyframesBuilder::build`] rejected a track.
///
/// Keyframes are numbered from 0, the start value; segment `i` (0-based, in
/// the order the builder methods were called) ends at keyframe `i + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyframesError {
    /// The track's `total` is [`Duration::ZERO`].
    #[error("keyframes total must be non-zero")]
    ZeroTotal,
    /// Segment `index` ends after the track's `total`.
    #[error("segment {index} ends at {end:?}, past total {total:?}")]
    Overrun {
        /// The segment that does not fit.
        index: usize,
        /// When that segment would end.
        end: Duration,
        /// The track's total.
        total: Duration,
    },
    /// The segment durations up to segment `index` overflow [`Duration`].
    #[error("segment durations overflow Duration at segment {index}")]
    DurationOverflow {
        /// The segment whose end time overflowed.
        index: usize,
    },
    /// Keyframe `index` has a NaN or infinite component.
    #[error("keyframe {index} has a non-finite component")]
    NonFiniteValue {
        /// The keyframe: 0 is the start value, `i + 1` ends segment `i`.
        index: usize,
    },
}

impl<T: Lerp + TwoWayConverter> Keyframes<T> {
    /// Starts a track at `start` that lasts `total`.
    ///
    /// `total` sets the loop period of
    /// [`value_at_looped`](Self::value_at_looped) and the time an
    /// [`Animatable`] progress of 1 maps to; tracks of one group share it.
    pub fn builder(start: T, total: Duration) -> KeyframesBuilder<T> {
        KeyframesBuilder {
            start,
            total,
            segments: Vec::new(),
        }
    }

    /// The track's length: the time [`value_at`](Self::value_at) clamps to
    /// and the period of [`value_at_looped`](Self::value_at_looped).
    #[must_use]
    pub fn total(&self) -> Duration {
        self.total
    }

    /// The value `elapsed` after the start, clamped to [`total`](Self::total).
    #[must_use]
    pub fn value_at(&self, elapsed: Duration) -> T {
        let _ = elapsed;
        self.start.clone()
    }

    /// The value at `elapsed` modulo [`total`](Self::total), for a track
    /// that repeats. Defined for every `Duration`, including
    /// [`Duration::MAX`].
    #[must_use]
    pub fn value_at_looped(&self, elapsed: Duration) -> T {
        let _ = elapsed;
        self.start.clone()
    }
}

impl<T: Lerp + TwoWayConverter> KeyframesBuilder<T> {
    /// Moves to `value` over `over`, eased by `curve`.
    ///
    /// The curve belongs to this segment — the one arriving at `value` — and
    /// maps the segment's own progress. An overshooting curve extrapolates,
    /// as it does in a [`Tween`](crate::Tween).
    pub fn to(
        mut self,
        value: T,
        over: Duration,
        curve: impl Curve + Send + Sync + 'static,
    ) -> Self {
        self.segments.push(Pending::Eased {
            value,
            over,
            curve: ArcCurve::new(curve),
        });
        self
    }

    /// Moves to `value` over `over` on a cubic spline through the keyframes.
    ///
    /// Consecutive `cubic` segments form a Catmull-Rom spline whose knots are
    /// the keyframe times, so the track passes through every keyframe *at its
    /// time*. Where a `cubic` segment meets a [`to`](Self::to) segment it
    /// takes that segment's velocity at the join; at either end of the track
    /// and next to a [`hold`](Self::hold) or [`jump`](Self::jump) the
    /// velocity is zero.
    pub fn cubic(mut self, value: T, over: Duration) -> Self {
        self.segments.push(Pending::Cubic { value, over });
        self
    }

    /// Keeps the current value for `over`.
    pub fn hold(mut self, over: Duration) -> Self {
        self.segments.push(Pending::Hold { over });
        self
    }

    /// Changes the value to `value` instantly; from this time on the track
    /// reads `value`.
    pub fn jump(mut self, value: T) -> Self {
        self.segments.push(Pending::Jump { value });
        self
    }

    /// Places the segments and returns the track.
    ///
    /// # Errors
    ///
    /// - [`KeyframesError::ZeroTotal`] when `total` is zero;
    /// - [`KeyframesError::NonFiniteValue`] when a keyframe has a NaN or
    ///   infinite component;
    /// - [`KeyframesError::DurationOverflow`] when the segment durations sum
    ///   past [`Duration::MAX`];
    /// - [`KeyframesError::Overrun`] when a segment ends after `total`.
    ///
    /// The first of these, in segment order, is reported.
    pub fn build(self) -> Result<Keyframes<T>, KeyframesError> {
        Ok(Keyframes {
            start: self.start,
            total: self.total,
            segments: Box::new([]),
        })
    }
}

impl<T: Lerp + TwoWayConverter> Animatable<T> for Keyframes<T> {
    /// Reads the track at progress `t` of [`total`](Keyframes::total):
    /// `t` is clamped into `[0, 1]` and NaN reads as 0.
    fn transform(&self, t: f64) -> T {
        let _ = t;
        self.start.clone()
    }
}

impl<T: TwoWayConverter> Clone for Keyframes<T> {
    fn clone(&self) -> Self {
        Self {
            start: self.start.clone(),
            total: self.total,
            segments: self.segments.clone(),
        }
    }
}

impl<T: TwoWayConverter> Clone for Segment<T> {
    fn clone(&self) -> Self {
        Self {
            start: self.start,
            end: self.end,
            from: self.from.clone(),
            to: self.to.clone(),
            motion: match &self.motion {
                Motion::Eased(curve) => Motion::Eased(curve.clone()),
                Motion::Cubic {
                    start_velocity,
                    end_velocity,
                } => Motion::Cubic {
                    start_velocity: *start_velocity,
                    end_velocity: *end_velocity,
                },
                Motion::Hold => Motion::Hold,
                Motion::Jump => Motion::Jump,
            },
        }
    }
}

impl<T: TwoWayConverter + fmt::Debug> fmt::Debug for Keyframes<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keyframes")
            .field("start", &self.start)
            .field("total", &self.total)
            .field("segments", &self.segments.len())
            .finish()
    }
}

impl<T: TwoWayConverter + fmt::Debug> fmt::Debug for KeyframesBuilder<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyframesBuilder")
            .field("start", &self.start)
            .field("total", &self.total)
            .field("segments", &self.segments.len())
            .finish()
    }
}
