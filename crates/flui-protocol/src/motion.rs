//! Presentation animation playback requests and replies.

/// Change a window's animation rate, advance its timeline, or read its state.
/// Rate is finite and non-negative; the owner validates before applying either
/// field. An absent field leaves that part of the clock unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct MotionRequest {
    /// Animation seconds per raw second. Zero pauses.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub rate: Option<f64>,
    /// Advance animation time by these milliseconds, including while paused.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub step_ms: Option<u64>,
}

impl MotionRequest {
    /// A request that reads the current state without changing it.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rate: None,
            step_ms: None,
        }
    }

    /// Set the requested playback rate.
    #[must_use]
    pub const fn with_rate(mut self, rate: f64) -> Self {
        self.rate = Some(rate);
        self
    }

    /// Set the requested forward step.
    #[must_use]
    pub const fn with_step_ms(mut self, step_ms: u64) -> Self {
        self.step_ms = Some(step_ms);
        self
    }
}

/// A window's accepted animation clock state after serving a request.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct MotionState {
    /// Animation seconds per raw second.
    pub rate: f64,
    /// Monotonic animation time from the clock's origin, in milliseconds.
    pub time_ms: f64,
}

impl MotionState {
    /// Build a reply from the owner's accepted clock state.
    #[must_use]
    pub const fn new(rate: f64, time_ms: f64) -> Self {
        Self { rate, time_ms }
    }
}
