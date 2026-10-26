//! What a haptic request names and what it reports (ADR-0210 §3).

use std::num::NonZeroU64;

use flui_foundation::InputSequenceId;

use crate::{DeviceId, Unsupported};

/// When a haptic request plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum HapticTiming {
    /// Now: the request returns its final outcome.
    #[default]
    Immediate,
    /// With the next frame the presentation presents, so the feedback lands
    /// with the change it confirms. The frame queue returns
    /// [`HapticOutcome::Queued`] and delivers the final outcome later; a
    /// request whose frame is not presented by its deadline is
    /// [`HapticOutcome::Suppressed`], never played late.
    FramePresented,
}

/// Where, when and in which input sequence a haptic request plays.
///
/// ```
/// use flui_platform::haptics::{HapticRequest, HapticTiming};
///
/// let request = HapticRequest::new(HapticTiming::FramePresented);
/// assert_eq!(request.source, None);
/// assert_eq!(request.sequence, None);
/// assert_eq!(HapticRequest::default().timing, HapticTiming::Immediate);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct HapticRequest {
    /// The device whose actuator plays it: a pen or touchpad that has one.
    /// `None`, or a device the backend no longer knows, plays on the window's
    /// default actuator (ADR-0210 §5).
    pub source: Option<DeviceId>,
    /// When it plays.
    pub timing: HapticTiming,
    /// The input sequence it answers, as diagnostic context: the widget
    /// handle fills it in during dispatch and leaves it `None` outside one.
    pub sequence: Option<InputSequenceId>,
}

impl HapticRequest {
    /// A request at `timing` on the window's actuator, outside any sequence.
    #[must_use]
    pub const fn new(timing: HapticTiming) -> Self {
        Self {
            source: None,
            timing,
            sequence: None,
        }
    }

    /// This request played on `source`'s actuator.
    #[must_use]
    pub const fn with_source(self, source: DeviceId) -> Self {
        Self {
            source: Some(source),
            ..self
        }
    }

    /// This request tagged with the input sequence it answers.
    #[must_use]
    pub const fn with_sequence(self, sequence: InputSequenceId) -> Self {
        Self {
            sequence: Some(sequence),
            ..self
        }
    }

    /// This request at `timing`.
    #[must_use]
    pub const fn with_timing(self, timing: HapticTiming) -> Self {
        Self { timing, ..self }
    }
}

/// The ordinal of a frame-queued request, unique within its presentation and
/// never reused there: the final outcome is delivered with the ticket the
/// request returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HapticTicket(NonZeroU64);

impl HapticTicket {
    /// The ticket of ordinal `ordinal`.
    #[must_use]
    pub const fn new(ordinal: NonZeroU64) -> Self {
        Self(ordinal)
    }

    /// The ordinal.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// What became of a haptic request. Every variant is a normal result, so this
/// is an enum, not a `Result` (ADR-0210 §3).
#[must_use = "a haptic outcome says whether the feedback played"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HapticOutcome {
    /// Handed to the actuator as asked.
    Submitted,
    /// Queued for the next presented frame; the final outcome follows with
    /// this ticket. Only the frame queue returns it, never a backend.
    Queued(HapticTicket),
    /// Played, with lost fidelity: a nearer kind, a dropped sharpness, a
    /// pulse for an envelope.
    Approximated,
    /// The pattern could not play; its tier-1 fallback did.
    FellBack,
    /// Deliberately not played: the user turned haptics off, a subtree
    /// switched them off, the window was hidden, or the frame it waited for
    /// was not presented in time.
    Suppressed,
    /// No actuator can play it, and [why](Unsupported::reason): the platform
    /// or the device has none, none is in range now, or its window is gone.
    Unsupported(Unsupported),
}

impl HapticOutcome {
    /// Whether the device vibrated: [`Submitted`](Self::Submitted),
    /// [`Approximated`](Self::Approximated) or [`FellBack`](Self::FellBack).
    #[must_use]
    pub const fn played(self) -> bool {
        matches!(self, Self::Submitted | Self::Approximated | Self::FellBack)
    }

    /// What is not here, when that is why nothing played.
    #[must_use]
    pub const fn unsupported(self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(unsupported),
            Self::Submitted
            | Self::Queued(_)
            | Self::Approximated
            | Self::FellBack
            | Self::Suppressed => None,
        }
    }
}

impl From<Unsupported> for HapticOutcome {
    fn from(unsupported: Unsupported) -> Self {
        Self::Unsupported(unsupported)
    }
}

/// How faithfully an actuator plays a [`HapticPattern`](super::HapticPattern).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PatternPlayback {
    /// As built.
    Exact,
    /// With lost fidelity ([`HapticOutcome::Approximated`]).
    Approximated,
    /// Not at all: patterns play their fallback ([`HapticOutcome::FellBack`]).
    FallsBack,
}

/// What one actuator can play. Read when the device set changes, never per
/// request (ADR-0210 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct HapticCapabilities {
    /// Whether it plays semantic [`HapticFeedback`](crate::HapticFeedback).
    pub feedback: bool,
    /// How it plays patterns.
    pub patterns: PatternPlayback,
    /// Whether a pattern's intensity reaches the actuator.
    pub intensity: bool,
    /// Whether a pattern's sharpness reaches the actuator.
    pub sharpness: bool,
}

impl HapticCapabilities {
    /// No actuator.
    pub const NONE: Self = Self {
        feedback: false,
        patterns: PatternPlayback::FallsBack,
        intensity: false,
        sharpness: false,
    };

    /// Semantic feedback only; patterns fall back.
    pub const FEEDBACK_ONLY: Self = Self {
        feedback: true,
        ..Self::NONE
    };

    /// Everything, exactly.
    pub const FULL: Self = Self {
        feedback: true,
        patterns: PatternPlayback::Exact,
        intensity: true,
        sharpness: true,
    };

    /// Capabilities with pattern playback `patterns` and whether intensity
    /// and sharpness reach the actuator.
    #[must_use]
    pub const fn new(
        feedback: bool,
        patterns: PatternPlayback,
        intensity: bool,
        sharpness: bool,
    ) -> Self {
        Self {
            feedback,
            patterns,
            intensity,
            sharpness,
        }
    }
}
