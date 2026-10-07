//! Input processing utilities
//!
//! This module provides utilities for processing and enhancing input events:
//!
//! - [`VelocityTracker`] - Velocity estimation from pointer movement
//! - [`PointerEventResampler`] - Resample events to consistent frame rate
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::{PointerKind, processing::VelocityTracker};
//! use flui_foundation::geometry::Offset;
//! use web_time::Instant;
//!
//! let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
//! tracker.add_position(Instant::now(), Offset::ZERO);
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
