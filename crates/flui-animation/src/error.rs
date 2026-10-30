//! Error types for the animation system.

/// Wire encoding for diagnostic numbers, including refused non-finite inputs.
/// Binary formats retain native floats; human-readable formats use numeric
/// finite values and exact non-finite tokens. NaN payload bits are canonicalized.
#[cfg(feature = "serde")]
pub(crate) mod diagnostic_number {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        if !serializer.is_human_readable() || value.is_finite() {
            serializer.serialize_f64(*value)
        } else {
            serializer.serialize_str(if value.is_nan() {
                "NaN"
            } else if value.is_sign_negative() {
                "-Infinity"
            } else {
                "Infinity"
            })
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        if !deserializer.is_human_readable() {
            return f64::deserialize(deserializer);
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Number {
            Finite(f64),
            Token(String),
        }
        match Number::deserialize(deserializer)? {
            Number::Finite(value) => Ok(value),
            Number::Token(token) => match token.as_str() {
                "NaN" => Ok(f64::NAN),
                "Infinity" => Ok(f64::INFINITY),
                "-Infinity" => Ok(f64::NEG_INFINITY),
                _ => Err(D::Error::custom(
                    "expected a number, NaN, Infinity or -Infinity",
                )),
            },
        }
    }
}

/// Why endpoints cannot form a finite, increasing [`ValueRange`](crate::ValueRange).
///
/// Value construction reports only range defects, independently of controller
/// lifetime or run admission. Non-finite endpoints take precedence over ordering;
/// finite endpoints must be ordered before their span is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum RangeDefect {
    /// At least one endpoint is NaN or infinite.
    #[error("a range endpoint is not finite")]
    NonFinite,
    /// The lower endpoint is greater than or equal to the upper endpoint.
    #[error("the lower endpoint must be less than the upper endpoint")]
    Inverted,
    /// The ordered finite endpoints have a difference that overflows `f64`.
    #[error("the range span overflows f64")]
    SpanOverflow,
}

/// The value-space input that a motion admission could not use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnimationInput {
    /// A value supplied when creating an animated value.
    InitialValue,
    /// An explicit starting value for a run.
    From,
    /// The destination of a run.
    Target,
    /// A caller-supplied velocity.
    Velocity,
    /// Velocity sampled from the preceding run at an interruption.
    InheritedVelocity,
    /// The initial position returned by a custom simulation.
    SimulationStart,
}

impl std::fmt::Display for AnimationInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InitialValue => "initial value",
            Self::From => "starting value",
            Self::Target => "target",
            Self::Velocity => "velocity",
            Self::InheritedVelocity => "inherited velocity",
            Self::SimulationStart => "simulation start",
        })
    }
}

/// Errors that can occur when using an [`AnimationController`](crate::AnimationController).
///
/// This enum represents all possible error conditions that can occur
/// during animation operations.
///
/// # Examples
///
/// ```
/// use flui_animation::{AnimationController, AnimationError};
/// use std::time::Duration;
///
/// let mut owner = AnimationController::builder(Duration::from_millis(300)).build_on(None);
/// let controller = owner.controller().clone();
///
/// // The owner closes the shared controller.
/// owner.dispose();
///
/// // Now operations will return AnimationError::Disposed
/// let result = controller.forward();
/// assert!(matches!(result, Err(AnimationError::Disposed)));
/// ```
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AnimationError {
    /// The [`AnimationController`](crate::AnimationController) has been disposed.
    ///
    /// This error occurs when attempting to use a controller after
    /// retiring its [`DrivenController`](crate::DrivenController) owner.
    #[error("AnimationController has been disposed")]
    Disposed,

    /// A mutator was called while the presentation the controller ticks for
    /// was building (ADR-0086 §7). The run, value and listeners are unchanged;
    /// mutate from an event callback or a lifecycle hook instead.
    #[error("AnimationController was mutated during build")]
    MutatedDuringBuild,

    /// The active run has no declared finite endpoint to finish at.
    /// Infinite repeats and arbitrary custom simulations refuse explicit
    /// completion without changing the run, its sample or its future.
    #[error("the active animation has no declared finite endpoint")]
    NoFiniteEndpoint,

    /// This controller permanently consumed its run or sample identities.
    /// New runs refuse before mutation; a final admitted run may still finish.
    #[error("AnimationController identities exhausted")]
    IdentityExhausted,

    /// User motion callbacks repeatedly changed the sampled controller before admission.
    /// The latest installed run remains intact; the caller can retry later.
    #[error("motion changed reentrantly while preparing a retarget")]
    ReentrantMotion,

    /// A grouped update encountered an ignored refusal or a caught panic
    /// during preparation. None of its prepared changes can be admitted.
    #[error("grouped motion preparation was interrupted")]
    AdmissionAborted,

    /// A gesture extent must be finite and strictly positive.
    #[error("gesture extent must be finite and strictly positive")]
    InvalidExtent,

    /// A repeat's range cannot be admitted within the controller bounds.
    /// NaN inputs are reported before clamping; other endpoints are effective
    /// values after defaulting and clamping. The existing run remains unchanged.
    #[error("invalid repeat range [{min}, {max}] within [{lower}, {upper}]: {defect}")]
    InvalidRepeatRange {
        /// The refused lower repeat endpoint.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        min: f64,
        /// The refused upper repeat endpoint.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        max: f64,
        /// The controller's lower bound.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        lower: f64,
        /// The controller's upper bound.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        upper: f64,
        /// The matchable reason the range is invalid.
        defect: RangeDefect,
    },

    /// An oscillating spring cannot drive a bounded fling.
    /// Use a custom simulation for oscillating motion.
    #[error("fling requires a non-oscillating spring; damping ratio is {damping_ratio}")]
    UnderdampedFlingSpring {
        /// The admitted spring's damping ratio, below one.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        damping_ratio: f64,
    },

    /// The physical simulation could not represent the admitted finite inputs.
    /// A refused fling leaves the preceding run and its future unchanged.
    #[error("cannot construct animation simulation: {0}")]
    Simulation(#[from] crate::simulation::SimulationError),

    /// A scalar input or sampled velocity is not finite.
    /// Bounded `target` and `from` infinities still clamp to their finite bound.
    #[error("{input} is not finite: {value}")]
    NonFiniteInput {
        /// Which input failed admission.
        input: AnimationInput,
        /// Its refused value.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        value: f64,
    },

    /// An input points toward an infinite bound rather than a finite endpoint.
    #[error("{input} = {value} runs toward an infinite bound")]
    UnboundedTarget {
        /// Which input selected the missing endpoint.
        input: AnimationInput,
        /// The input value or requested bound.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        value: f64,
    },

    /// Finite endpoints have a difference that cannot be represented.
    #[error("span {from} -> {to} overflows f64")]
    SpanOverflow {
        /// The admitted starting value.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        from: f64,
        /// The requested ending value.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        to: f64,
    },

    /// Converting a finite gesture velocity to controller units overflows.
    #[error("velocity {velocity} across extent {extent} and span {span} overflows f64")]
    VelocityOverflow {
        /// The velocity in gesture units per second.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        velocity: f64,
        /// The positive gesture extent.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        extent: f64,
        /// The finite controller span.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        span: f64,
    },

    /// A converter produced a non-finite component before admission.
    #[error("{input} component {component} is not finite: {value}")]
    NonFiniteComponent {
        /// Which value was converted.
        input: AnimationInput,
        /// Its zero-based vector component.
        component: usize,
        /// The refused component value.
        #[cfg_attr(feature = "serde", serde(with = "diagnostic_number"))]
        value: f64,
    },

    /// A vector component's physical motion could not be represented.
    #[error("component {component} motion could not be constructed: {source}")]
    ComponentSimulation {
        /// The zero-based vector component.
        component: usize,
        /// The original typed construction failure.
        #[source]
        source: crate::simulation::SimulationError,
    },
}

impl AnimationError {
    /// A stable, machine-readable name for this error's variant: the `kind`
    /// field of a refused command's diagnostic (ADR-0220). A kind keeps its
    /// name across releases, so filters and dashboards can match on it.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Disposed => "disposed",
            Self::MutatedDuringBuild => "mutated_during_build",
            Self::NoFiniteEndpoint => "no_finite_endpoint",
            Self::IdentityExhausted => "identity_exhausted",
            Self::ReentrantMotion => "reentrant_motion",
            Self::AdmissionAborted => "admission_aborted",
            Self::InvalidExtent => "invalid_extent",
            Self::InvalidRepeatRange { .. } => "invalid_repeat_range",
            Self::UnderdampedFlingSpring { .. } => "underdamped_fling_spring",
            Self::Simulation(_) => "simulation",
            Self::NonFiniteInput { .. } => "non_finite_input",
            Self::UnboundedTarget { .. } => "unbounded_target",
            Self::SpanOverflow { .. } => "span_overflow",
            Self::VelocityOverflow { .. } => "velocity_overflow",
            Self::NonFiniteComponent { .. } => "non_finite_component",
            Self::ComponentSimulation { .. } => "component_simulation",
        }
    }
}
