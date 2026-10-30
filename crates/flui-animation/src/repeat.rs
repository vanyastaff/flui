//! What [`AnimationController::repeat`](crate::AnimationController::repeat)
//! runs: a [`RepeatSpec`].

use std::time::Duration;

use crate::ValueRange;

/// How a repeat moves between its endpoints from one iteration to the next.
///
/// Named as CSS `animation-direction` names it, so it is not confused with
/// bounce curves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RepeatMode {
    /// Every iteration runs from the lower endpoint up to the upper one.
    #[default]
    Restart,
    /// Iterations alternate direction: up, then back down.
    Alternate,
}

/// How many iterations a repeat runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Iterations {
    /// This many, then the run settles on the end of the last one. Zero
    /// settles at the call, where the value already is: no iteration ran,
    /// so there is none to land on.
    Finite(u32),
    /// Until the run is replaced or stopped; its future resolves only by
    /// cancellation.
    #[default]
    Infinite,
}

/// A controller repeat: its range, mode, period and iteration count.
///
/// The default restarts forever over the controller's own bounds, one
/// iteration per forward duration.
///
/// ```
/// use std::time::Duration;
/// use flui_animation::{AnimationController, Iterations, RepeatMode, RepeatSpec, ValueRange};
///
/// let controller = AnimationController::builder(Duration::from_millis(300)).build();
/// let range = ValueRange::try_new(0.2, 0.8).expect("finite, ordered range");
/// let _run = controller.repeat(
///     RepeatSpec::new(RepeatMode::Alternate)
///         .with_range(range)
///         .with_iterations(Iterations::Finite(3)),
/// )?;
/// # Ok::<(), flui_animation::AnimationError>(())
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[must_use]
pub struct RepeatSpec {
    pub(crate) range: Option<ValueRange>,
    pub(crate) mode: RepeatMode,
    pub(crate) period: Option<Duration>,
    pub(crate) iterations: Iterations,
}

impl RepeatSpec {
    /// An infinite repeat in `mode` over the controller's bounds.
    pub const fn new(mode: RepeatMode) -> Self {
        Self {
            range: None,
            mode,
            period: None,
            iterations: Iterations::Infinite,
        }
    }

    /// Repeat within `range`, clamped into the controller's bounds, instead
    /// of over the bounds themselves.
    pub const fn with_range(mut self, range: ValueRange) -> Self {
        self.range = Some(range);
        self
    }

    /// One iteration per `period` instead of the forward duration. Resolved
    /// at the call: a later duration change does not retime the run, and
    /// both legs of an alternating repeat share it. A zero period settles
    /// at the call on the end of the last iteration.
    pub const fn with_period(mut self, period: Duration) -> Self {
        self.period = Some(period);
        self
    }

    /// Run `iterations` instead of repeating forever.
    pub const fn with_iterations(mut self, iterations: Iterations) -> Self {
        self.iterations = iterations;
        self
    }
}

impl From<RepeatMode> for RepeatSpec {
    fn from(mode: RepeatMode) -> Self {
        Self::new(mode)
    }
}
