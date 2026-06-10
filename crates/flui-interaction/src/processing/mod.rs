//! Input processing utilities
//!
//! This module provides utilities for processing and enhancing input events:
//!
//! - [`VelocityTracker`] - Velocity estimation from pointer movement
//! - [`InputPredictor`] - Predict future pointer positions for reduced latency
//! - [`PointerEventResampler`] - Resample events to consistent frame rate
//! - [`RawInputHandler`] - Low-level input handling without gesture recognition
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::processing::{VelocityTracker, InputPredictor};
//!
//! let mut tracker = VelocityTracker::new();
//! tracker.add_position(now, position);
//! let velocity = tracker.get_velocity();
//!
//! let mut predictor = InputPredictor::new();
//! predictor.add_sample(now, position);
//! let predicted = predictor.predict(Duration::from_millis(16));
//! ```

mod lsq_solver;
mod prediction;
mod raw_input;
mod resampler;
mod sampling_clock;
mod velocity;

// `lsq_solver` (LeastSquaresSolver / PolynomialFit / MAX_*) is crate-internal
// numerical machinery shared by the velocity tracker; it is intentionally NOT
// re-exported, so the public API is not pinned to the solver's internals.
pub use prediction::{InputPredictor, PredictedPosition, PredictionConfig};
pub use raw_input::{InputMode, RawInputHandler, RawPointerEvent};
pub use resampler::PointerEventResampler;
pub use sampling_clock::{DEFAULT_SAMPLE_PERIOD, SamplingClock};
pub use velocity::{
    IosFlingVelocityTracker, MacosFlingVelocityTracker, Velocity, VelocityEstimate, VelocityTracker,
};
