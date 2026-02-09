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
//! let velocity = tracker.velocity();
//!
//! let mut predictor = InputPredictor::new();
//! predictor.add_sample(now, position);
//! let predicted = predictor.predict(Duration::from_millis(16));
//! ```

mod prediction;
mod raw_input;
mod resampler;
mod velocity;

pub use prediction::{InputPredictor, PredictedPosition, PredictionConfig};
pub use raw_input::{InputMode, RawInputHandler, RawPointerEvent};
pub use resampler::PointerEventResampler;
pub use velocity::{Velocity, VelocityEstimate, VelocityEstimationStrategy, VelocityTracker};
