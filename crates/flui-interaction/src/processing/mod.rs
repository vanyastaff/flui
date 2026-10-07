//! Input processing utilities
//!
//! This module provides utilities for processing and enhancing input events:
//!
//! - [`VelocityTracker`] - Velocity estimation from pointer movement
//! - [`PointerEventResampler`] - Resample events to consistent frame rate
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::processing::VelocityTracker;
//!
//! let mut tracker = VelocityTracker::new();
//! tracker.add_position(now, position);
//! let velocity = tracker.get_velocity();
//!
//! ```

mod lsq_solver;
mod one_euro;
mod resampler;
mod sampling_clock;
mod velocity;

// `lsq_solver` (LeastSquaresSolver / PolynomialFit / MAX_*) is crate-internal
// numerical machinery shared by the velocity tracker; it is intentionally NOT
// re-exported, so the public API is not pinned to the solver's internals.
pub use one_euro::{OneEuroFilter, OneEuroFilter2D};
pub use resampler::{DEFAULT_RESAMPLE_LOOKBACK, PointerEventResampler};
pub use sampling_clock::{DEFAULT_SAMPLE_PERIOD, SamplingClock};
pub use velocity::{
    ImpulseVelocityTracker, IosFlingVelocityTracker, MacosFlingVelocityTracker, Velocity,
    VelocityEstimate, VelocityTracker,
};
