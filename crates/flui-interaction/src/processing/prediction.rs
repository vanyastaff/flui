//! Input prediction for low-latency interaction
//!
//! This module provides pointer position prediction to reduce perceived latency.
//! Essential for games and real-time applications where responsiveness is critical.
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
//! use flui_interaction::prediction::InputPredictor;
//!
//! let mut predictor = InputPredictor::new();
//!
//! // Add samples as pointer moves
//! predictor.add_sample(Instant::now(), Offset::new(Pixels(100.0), Pixels(100.0)));
//!
//! // Get predicted position 16ms into the future (one frame at 60fps)
//! let predicted = predictor.predict(Duration::from_millis(16));
//! ```
//!
//! # Accuracy
//!
//! Prediction accuracy decreases with prediction distance. Recommended limits:
//! - High accuracy: 8-16ms (half to one frame)
//! - Medium accuracy: 16-32ms (one to two frames)
//! - Low accuracy: 32-50ms (use with caution)

use super::velocity::{Velocity, VelocityEstimationStrategy, VelocityTracker};
use flui_types::geometry::Pixels;

use flui_types::geometry::Offset;
use std::time::{Duration, Instant};

// ============================================================================
// Constants
// ============================================================================

/// Maximum prediction time (50ms)
const MAX_PREDICTION_TIME: Duration = Duration::from_millis(50);

/// Default prediction time (16ms - one frame at 60fps)
const DEFAULT_PREDICTION_TIME: Duration = Duration::from_millis(16);

/// Minimum samples needed for prediction
const MIN_PREDICTION_SAMPLES: usize = 3;

// ============================================================================
// PredictionConfig
// ============================================================================

/// Configuration for input prediction.
#[derive(Debug, Clone)]
pub struct PredictionConfig {
    /// Maximum prediction time allowed.
    pub max_prediction_time: Duration,
    /// Whether to use acceleration in prediction (quadratic extrapolation).
    pub use_acceleration: bool,
    /// Smoothing factor for predictions (0.0 = no smoothing, 1.0 = max smoothing).
    pub smoothing: f32,
    /// Velocity estimation strategy.
    pub velocity_strategy: VelocityEstimationStrategy,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            max_prediction_time: MAX_PREDICTION_TIME,
            use_acceleration: true,
            smoothing: 0.3,
            velocity_strategy: VelocityEstimationStrategy::LeastSquaresPolynomial,
        }
    }
}

impl PredictionConfig {
    /// Create a config optimized for games (lower latency, less smoothing).
    pub fn for_games() -> Self {
        Self {
            max_prediction_time: Duration::from_millis(32),
            use_acceleration: true,
            smoothing: 0.1,
            velocity_strategy: VelocityEstimationStrategy::LeastSquaresPolynomial,
        }
    }

    /// Create a config optimized for UI (more smoothing, conservative prediction).
    pub fn for_ui() -> Self {
        Self {
            max_prediction_time: Duration::from_millis(16),
            use_acceleration: false,
            smoothing: 0.5,
            velocity_strategy: VelocityEstimationStrategy::LinearRegression,
        }
    }

    /// Create a config with no prediction (pass-through).
    pub fn disabled() -> Self {
        Self {
            max_prediction_time: Duration::ZERO,
            use_acceleration: false,
            smoothing: 0.0,
            velocity_strategy: VelocityEstimationStrategy::TwoSample,
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
    pub position: Offset<Pixels>,
    /// Confidence in prediction (0.0 - 1.0).
    pub confidence: f32,
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
    pub fn position_or(&self, fallback: Offset<Pixels>) -> Offset<Pixels> {
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
    last_position: Option<Offset<Pixels>>,
    /// Last sample time.
    last_time: Option<Instant>,
    /// Previous velocity (for acceleration calculation).
    prev_velocity: Option<Velocity>,
    /// Previous velocity time.
    prev_velocity_time: Option<Instant>,
    /// Smoothed prediction (for reducing jitter).
    smoothed_prediction: Option<Offset<Pixels>>,
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

    /// Create a predictor with custom configuration.
    pub fn with_config(config: PredictionConfig) -> Self {
        Self {
            velocity_tracker: VelocityTracker::with_strategy(config.velocity_strategy),
            config,
            last_position: None,
            last_time: None,
            prev_velocity: None,
            prev_velocity_time: None,
            smoothed_prediction: None,
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
    pub fn add_sample(&mut self, time: Instant, position: Offset<Pixels>) {
        // Store previous velocity for acceleration
        if self.velocity_tracker.has_sufficient_data() {
            self.prev_velocity = Some(self.velocity_tracker.velocity());
            self.prev_velocity_time = self.last_time;
        }

        self.velocity_tracker.add_position(time, position);
        self.last_position = Some(position);
        self.last_time = Some(time);
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

        // If disabled or no data, return last known position
        if time_ahead.is_zero() || self.last_position.is_none() {
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

        let last_pos = self.last_position.unwrap();
        let velocity = self.velocity_tracker.velocity();

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

        let dt = time_ahead.as_secs_f32();

        // Basic linear prediction: pos + velocity * time
        let mut predicted = Offset::new(
            last_pos.dx + Pixels((velocity.pixels_per_second.dx * dt).0),
            last_pos.dy + Pixels((velocity.pixels_per_second.dy * dt).0),
        );

        // Add acceleration term if enabled
        if self.config.use_acceleration {
            if let (Some(prev_vel), Some(prev_time), Some(last_time)) =
                (self.prev_velocity, self.prev_velocity_time, self.last_time)
            {
                let vel_dt = last_time.duration_since(prev_time).as_secs_f32();
                if vel_dt > 0.001 {
                    // Acceleration = (v2 - v1) / dt
                    let accel_x =
                        (velocity.pixels_per_second.dx - prev_vel.pixels_per_second.dx) / vel_dt;
                    let accel_y =
                        (velocity.pixels_per_second.dy - prev_vel.pixels_per_second.dy) / vel_dt;

                    // Add 0.5 * a * t^2 term
                    predicted.dx += Pixels((0.5 * accel_x * dt * dt).0);
                    predicted.dy += Pixels((0.5 * accel_y * dt * dt).0);
                }
            }
        }

        // Apply smoothing to reduce jitter
        if self.config.smoothing > 0.0 {
            if let Some(prev_predicted) = self.smoothed_prediction {
                let alpha = 1.0 - self.config.smoothing;
                predicted = Offset::new(
                    predicted.dx * alpha + prev_predicted.dx * self.config.smoothing,
                    predicted.dy * alpha + prev_predicted.dy * self.config.smoothing,
                );
            }
            self.smoothed_prediction = Some(predicted);
        }

        // Calculate confidence based on velocity consistency and sample count
        let estimate = self.velocity_tracker.estimate();
        let base_confidence = estimate.confidence;

        // Reduce confidence for longer predictions
        let time_factor = 1.0 - (dt / self.config.max_prediction_time.as_secs_f32()).min(1.0);
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
        let frame_time = Duration::from_secs_f32(1.0 / fps as f32);
        self.predict(frame_time)
    }

    /// Predict position for default frame time (16ms / 60fps).
    pub fn predict_default(&mut self) -> PredictedPosition {
        self.predict(DEFAULT_PREDICTION_TIME)
    }

    /// Get the last known position.
    pub fn last_position(&self) -> Option<Offset<Pixels>> {
        self.last_position
    }

    /// Get the current velocity estimate.
    pub fn velocity(&self) -> Velocity {
        self.velocity_tracker.velocity()
    }

    /// Reset the predictor, clearing all history.
    pub fn reset(&mut self) {
        self.velocity_tracker.reset();
        self.last_position = None;
        self.last_time = None;
        self.prev_velocity = None;
        self.prev_velocity_time = None;
        self.smoothed_prediction = None;
    }

    /// Returns true if there's enough data for prediction.
    pub fn can_predict(&self) -> bool {
        self.velocity_tracker.sample_count() >= MIN_PREDICTION_SAMPLES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predictor_empty() {
        let mut predictor = InputPredictor::new();
        let predicted = predictor.predict(Duration::from_millis(16));

        assert_eq!(predicted.position, Offset::ZERO);
        assert_eq!(predicted.confidence, 0.0);
    }

    #[test]
    fn test_predictor_single_sample() {
        let mut predictor = InputPredictor::new();
        predictor.add_sample(Instant::now(), Offset::new(Pixels(100.0), Pixels(100.0)));

        let predicted = predictor.predict(Duration::from_millis(16));

        // Should return last position with low confidence
        assert_eq!(predicted.position.dx, Pixels(100.0));
        assert_eq!(predicted.position.dy, Pixels(100.0));
    }

    #[test]
    fn test_predictor_horizontal_motion() {
        let mut predictor = InputPredictor::new();
        let start = Instant::now();

        // Simulate horizontal motion: 100 pixels in 100ms = 1000 px/s
        for i in 0..10 {
            let t = start + Duration::from_millis(i * 10);
            predictor.add_sample(t, Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)));
        }

        // Predict 16ms into future
        let predicted = predictor.predict(Duration::from_millis(16));

        // At 1000 px/s, after 16ms we should move ~16 pixels
        // Last position was 90px, predicted should be around 106px
        assert!(predicted.position.dx > Pixels(100.0));
        assert!(predicted.position.dx < Pixels(120.0));
        assert!(predicted.confidence > 0.3);
    }

    #[test]
    fn test_predictor_configs() {
        let game_predictor = InputPredictor::for_games();
        assert!(game_predictor.config.use_acceleration);
        assert!(game_predictor.config.smoothing < 0.2);

        let ui_predictor = InputPredictor::for_ui();
        assert!(!ui_predictor.config.use_acceleration);
        assert!(ui_predictor.config.smoothing > 0.3);
    }

    #[test]
    fn test_predictor_reset() {
        let mut predictor = InputPredictor::new();
        predictor.add_sample(Instant::now(), Offset::new(Pixels(100.0), Pixels(100.0)));

        assert!(predictor.last_position().is_some());

        predictor.reset();

        assert!(predictor.last_position().is_none());
        assert!(!predictor.can_predict());
    }

    #[test]
    fn test_predicted_position_helpers() {
        let confident = PredictedPosition {
            position: Offset::new(Pixels(100.0), Pixels(100.0)),
            confidence: 0.8,
            prediction_time: Duration::from_millis(16),
            velocity: Velocity::ZERO,
        };

        assert!(confident.is_confident());
        assert_eq!(
            confident.position_or(Offset::ZERO),
            Offset::new(Pixels(100.0), Pixels(100.0))
        );

        let not_confident = PredictedPosition {
            position: Offset::new(Pixels(100.0), Pixels(100.0)),
            confidence: 0.2,
            prediction_time: Duration::from_millis(16),
            velocity: Velocity::ZERO,
        };

        assert!(!not_confident.is_confident());
        assert_eq!(
            not_confident.position_or(Offset::new(Pixels(50.0), Pixels(50.0))),
            Offset::new(Pixels(50.0), Pixels(50.0))
        );
    }

    #[test]
    fn test_predictor_max_prediction_clamped() {
        let mut predictor = InputPredictor::new();
        let start = Instant::now();

        for i in 0..10 {
            let t = start + Duration::from_millis(i * 10);
            predictor.add_sample(t, Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)));
        }

        // Request very long prediction - should be clamped
        let predicted = predictor.predict(Duration::from_secs(1));

        // Should be clamped to max (50ms), not 1 second
        assert!(predicted.prediction_time <= MAX_PREDICTION_TIME);
    }

    #[test]
    fn test_predict_next_frame() {
        let mut predictor = InputPredictor::new();
        let start = Instant::now();

        for i in 0..10 {
            let t = start + Duration::from_millis(i * 10);
            predictor.add_sample(t, Offset::new(Pixels(i as f32 * 10.0), Pixels(0.0)));
        }

        let at_60fps = predictor.predict_next_frame(60);
        let at_30fps = predictor.predict_next_frame(30);

        // 30fps should predict further than 60fps
        assert!(at_30fps.prediction_time > at_60fps.prediction_time);
    }
}
