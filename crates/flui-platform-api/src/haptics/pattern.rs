//! Haptic patterns, the second tier (ADR-0210 §2).
//!
//! A [`HapticPattern`] is the model Core Haptics and Android's API 36 envelopes
//! share: transient taps and continuous events whose [`Intensity`] and
//! [`Sharpness`] follow a [`ParameterCurve`] of control points. A pattern is
//! built only by [`HapticPattern::builder`], which checks every rule once and
//! returns an error at construction, never at play time, then frozen as an
//! immutable `Arc<[HapticEvent]>` that crosses the `Send + Sync` platform
//! boundary.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// A value was not a finite number in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("{value} is not a finite value in 0.0..=1.0")]
pub struct UnitRangeError {
    value: f32,
}

impl UnitRangeError {
    /// The refused value.
    #[must_use]
    pub const fn value(&self) -> f32 {
        self.value
    }
}

macro_rules! unit_value {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
        pub struct $name(f32);

        impl $name {
            /// The lowest value.
            pub const MIN: Self = Self(0.0);
            /// The highest value.
            pub const MAX: Self = Self(1.0);

            /// `value`, or `None` unless it is finite and in `0.0..=1.0`.
            /// Negative zero is admitted as zero.
            #[must_use]
            pub const fn new(value: f32) -> Option<Self> {
                // NaN fails both comparisons and an infinity fails one.
                if value >= 0.0 && value <= 1.0 {
                    Some(Self(value + 0.0))
                } else {
                    None
                }
            }

            /// The value, in `0.0..=1.0`.
            #[must_use]
            pub const fn get(self) -> f32 {
                self.0
            }
        }

        impl TryFrom<f32> for $name {
            type Error = UnitRangeError;

            fn try_from(value: f32) -> Result<Self, Self::Error> {
                Self::new(value).ok_or(UnitRangeError { value })
            }
        }
    };
}

unit_value!(
    /// How strong a haptic event is: a finite `f32` in `0.0..=1.0`, the
    /// normalized scale Core Haptics and Android envelopes share.
    Intensity
);

unit_value!(
    /// How crisp a haptic event feels, from round and soft (`0.0`) to sharp
    /// and precise (`1.0`): a finite `f32` in `0.0..=1.0`.
    Sharpness
);

/// One point of a [`ParameterCurve`]: the intensity and sharpness reached
/// `offset` after the continuous event starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlPoint {
    /// Time since the start of the continuous event.
    pub offset: Duration,
    /// The intensity reached at `offset`.
    pub intensity: Intensity,
    /// The sharpness reached at `offset`.
    pub sharpness: Sharpness,
}

/// The envelope of a continuous haptic event: control points ordered by
/// strictly increasing positive offset, ending at zero intensity, so every
/// continuous event stops on its own. Before its first point the event holds
/// that point's values.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterCurve {
    points: Box<[ControlPoint]>,
}

impl ParameterCurve {
    /// The control points, in order.
    #[must_use]
    pub fn points(&self) -> &[ControlPoint] {
        &self.points
    }

    /// How long the event lasts: its last control point's offset.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.points
            .last()
            .map_or(Duration::ZERO, |point| point.offset)
    }
}

/// One event of a [`HapticPattern`].
#[derive(Debug, Clone, PartialEq)]
pub enum HapticEvent {
    /// A single tap `at` the given time after the pattern starts.
    Transient {
        /// Time since the pattern starts.
        at: Duration,
        /// How strong.
        intensity: Intensity,
        /// How crisp.
        sharpness: Sharpness,
    },
    /// A sustained vibration starting `at` the given time, shaped by `curve`.
    Continuous {
        /// Time since the pattern starts.
        at: Duration,
        /// Its envelope.
        curve: ParameterCurve,
    },
}

impl HapticEvent {
    /// When the event starts, after the pattern starts.
    #[must_use]
    pub const fn at(&self) -> Duration {
        match self {
            Self::Transient { at, .. } | Self::Continuous { at, .. } => *at,
        }
    }

    /// When the event ends, after the pattern starts; a transient ends where
    /// it starts. `None` past `Duration::MAX`.
    fn end(&self) -> Option<Duration> {
        match self {
            Self::Transient { at, .. } => Some(*at),
            Self::Continuous { at, curve } => at.checked_add(curve.duration()),
        }
    }
}

/// Why a [`HapticPatternBuilder`] refused its pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HapticPatternError {
    /// The event at `index` starts before the one before it, or a control
    /// point's offset is not positive and later than the point before it.
    #[error("event {index} is out of order")]
    Unordered {
        /// The index of the event that is out of order, or of the continuous
        /// event whose curve is.
        index: usize,
    },
    /// An intensity or sharpness was NaN or infinite.
    #[error("an intensity or sharpness is not finite")]
    NonFinite,
    /// An intensity or sharpness was finite but outside `0.0..=1.0`.
    #[error("an intensity or sharpness is outside 0.0..=1.0")]
    OutOfRange,
    /// A continuous event's curve has no control point, or its last one is
    /// not at zero intensity.
    #[error("a continuous event's envelope does not end at zero intensity")]
    EnvelopeNotClosed,
    /// The pattern ends later than `max` after it starts.
    #[error("the pattern is longer than {max:?}")]
    TooLong {
        /// The longest pattern admitted, [`HapticPattern::MAX_LENGTH`].
        max: Duration,
    },
}

/// An immutable, validated haptic pattern (ADR-0210 §2).
///
/// Cheap to clone: the events are shared.
///
/// ```
/// use std::time::Duration;
/// use flui_platform_api::haptics::{HapticPattern, HapticPatternError};
///
/// let pattern = HapticPattern::builder()
///     .transient(Duration::ZERO, 1.0, 0.8)
///     .continuous(
///         Duration::from_millis(20),
///         [(Duration::from_millis(40), 0.6, 0.3), (Duration::from_millis(120), 0.0, 0.3)],
///     )
///     .build()?;
/// assert_eq!(pattern.events().len(), 2);
/// assert_eq!(pattern.duration(), Duration::from_millis(140));
///
/// let open = HapticPattern::builder()
///     .continuous(Duration::ZERO, [(Duration::from_millis(50), 0.5, 0.5)])
///     .build();
/// assert_eq!(open, Err(HapticPatternError::EnvelopeNotClosed));
/// # Ok::<(), HapticPatternError>(())
/// ```
#[derive(Clone, PartialEq)]
pub struct HapticPattern {
    events: Arc<[HapticEvent]>,
}

impl HapticPattern {
    /// The longest pattern admitted. Chromium caps a vibration at ten seconds
    /// (`kVibrationDurationMax`), the strictest of the backends' limits, so a
    /// pattern that plays on one backend is never refused by another for its
    /// length.
    pub const MAX_LENGTH: Duration = Duration::from_secs(10);

    /// Start an empty pattern.
    pub fn builder() -> HapticPatternBuilder {
        HapticPatternBuilder {
            events: Vec::new(),
            error: None,
        }
    }

    /// The events, ordered by start time.
    #[must_use]
    pub fn events(&self) -> &[HapticEvent] {
        &self.events
    }

    /// When the last event ends, after the pattern starts.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.events
            .iter()
            .filter_map(HapticEvent::end)
            .max()
            .unwrap_or(Duration::ZERO)
    }
}

impl fmt::Debug for HapticPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.events.iter()).finish()
    }
}

/// Builds a [`HapticPattern`], keeping the first rule an event breaks.
///
/// Consuming: each call takes the builder and returns it, and
/// [`build`](Self::build) reports the first error any call recorded.
#[derive(Debug, Clone)]
#[must_use = "a builder does nothing until `build` is called"]
pub struct HapticPatternBuilder {
    events: Vec<HapticEvent>,
    error: Option<HapticPatternError>,
}

fn unit<T>(value: f32, admit: fn(f32) -> Option<T>) -> Result<T, HapticPatternError> {
    if !value.is_finite() {
        return Err(HapticPatternError::NonFinite);
    }
    admit(value).ok_or(HapticPatternError::OutOfRange)
}

impl HapticPatternBuilder {
    /// Add a transient tap `at` the given time with raw intensity and
    /// sharpness, each checked to be finite and in `0.0..=1.0`.
    pub fn transient(self, at: Duration, intensity: f32, sharpness: f32) -> Self {
        self.push(|| {
            Ok(HapticEvent::Transient {
                at,
                intensity: unit(intensity, Intensity::new)?,
                sharpness: unit(sharpness, Sharpness::new)?,
            })
        })
    }

    /// Add a continuous event starting `at` the given time, whose envelope is
    /// `points` of `(offset, intensity, sharpness)`, offsets after `at`.
    pub fn continuous(
        self,
        at: Duration,
        points: impl IntoIterator<Item = (Duration, f32, f32)>,
    ) -> Self {
        let points: Vec<_> = points.into_iter().collect();
        self.push(|| {
            let points = points
                .into_iter()
                .map(|(offset, intensity, sharpness)| {
                    Ok(ControlPoint {
                        offset,
                        intensity: unit(intensity, Intensity::new)?,
                        sharpness: unit(sharpness, Sharpness::new)?,
                    })
                })
                .collect::<Result<Box<[_]>, _>>()?;
            Ok(HapticEvent::Continuous {
                at,
                curve: ParameterCurve { points },
            })
        })
    }

    /// Add an already typed event; its order, envelope and length are
    /// checked like any other.
    pub fn event(self, event: HapticEvent) -> Self {
        self.push(|| Ok(event))
    }

    fn push(mut self, event: impl FnOnce() -> Result<HapticEvent, HapticPatternError>) -> Self {
        if self.error.is_some() {
            return self;
        }
        let index = self.events.len();
        match event().and_then(|event| self.check(index, &event).map(|()| event)) {
            Ok(event) => self.events.push(event),
            Err(error) => self.error = Some(error),
        }
        self
    }

    fn check(&self, index: usize, event: &HapticEvent) -> Result<(), HapticPatternError> {
        if self
            .events
            .last()
            .is_some_and(|previous| previous.at() > event.at())
        {
            return Err(HapticPatternError::Unordered { index });
        }
        if let HapticEvent::Continuous { curve, .. } = event {
            let mut previous = Duration::ZERO;
            for point in curve.points() {
                if point.offset <= previous {
                    return Err(HapticPatternError::Unordered { index });
                }
                previous = point.offset;
            }
            if curve
                .points()
                .last()
                .is_none_or(|last| last.intensity != Intensity::MIN)
            {
                return Err(HapticPatternError::EnvelopeNotClosed);
            }
        }
        let max = HapticPattern::MAX_LENGTH;
        if event.end().is_none_or(|end| end > max) {
            return Err(HapticPatternError::TooLong { max });
        }
        Ok(())
    }

    /// The pattern, or the first rule an event broke.
    ///
    /// # Errors
    ///
    /// The first [`HapticPatternError`] any call recorded.
    pub fn build(self) -> Result<HapticPattern, HapticPatternError> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(HapticPattern {
                events: self.events.into(),
            }),
        }
    }
}
