//! Animation controller for smooth UI transitions

use instant::Instant;
use std::time::Duration;
use super::Controller;

/// Controls animations with various curves and states
#[derive(Debug, Clone)]
pub struct AnimationController {
    /// Current animation value (0.0 to 1.0)
    value: f32,
    /// Target value for animation
    target: f32,
    /// Animation duration
    duration: Duration,
    /// Animation curve
    curve: AnimationCurve,
    /// Current animation state
    state: AnimationState,
    /// Start time of current animation
    start_time: Option<Instant>,
    /// Starting value for interpolation
    start_value: f32,
}

/// Animation state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationState {
    /// Animation is idle
    Idle,
    /// Animation is running forward (0 -> 1)
    Forward,
    /// Animation is running in reverse (1 -> 0)
    Reverse,
    /// Animation has completed
    Completed,
}

/// Animation curve types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationCurve {
    /// Linear interpolation
    Linear,
    /// Slow start, fast end
    EaseIn,
    /// Fast start, slow end
    EaseOut,
    /// Slow start and end
    EaseInOut,
    /// Bounce effect at end
    Bounce,
    /// Elastic overshoot
    Elastic,
    /// Custom curve function
    Custom(fn(f32) -> f32),
}

impl Default for AnimationController {
    fn default() -> Self {
        Self::new(Duration::from_millis(200))
    }
}

impl AnimationController {
    /// Create a new animation controller with the given duration
    pub fn new(duration: Duration) -> Self {
        Self {
            value: 0.0,
            target: 0.0,
            duration,
            curve: AnimationCurve::EaseInOut,
            state: AnimationState::Idle,
            start_time: None,
            start_value: 0.0,
        }
    }

    /// Set the animation curve
    pub fn with_curve(mut self, curve: AnimationCurve) -> Self {
        self.curve = curve;
        self
    }

    /// Start forward animation (0 -> 1)
    pub fn forward(&mut self) {
        if self.state != AnimationState::Forward || self.target != 1.0 {
            self.start_value = self.value;
            self.target = 1.0;
            self.state = AnimationState::Forward;
            self.start_time = Some(Instant::now());
        }
    }

    /// Start reverse animation (1 -> 0)
    pub fn reverse(&mut self) {
        if self.state != AnimationState::Reverse || self.target != 0.0 {
            self.start_value = self.value;
            self.target = 0.0;
            self.state = AnimationState::Reverse;
            self.start_time = Some(Instant::now());
        }
    }

    /// Toggle animation direction
    pub fn toggle(&mut self) {
        if self.value < 0.5 {
            self.forward();
        } else {
            self.reverse();
        }
    }

    /// Jump to a specific value immediately
    pub fn set_value(&mut self, value: f32) {
        self.value = value.clamp(0.0, 1.0);
        self.target = self.value;
        self.state = AnimationState::Idle;
        self.start_time = None;
    }

    /// Animate to a specific target value
    pub fn animate_to(&mut self, target: f32) {
        let target = target.clamp(0.0, 1.0);
        if (self.target - target).abs() > 0.001 {
            self.start_value = self.value;
            self.target = target;
            self.state = if target > self.value {
                AnimationState::Forward
            } else {
                AnimationState::Reverse
            };
            self.start_time = Some(Instant::now());
        }
    }

    /// Update animation and return current value
    pub fn tick(&mut self) -> f32 {
        if let Some(start_time) = self.start_time {
            let elapsed = start_time.elapsed().as_secs_f32();
            let progress = (elapsed / self.duration.as_secs_f32()).min(1.0);

            // Apply curve
            let curved_progress = self.apply_curve(progress);

            // Interpolate between start and target
            self.value = self.start_value + (self.target - self.start_value) * curved_progress;

            // Check if completed
            if progress >= 1.0 {
                self.value = self.target;
                self.state = AnimationState::Completed;
                self.start_time = None;
            }
        }

        self.value
    }

    /// Get current value without updating
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Check if animation is running
    pub fn is_animating(&self) -> bool {
        matches!(self.state, AnimationState::Forward | AnimationState::Reverse)
    }

    /// Get current state
    pub fn state(&self) -> AnimationState {
        self.state
    }

    /// Reset animation to initial state
    pub fn reset(&mut self) {
        self.value = 0.0;
        self.target = 0.0;
        self.state = AnimationState::Idle;
        self.start_time = None;
    }

    /// Apply animation curve to progress value
    fn apply_curve(&self, t: f32) -> f32 {
        match self.curve {
            AnimationCurve::Linear => t,
            AnimationCurve::EaseIn => t * t,
            AnimationCurve::EaseOut => t * (2.0 - t),
            AnimationCurve::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
            AnimationCurve::Bounce => {
                if t < 0.5 {
                    8.0 * t * t * t * t
                } else {
                    1.0 - 8.0 * (t - 1.0) * (t - 1.0) * (t - 1.0) * (t - 1.0)
                }
            }
            AnimationCurve::Elastic => {
                let c = (2.0 * std::f32::consts::PI) / 3.0;
                if t == 0.0 {
                    0.0
                } else if t == 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c).sin()
                }
            }
            AnimationCurve::Custom(f) => f(t),
        }
    }
}

// Implement Controller trait for AnimationController
impl Controller for AnimationController {
    fn update(&mut self, ctx: &egui::Context) {
        // Tick the animation
        self.tick();

        // Request repaint if animation is active
        if self.is_active() {
            ctx.request_repaint();
        }
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.target = 0.0;
        self.state = AnimationState::Idle;
        self.start_time = None;
    }

    fn debug_name(&self) -> &'static str {
        "AnimationController"
    }

    fn is_active(&self) -> bool {
        self.is_animating()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animation_forward() {
        let mut anim = AnimationController::new(Duration::from_millis(100));
        assert_eq!(anim.value(), 0.0);

        anim.forward();
        assert_eq!(anim.state(), AnimationState::Forward);

        // Simulate some time passing
        std::thread::sleep(Duration::from_millis(50));
        let val = anim.tick();
        assert!(val > 0.0 && val < 1.0);

        // Complete animation
        std::thread::sleep(Duration::from_millis(60));
        let val = anim.tick();
        assert_eq!(val, 1.0);
        assert_eq!(anim.state(), AnimationState::Completed);
    }

    #[test]
    fn test_animation_curves() {
        let curves = [
            AnimationCurve::Linear,
            AnimationCurve::EaseIn,
            AnimationCurve::EaseOut,
            AnimationCurve::EaseInOut,
            AnimationCurve::Bounce,
            AnimationCurve::Elastic,
        ];

        for curve in curves {
            let anim = AnimationController::new(Duration::from_millis(100)).with_curve(curve);
            assert_eq!(anim.apply_curve(0.0), 0.0);
            // Most curves should end at 1.0
            if !matches!(curve, AnimationCurve::Elastic) {
                assert!((anim.apply_curve(1.0) - 1.0).abs() < 0.01);
            }
        }
    }
}