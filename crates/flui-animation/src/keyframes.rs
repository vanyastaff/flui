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
        let elapsed = elapsed.min(self.total);
        // The first segment still running at `elapsed`. Segments that ended
        // at or before it — including zero-length jumps at `elapsed` — are
        // passed, which makes the track right-continuous.
        let index = self
            .segments
            .partition_point(|segment| segment.end <= elapsed);
        let Some(segment) = self.segments.get(index) else {
            return self
                .segments
                .last()
                .map_or_else(|| self.start.clone(), |last| last.to.clone());
        };
        // Segments are contiguous, so this one started at or before `elapsed`.
        let offset = elapsed.saturating_sub(segment.start);
        if offset.is_zero() {
            return segment.from.clone();
        }
        let length = segment.end.saturating_sub(segment.start);
        segment.sample(offset.as_secs_f64() / length.as_secs_f64(), length)
    }

    /// The value at `elapsed` modulo [`total`](Self::total), for a track
    /// that repeats. Defined for every `Duration`, including
    /// [`Duration::MAX`].
    #[must_use]
    pub fn value_at_looped(&self, elapsed: Duration) -> T {
        let phase = elapsed.as_nanos() % self.total.as_nanos();
        // `phase < total`, so both parts fit their types.
        let seconds = u64::try_from(phase / NANOS_PER_SECOND)
            .expect("BUG: a phase inside the track's total fits Duration");
        let nanos =
            u32::try_from(phase % NANOS_PER_SECOND).expect("BUG: a remainder of 1e9 fits in u32");
        self.value_at(Duration::new(seconds, nanos))
    }
}

const NANOS_PER_SECOND: u128 = 1_000_000_000;

impl<T: Lerp + TwoWayConverter> Segment<T> {
    /// The value at local progress `s ∈ (0, 1)` of a segment lasting `length`.
    /// A non-finite result is replaced by `from`.
    fn sample(&self, s: f64, length: Duration) -> T {
        let sampled = match &self.motion {
            Motion::Hold | Motion::Jump => return self.from.clone(),
            Motion::Eased(curve) => {
                let eased = curve.transform(s);
                if !eased.is_finite() {
                    return self.from.clone();
                }
                let lerped = self.from.lerp_to(&self.to, eased);
                if is_finite_vector(&lerped.to_vector()) {
                    lerped
                } else {
                    // `a + (b − a)·e` overflows on `b − a` for far-apart
                    // finite ends (1e308 → -1e308); `a·(1 − e) + b·e` does
                    // not, and is still exact at both ends.
                    let mut out = self.from.to_vector();
                    let to = self.to.to_vector();
                    for (component, &b) in out.as_mut().iter_mut().zip(to.as_ref()) {
                        *component = *component * (1.0 - eased) + b * eased;
                    }
                    T::from_vector(out)
                }
            }
            Motion::Cubic {
                start_velocity,
                end_velocity,
            } => {
                let d = length.as_secs_f64();
                let (s2, s3) = (s * s, s * s * s);
                let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
                let h10 = s3 - 2.0 * s2 + s;
                let h01 = 3.0 * s2 - 2.0 * s3;
                let h11 = s3 - s2;
                let mut out = self.from.to_vector();
                let p1 = self.to.to_vector();
                for (i, component) in out.as_mut().iter_mut().enumerate() {
                    let p0 = *component;
                    *component = h00 * p0
                        + h10 * d * start_velocity.as_ref()[i]
                        + h01 * p1.as_ref()[i]
                        + h11 * d * end_velocity.as_ref()[i];
                }
                if !is_finite_vector(&out) {
                    return self.from.clone();
                }
                T::from_vector(out)
            }
        };
        if is_finite_vector(&sampled.to_vector()) {
            sampled
        } else {
            self.from.clone()
        }
    }

    /// The velocity, in units per second, at which this segment arrives at
    /// `to` (`at_end`) or leaves `from`, for a neighbouring cubic segment.
    fn velocity(&self, at_end: bool, zero: T::Vector) -> T::Vector {
        let Motion::Eased(curve) = &self.motion else {
            return zero;
        };
        let length = self.end.saturating_sub(self.start).as_secs_f64();
        if length == 0.0 {
            return zero;
        }
        let rate = curve.slope(if at_end { 1.0 } else { 0.0 }) / length;
        difference(self.to.to_vector(), self.from.to_vector(), rate)
    }
}

/// `(a − b) · scale` per component, with non-finite components as 0.
fn difference<V: AsRef<[f64]> + AsMut<[f64]> + Copy>(a: V, b: V, scale: f64) -> V {
    let mut out = a;
    for (component, &b) in out.as_mut().iter_mut().zip(b.as_ref()) {
        let value = (*component - b) * scale;
        *component = if value.is_finite() { value } else { 0.0 };
    }
    out
}

fn is_finite_vector<V: AsRef<[f64]>>(vector: &V) -> bool {
    vector
        .as_ref()
        .iter()
        .all(|component| component.is_finite())
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
        let Self {
            start,
            total,
            segments: pending,
        } = self;
        if total.is_zero() {
            return Err(KeyframesError::ZeroTotal);
        }
        if !is_finite_vector(&start.to_vector()) {
            return Err(KeyframesError::NonFiniteValue { index: 0 });
        }
        let mut zero = start.to_vector();
        zero.as_mut().fill(0.0);

        let mut segments = Vec::with_capacity(pending.len());
        let mut cursor = Duration::ZERO;
        let mut current = start.clone();
        for (index, pending) in pending.into_iter().enumerate() {
            let (value, over, motion) = match pending {
                Pending::Eased { value, over, curve } => (value, over, Motion::Eased(curve)),
                Pending::Cubic { value, over } => (
                    value,
                    over,
                    Motion::Cubic {
                        start_velocity: zero,
                        end_velocity: zero,
                    },
                ),
                Pending::Hold { over } => (current.clone(), over, Motion::Hold),
                Pending::Jump { value } => (value, Duration::ZERO, Motion::Jump),
            };
            if !is_finite_vector(&value.to_vector()) {
                return Err(KeyframesError::NonFiniteValue { index: index + 1 });
            }
            let end = cursor
                .checked_add(over)
                .ok_or(KeyframesError::DurationOverflow { index })?;
            if end > total {
                return Err(KeyframesError::Overrun { index, end, total });
            }
            segments.push(Segment {
                start: cursor,
                end,
                from: current,
                to: value.clone(),
                motion,
            });
            cursor = end;
            current = value;
        }

        // Cubic velocities: Catmull-Rom by keyframe time between cubic
        // segments, the neighbour's own velocity next to an eased segment,
        // zero at the track's ends and next to a hold or jump.
        for index in 0..segments.len() {
            if !matches!(segments[index].motion, Motion::Cubic { .. }) {
                continue;
            }
            let segment = &segments[index];
            let incoming = match index.checked_sub(1).map(|i| &segments[i]) {
                Some(previous) if matches!(previous.motion, Motion::Cubic { .. }) => catmull_rom(
                    previous.from.to_vector(),
                    previous.start,
                    segment.to.to_vector(),
                    segment.end,
                ),
                Some(previous) => previous.velocity(true, zero),
                None => zero,
            };
            let outgoing = match segments.get(index + 1) {
                Some(next) if matches!(next.motion, Motion::Cubic { .. }) => catmull_rom(
                    segment.from.to_vector(),
                    segment.start,
                    next.to.to_vector(),
                    next.end,
                ),
                Some(next) => next.velocity(false, zero),
                None => zero,
            };
            segments[index].motion = Motion::Cubic {
                start_velocity: incoming,
                end_velocity: outgoing,
            };
        }

        Ok(Keyframes {
            start,
            total,
            segments: segments.into_boxed_slice(),
        })
    }
}

/// The Catmull-Rom tangent at the knot between `(before, at_before)` and
/// `(after, at_after)`: the chord's slope, in units per second.
fn catmull_rom<V: AsRef<[f64]> + AsMut<[f64]> + Copy>(
    before: V,
    at_before: Duration,
    after: V,
    at_after: Duration,
) -> V {
    let span = at_after.saturating_sub(at_before).as_secs_f64();
    difference(after, before, 1.0 / span)
}

impl<T: Lerp + TwoWayConverter> Animatable<T> for Keyframes<T> {
    /// Reads the track at progress `t` of [`total`](Keyframes::total):
    /// `t` is clamped into `[0, 1]` and NaN reads as 0.
    fn transform(&self, t: f64) -> T {
        let elapsed = if t.is_nan() || t <= 0.0 {
            Duration::ZERO
        } else if t >= 1.0 {
            self.total
        } else {
            Duration::try_from_secs_f64(self.total.as_secs_f64() * t)
                .map_or(self.total, |elapsed| elapsed.min(self.total))
        };
        self.value_at(elapsed)
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
