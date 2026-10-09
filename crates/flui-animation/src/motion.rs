//! A presentation's animation clock: raw frame timestamps in, a monotonic,
//! finite animation time out.
//!
//! [`MotionClock`] is a plain value — no lock, no `Arc`, no process-global
//! state. Each presentation (and each test double of one) owns its own and
//! feeds it the raw timestamp of every frame it draws; the clock answers with
//! a [`FrameTick`], the only way to obtain one.
//!
//! # Time model
//!
//! Animation time is `epoch_time + (raw − epoch_raw) · rate`. Changing the
//! rate rebases the epoch on the last raw time the clock accepted, so the
//! timeline is continuous: time before the change ran at the old rate, time
//! after it runs at the new one, and the interval between the last frame and
//! the change is never lost or rewritten. [`MotionClock::step`] moves the
//! epoch forward by an exact amount regardless of the rate.
//!
//! Animation time never decreases: a raw timestamp earlier than the last
//! accepted one holds the timeline (the tick repeats the previous time), and
//! every sum saturates at [`Duration::MAX`] instead of overflowing.
//!
//! A controller's local playback rate composes with its presentation clock.
//! Each rate change preserves elapsed time already accepted at the old rate.
//! Presentation clocks and controller rates have no process-global state.

use std::time::Duration;

/// Application policy applied to the host's motion observation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MotionPreference {
    /// Honor the current host preference and duration scale.
    #[default]
    FollowSystem,
    /// Reduce animation regardless of the host preference.
    Reduce,
    /// Use full animation at its authored duration.
    Full,
}

/// The resolved presentation policy, independent of its duration scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MotionPolicy {
    /// Sample normal animation using its projected duration timeline.
    #[default]
    Full,
    /// Settle normal animation at the next registry tick.
    Reduce,
}

/// Animation time of one presentation, measured from the origin of its
/// [`MotionClock`].
///
/// Finite and never decreasing within one clock: only a [`MotionClock`]
/// produces it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationTime(Duration);

impl AnimationTime {
    /// The clock's origin.
    pub const ZERO: Self = Self(Duration::ZERO);

    /// This time as a span from the clock's origin.
    #[must_use]
    pub const fn as_duration(self) -> Duration {
        self.0
    }

    /// Elapsed animation time, holding at zero for an earlier timestamp.
    #[must_use]
    pub const fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

/// How fast animation time runs relative to raw frame time: finite and
/// non-negative; `0` pauses.
///
/// # Examples
///
/// ```
/// use flui_animation::{InvalidPlaybackRate, PlaybackRate};
///
/// let slow = PlaybackRate::new(0.1).expect("0.1 is a valid rate");
/// assert_eq!(slow.get(), 0.1);
/// assert!(PlaybackRate::PAUSED.is_paused());
/// assert_eq!(PlaybackRate::new(-1.0), Err(InvalidPlaybackRate::Negative(-1.0)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct PlaybackRate(f64);

impl PlaybackRate {
    /// Animation time runs at raw frame time.
    pub const NORMAL: Self = Self(1.0);
    /// Animation time stands still.
    pub const PAUSED: Self = Self(0.0);

    /// A rate of `rate` animation seconds per raw second.
    ///
    /// `-0.0` is accepted as [`PlaybackRate::PAUSED`].
    ///
    /// # Errors
    ///
    /// [`InvalidPlaybackRate::NonFinite`] for NaN or ±∞,
    /// [`InvalidPlaybackRate::Negative`] for a rate below zero.
    pub fn new(rate: f64) -> Result<Self, InvalidPlaybackRate> {
        if !rate.is_finite() {
            return Err(InvalidPlaybackRate::NonFinite(rate));
        }
        if rate < 0.0 {
            return Err(InvalidPlaybackRate::Negative(rate));
        }
        // `-0.0 + 0.0` is `+0.0`: one representation of the paused rate.
        Ok(Self(rate + 0.0))
    }

    /// Animation seconds per raw second.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// Whether this rate stands time still.
    #[must_use]
    pub fn is_paused(self) -> bool {
        self.0 == 0.0
    }
}

impl Default for PlaybackRate {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl TryFrom<f64> for PlaybackRate {
    type Error = InvalidPlaybackRate;

    fn try_from(rate: f64) -> Result<Self, Self::Error> {
        Self::new(rate)
    }
}

/// Why a value is not a [`PlaybackRate`].
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidPlaybackRate {
    /// The rate was NaN or infinite.
    #[error("playback rate {0} is not finite")]
    NonFinite(f64),
    /// The rate was below zero.
    #[error("playback rate {0} is negative")]
    Negative(f64),
}

/// One frame of a presentation's animation time.
///
/// Only [`MotionClock::frame`] produces one, so the time it carries is finite
/// and never earlier than the previous tick of the same clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTick {
    now: AnimationTime,
    normal: AnimationTime,
    policy: MotionPolicy,
}

impl FrameTick {
    /// The animation time this frame samples.
    #[must_use]
    pub const fn now(&self) -> AnimationTime {
        self.now
    }

    pub(crate) const fn time(&self, behavior: crate::AnimationBehavior) -> AnimationTime {
        match behavior {
            crate::AnimationBehavior::Normal => self.normal,
            crate::AnimationBehavior::Preserve => self.now,
        }
    }

    pub(crate) fn hold_after(mut self, earlier: Self) -> Self {
        self.now = self.now.max(earlier.now);
        self.normal = self.normal.max(earlier.normal);
        self
    }

    pub(crate) const fn policy(&self) -> MotionPolicy {
        self.policy
    }
}

/// A presentation's animation clock. See the [module docs](self) for the time
/// model.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use flui_animation::{MotionClock, PlaybackRate};
///
/// let mut clock = MotionClock::new();
/// let ms = Duration::from_millis;
/// assert_eq!(clock.frame(ms(100)).now().as_duration(), ms(100));
///
/// // Half speed from the last frame on: no jump at the change.
/// clock.set_rate(PlaybackRate::new(0.5).expect("0.5 is a valid rate"));
/// assert_eq!(clock.frame(ms(300)).now().as_duration(), ms(200));
///
/// // A timestamp from the past holds the timeline.
/// assert_eq!(clock.frame(ms(50)).now().as_duration(), ms(200));
///
/// // A step is exact at any rate, even paused.
/// clock.set_rate(PlaybackRate::PAUSED);
/// assert_eq!(clock.step(ms(16)).as_duration(), ms(216));
/// assert_eq!(clock.frame(ms(10_000)).now().as_duration(), ms(216));
/// ```
#[derive(Clone, Debug)]
pub struct MotionClock {
    rate: PlaybackRate,
    /// Raw time at which the current rate took effect.
    epoch_raw: Duration,
    /// Animation time at `epoch_raw`, plus every step since.
    epoch_time: Duration,
    /// The latest raw time accepted.
    last_raw: Duration,
    /// The latest animation time produced.
    now: AnimationTime,
    preference: MotionPreference,
    system: flui_platform_api::MotionPreference,
    normal_epoch: AnimationTime,
    normal_epoch_time: Duration,
    normal: AnimationTime,
}

impl Default for MotionClock {
    fn default() -> Self {
        Self {
            rate: PlaybackRate::NORMAL,
            epoch_raw: Duration::ZERO,
            epoch_time: Duration::ZERO,
            last_raw: Duration::ZERO,
            now: AnimationTime::ZERO,
            preference: MotionPreference::FollowSystem,
            system: flui_platform_api::MotionPreference::NoPreference,
            normal_epoch: AnimationTime::ZERO,
            normal_epoch_time: Duration::ZERO,
            normal: AnimationTime::ZERO,
        }
    }
}

impl MotionClock {
    /// A clock at [`AnimationTime::ZERO`] running at [`PlaybackRate::NORMAL`],
    /// whose raw origin is raw time zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Map the raw timestamp of a frame, measured from the same origin as
    /// every earlier one, to this frame's animation time.
    ///
    /// A `raw` earlier than the last accepted one holds the timeline: the
    /// tick repeats the current time and the clock is unchanged. Overflow
    /// saturates at [`Duration::MAX`]. Never panics.
    pub fn frame(&mut self, raw: Duration) -> FrameTick {
        if raw < self.last_raw {
            return self.tick();
        }
        self.last_raw = raw;
        let scaled = scale(raw.saturating_sub(self.epoch_raw), self.rate);
        let candidate = AnimationTime(self.epoch_time.saturating_add(scaled));
        // The epoch formula is already monotone in `raw` within one epoch and
        // continuous across a rebase; `max` keeps that true under saturation.
        self.now = self.now.max(candidate);
        self.project_normal();
        self.tick()
    }

    fn tick(&self) -> FrameTick {
        FrameTick {
            now: self.now,
            normal: self.normal,
            policy: self.policy(),
        }
    }

    /// Apply application policy continuously from the last accepted frame.
    /// Returns whether the preference changed; no controller is sampled here.
    pub fn set_preference(&mut self, preference: MotionPreference) -> bool {
        if self.preference != preference {
            self.rebase_normal();
            self.preference = preference;
            true
        } else {
            false
        }
    }

    /// Apply the existing host preference after runtime projection.
    /// Returns whether the observation changed; no controller is sampled here.
    pub fn set_system_motion(&mut self, system: flui_platform_api::MotionPreference) -> bool {
        if self.system != system {
            self.rebase_normal();
            self.system = system;
            true
        } else {
            false
        }
    }

    /// Resolve the application preference against the current host observation.
    #[must_use]
    pub fn policy(&self) -> MotionPolicy {
        match self.preference {
            MotionPreference::Reduce => MotionPolicy::Reduce,
            MotionPreference::Full => MotionPolicy::Full,
            MotionPreference::FollowSystem => match self.system {
                flui_platform_api::MotionPreference::Reduce => MotionPolicy::Reduce,
                _ => MotionPolicy::Full,
            },
        }
    }

    fn rebase_normal(&mut self) {
        self.normal_epoch = self.now;
        self.normal_epoch_time = self.normal.0;
    }

    fn project_normal(&mut self) {
        if self.policy() == MotionPolicy::Reduce {
            return;
        }
        let span = self.now.saturating_duration_since(self.normal_epoch);
        let span = match (self.preference, self.system) {
            (
                MotionPreference::FollowSystem,
                flui_platform_api::MotionPreference::Scaled(scale),
            ) => Duration::try_from_secs_f64(span.as_secs_f64() / scale.get())
                .unwrap_or(Duration::MAX),
            _ => span,
        };
        self.normal = self
            .normal
            .max(AnimationTime(self.normal_epoch_time.saturating_add(span)));
    }

    /// The animation time of the latest tick or step.
    #[must_use]
    pub const fn now(&self) -> AnimationTime {
        self.now
    }

    /// The current rate.
    #[must_use]
    pub const fn rate(&self) -> PlaybackRate {
        self.rate
    }

    /// Run at `rate` from the last accepted raw time on.
    ///
    /// The epoch is rebased there, so animation time is continuous across
    /// the change and the interval since the last frame runs at `rate`.
    pub fn set_rate(&mut self, rate: PlaybackRate) {
        self.epoch_raw = self.last_raw;
        self.epoch_time = self.now.0;
        self.rate = rate;
    }

    /// Advance animation time by exactly `dt` at any rate, paused included,
    /// and return the new time. Saturates at [`Duration::MAX`].
    pub fn step(&mut self, dt: Duration) -> AnimationTime {
        self.epoch_time = self.epoch_time.saturating_add(dt);
        self.now = AnimationTime(self.now.0.saturating_add(dt));
        self.project_normal();
        self.now
    }

    /// Whether the rate stands time still (only [`step`](Self::step) moves it).
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.rate.is_paused()
    }
}

/// `span · rate`, saturating at [`Duration::MAX`]. The normal rate is exact.
fn scale(span: Duration, rate: PlaybackRate) -> Duration {
    if rate == PlaybackRate::NORMAL {
        return span;
    }
    if rate.is_paused() {
        return Duration::ZERO;
    }
    // Both factors are finite and non-negative, so the product is either a
    // representable duration or too large for one (including +∞).
    Duration::try_from_secs_f64(span.as_secs_f64() * rate.get()).unwrap_or(Duration::MAX)
}
