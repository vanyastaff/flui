//! Input prediction for low-latency interaction
//!
//! This module provides pointer position prediction to reduce perceived
//! latency. Essential for games and real-time applications where responsiveness
//! is critical.
//!
//! # How it works
//!
//! Input prediction uses recent position history and velocity estimation to
//! extrapolate where the pointer will be in the near future. This allows
//! rendering to "lead" the actual input, reducing perceived lag.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::processing::InputPredictor;
//!
//! let mut predictor = InputPredictor::new();
//!
//! // Add samples as pointer moves
//! predictor.add_sample(Instant::now(), Offset::new(100.0, 100.0));
//!
//! // Get predicted position 16ms into the future (one frame at 60fps)
//! let predicted = predictor.predict(Duration::from_millis(16));
//! ```
//!
//! # Accuracy
//!
//! Prediction accuracy decreases with prediction distance. Recommended limits:
//! - High accuracy: 8-16ms (half to one frame)
//! - Medium accuracy: 16-25ms (one to one and a half frames)
//! - Beyond 25 ms: not offered (the look-ahead is capped there)

use web_time::{Duration, Instant};

use flui_foundation::geometry::Offset;

use super::velocity::{Velocity, VelocityTracker};

// ============================================================================
// Constants
// ============================================================================

/// Maximum prediction time.
///
/// 25 ms is Chromium's empirically validated ceiling
/// (`ui/base/prediction/input_predictor.h` `kMaxPredictionTime`): beyond it,
/// prediction visibly overshoots on direction changes and jitters on noisy
/// trajectories. Was 50 ms here before being aligned with that evidence.
const MAX_PREDICTION_TIME: Duration = Duration::from_millis(25);

/// Default prediction time (16ms - one frame at 60fps)
const DEFAULT_PREDICTION_TIME: Duration = Duration::from_millis(16);

/// Minimum samples needed for prediction
const MIN_PREDICTION_SAMPLES: usize = 3;

// ============================================================================
// PredictionConfig
// ============================================================================

/// Configuration for input prediction.
///
/// The fields are plain data; [`InputPredictor::with_config`] sanitizes
/// them: `max_prediction_time` is capped at 25 ms, and a `smoothing` outside
/// `[0, MAX_SMOOTHING]` (or NaN) is clamped into it (NaN means no smoothing).
#[derive(Debug, Clone)]
pub struct PredictionConfig {
    /// Maximum prediction time allowed. Capped at 25 ms.
    pub max_prediction_time: Duration,
    /// Whether to use acceleration in prediction (quadratic extrapolation).
    pub use_acceleration: bool,
    /// Smoothing factor for the prediction rate (0.0 = no smoothing; values
    /// near 1.0 react slowly). Clamped to `[0, 0.9]`.
    pub smoothing: f64,
}

/// Largest admitted smoothing factor: 1.0 would freeze the prediction and
/// anything above diverges.
const MAX_SMOOTHING: f64 = 0.9;

impl PredictionConfig {
    /// This config with every field inside its admitted range.
    fn sanitized(mut self) -> Self {
        self.max_prediction_time = self.max_prediction_time.min(MAX_PREDICTION_TIME);
        self.smoothing = if self.smoothing.is_nan() {
            0.0
        } else {
            self.smoothing.clamp(0.0, MAX_SMOOTHING)
        };
        self
    }
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            max_prediction_time: MAX_PREDICTION_TIME,
            use_acceleration: true,
            smoothing: 0.3,
        }
    }
}

impl PredictionConfig {
    /// Create a config optimized for games (lower latency, less smoothing).
    pub fn for_games() -> Self {
        Self {
            // Capped at the Chromium-validated 25 ms ceiling (was 32 ms):
            // longer horizons overshoot on direction changes.
            max_prediction_time: MAX_PREDICTION_TIME,
            use_acceleration: true,
            smoothing: 0.1,
        }
    }

    /// Create a config optimized for UI (more smoothing, conservative
    /// prediction).
    pub fn for_ui() -> Self {
        Self {
            max_prediction_time: Duration::from_millis(16),
            use_acceleration: false,
            smoothing: 0.5,
        }
    }

    /// Create a config with no prediction (pass-through).
    pub fn disabled() -> Self {
        Self {
            max_prediction_time: Duration::ZERO,
            use_acceleration: false,
            smoothing: 0.0,
        }
    }
}

// ============================================================================
// PredictedPosition
// ============================================================================

/// A predicted position with confidence information.
#[derive(Debug, Clone, Copy)]
pub struct PredictedPosition {
    /// The predicted position.
    pub position: Offset<f64>,
    /// Confidence in prediction (0.0 - 1.0).
    pub confidence: f64,
    /// How far into the future this prediction is.
    pub prediction_time: Duration,
    /// The velocity used for prediction.
    pub velocity: Velocity,
}

impl PredictedPosition {
    /// Returns true if confidence is above threshold (default 0.5).
    pub fn is_confident(&self) -> bool {
        self.confidence > 0.5
    }

    /// Returns the position if confident, otherwise returns fallback.
    pub fn position_or(&self, fallback: Offset<f64>) -> Offset<f64> {
        if self.is_confident() {
            self.position
        } else {
            fallback
        }
    }
}

// ============================================================================
// InputPredictor
// ============================================================================

/// Predicts future pointer positions for low-latency interaction.
///
/// Uses velocity estimation and optional acceleration to extrapolate
/// where the pointer will be in the near future.
///
/// # Example
///
/// ```rust,ignore
/// let mut predictor = InputPredictor::new();
///
/// // In your input handler:
/// predictor.add_sample(Instant::now(), pointer_position);
///
/// // In your render loop:
/// let frame_time = Duration::from_millis(16);
/// let predicted = predictor.predict(frame_time);
///
/// // Use predicted position for rendering
/// render_cursor(predicted.position_or(last_known_position));
/// ```
#[derive(Debug, Clone)]
pub struct InputPredictor {
    /// Velocity tracker for position history.
    velocity_tracker: VelocityTracker,
    /// Configuration.
    config: PredictionConfig,
    /// Last known position.
    last_position: Option<Offset<f64>>,
    /// Smoothed prediction rate, px/s: the predicted displacement divided by
    /// the time it was predicted over. Smoothing the rate rather than the
    /// absolute position keeps the prediction anchored to the newest sample
    /// and comparable across different look-ahead times.
    smoothed_rate: Option<Offset<f64>>,
}

impl Default for InputPredictor {
    fn default() -> Self {
        Self::new()
    }
}

impl InputPredictor {
    /// Create a new input predictor with default configuration.
    pub fn new() -> Self {
        Self::with_config(PredictionConfig::default())
    }

    /// Create a predictor with custom configuration, sanitized as described
    /// on [`PredictionConfig`].
    pub fn with_config(config: PredictionConfig) -> Self {
        Self {
            velocity_tracker: VelocityTracker::new(),
            config: config.sanitized(),
            last_position: None,
            smoothed_rate: None,
        }
    }

    /// Create a predictor optimized for games.
    pub fn for_games() -> Self {
        Self::with_config(PredictionConfig::for_games())
    }

    /// Create a predictor optimized for UI.
    pub fn for_ui() -> Self {
        Self::with_config(PredictionConfig::for_ui())
    }

    /// Add a position sample.
    ///
    /// A non-finite position is ignored.
    pub fn add_sample(&mut self, time: Instant, position: Offset<f64>) {
        if !position.dx.is_finite() || !position.dy.is_finite() {
            return;
        }
        self.velocity_tracker.add_position(time, position);
        self.last_position = Some(position);
    }

    /// Predict position at a future time.
    ///
    /// # Arguments
    ///
    /// * `time_ahead` - How far into the future to predict
    ///
    /// # Returns
    ///
    /// A `PredictedPosition` with the predicted location and confidence.
    pub fn predict(&mut self, time_ahead: Duration) -> PredictedPosition {
        // Clamp prediction time
        let time_ahead = time_ahead.min(self.config.max_prediction_time);

        // If disabled or no data, return last known position (if any)
        if time_ahead.is_zero() {
            return PredictedPosition {
                position: self.last_position.unwrap_or(Offset::ZERO),
                confidence: if self.last_position.is_some() {
                    1.0
                } else {
                    0.0
                },
                prediction_time: Duration::ZERO,
                velocity: Velocity::ZERO,
            };
        }

        // From here on, `last_pos` is the only way to access `self.last_position`;
        // the `if let` guard makes the `None` case unrepresentable and removes
        // the need for an `.unwrap()`.
        let Some(last_pos) = self.last_position else {
            return PredictedPosition {
                position: Offset::ZERO,
                confidence: 0.0,
                prediction_time: Duration::ZERO,
                velocity: Velocity::ZERO,
            };
        };
        let velocity = self.velocity_tracker.get_velocity();

        // Not enough data for prediction
        if !self.velocity_tracker.has_sufficient_data()
            || self.velocity_tracker.sample_count() < MIN_PREDICTION_SAMPLES
        {
            return PredictedPosition {
                position: last_pos,
                confidence: 0.3,
                prediction_time: Duration::ZERO,
                velocity,
            };
        }

        let dt = time_ahead.as_secs_f64();

        // Linear prediction: velocity × time. The tracker bounds the velocity,
        // so this term is at most the fling bound × 25 ms.
        let linear = velocity.pixels_per_second * dt;

        // Quadratic term from the least-squares fit's own curvature, which
        // is fitted over the whole sample window rather than differenced over
        // one interval (that amplifies jitter). Capped at the linear term's
        // length: the curve may bend the prediction, never overtake it.
        let mut quadratic = if self.config.use_acceleration {
            self.velocity_tracker.acceleration() * (0.5 * dt * dt)
        } else {
            Offset::ZERO
        };
        let (linear_len, quadratic_len) = (linear.distance(), quadratic.distance());
        if quadratic_len > linear_len {
            quadratic *= linear_len / quadratic_len;
        }

        // Smooth the prediction *rate*: an exponential average over
        // successive predictions, each normalized by its own look-ahead. The
        // weight is at most `MAX_SMOOTHING`, so the average is a convex
        // combination and stays within the bound of its inputs.
        let mut rate = (linear + quadratic) / dt;
        if self.config.smoothing > 0.0 {
            if let Some(previous) = self.smoothed_rate {
                let keep = self.config.smoothing;
                rate = rate * (1.0 - keep) + previous * keep;
            }
            self.smoothed_rate = Some(rate);
        }
        let predicted = last_pos + rate * dt;

        // Calculate confidence based on velocity consistency and sample count
        // (estimate() returns Option — a missing estimate is a confidence 0 signal).
        let base_confidence = self
            .velocity_tracker
            .estimate()
            .map_or(0.0, |e| e.confidence);

        // Reduce confidence for longer predictions
        let time_factor = 1.0 - (dt / self.config.max_prediction_time.as_secs_f64()).min(1.0);
        let confidence = base_confidence * time_factor;

        PredictedPosition {
            position: predicted,
            confidence,
            prediction_time: time_ahead,
            velocity,
        }
    }

    /// Predict position for next frame at given frame rate.
    pub fn predict_next_frame(&mut self, fps: u32) -> PredictedPosition {
        // Clamp to >= 1 fps so `fps == 0` cannot produce an infinite/NaN frame
        // time (untrusted callers / config may pass 0).
        let frame_time = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
        self.predict(frame_time)
    }

    /// Predict position for default frame time (16ms / 60fps).
    pub fn predict_default(&mut self) -> PredictedPosition {
        self.predict(DEFAULT_PREDICTION_TIME)
    }

    /// Get the last known position.
    pub fn last_position(&self) -> Option<Offset<f64>> {
        self.last_position
    }

    /// Get the current velocity estimate.
    ///
    /// `&mut self` because the underlying [`VelocityTracker`] memoizes its
    /// estimate on query (the cache is invalidated by the next sample).
    pub fn velocity(&mut self) -> Velocity {
        self.velocity_tracker.get_velocity()
    }

    /// Reset the predictor, clearing all history.
    pub fn reset(&mut self) {
        self.velocity_tracker.reset();
        self.last_position = None;
        self.smoothed_rate = None;
    }

    /// Returns true if there's enough data for prediction.
    pub fn can_predict(&self) -> bool {
        self.velocity_tracker.sample_count() >= MIN_PREDICTION_SAMPLES
    }
}
