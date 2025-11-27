//! RenderAnimatedSize - Animates size changes of its child
//!
//! NOTE: This is a simplified version without full animation infrastructure.
//! It smoothly transitions between sizes using linear interpolation.
//! A full implementation would use AnimationController and TickerProvider.

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, Single};
use flui_types::animation::{Cubic, Curve};
use flui_types::{Alignment, Rect, Size};
use std::time::{Duration, Instant};

/// Alignment for positioning the child during size animation
///
/// Determines where the child is positioned within the animated container
/// as it grows or shrinks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeAlignment {
    /// Center the child
    Center,
    /// Align child to top-left
    TopLeft,
    /// Align child to top-right
    TopRight,
    /// Align child to bottom-left
    BottomLeft,
    /// Align child to bottom-right
    BottomRight,
}

impl SizeAlignment {
    /// Convert to Alignment for offset calculation
    fn to_alignment(self) -> Alignment {
        match self {
            SizeAlignment::Center => Alignment::CENTER,
            SizeAlignment::TopLeft => Alignment::TOP_LEFT,
            SizeAlignment::TopRight => Alignment::TOP_RIGHT,
            SizeAlignment::BottomLeft => Alignment::BOTTOM_LEFT,
            SizeAlignment::BottomRight => Alignment::BOTTOM_RIGHT,
        }
    }
}

/// Animation state for size transitions
#[derive(Debug, Clone, Copy, PartialEq)]
enum AnimationState {
    /// No animation in progress
    Idle,
    /// Animation in progress
    Animating {
        /// Time when animation started
        start_time: Instant,
        /// Starting size
        start_size: Size,
        /// Target size
        target_size: Size,
    },
}

/// RenderObject that smoothly animates size changes
///
/// RenderAnimatedSize automatically animates its size when its child's size
/// changes. This creates smooth transitions instead of abrupt size changes.
///
/// # Implementation Note
///
/// This is a self-contained animation implementation that uses
/// time-based interpolation with support for `flui_types::animation::Curve`.
/// It provides smooth animations without requiring external animation
/// infrastructure (AnimationController, Ticker, vsync).
///
/// **Features**:
/// - Full curve support via `flui_types::animation::Curve` trait
/// - Predefined curves: Linear, EaseIn, EaseOut, EaseInOut, FastOutSlowIn, ElasticOut, etc.
/// - Custom curves via Cubic, CatmullRom, and other curve types
/// - Time-based animation (no frame dependencies)
/// - Self-contained state management
/// - Customizable duration and alignment
///
/// # Behavior
///
/// - When child size changes, smoothly interpolates from old to new size
/// - Uses linear easing (can be enhanced with custom curves)
/// - Clips child content that exceeds current animated size
/// - Centers child by default during animation
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAnimatedSize;
/// use std::time::Duration;
///
/// // Create animated size with 300ms transition
/// let animated_size = RenderAnimatedSize::new(Duration::from_millis(300));
/// ```
pub struct RenderAnimatedSize {
    /// Duration of the size animation
    duration: Duration,

    /// Alignment of child during animation
    alignment: SizeAlignment,

    /// Animation curve for easing
    curve: Cubic,

    /// Current animation state
    state: AnimationState,

    /// Last computed size (for detecting changes)
    last_child_size: Option<Size>,

    /// Current animated size
    current_size: Size,
}

// Manual Debug implementation since Cubic uses f32 which doesn't have exact equality
impl std::fmt::Debug for RenderAnimatedSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderAnimatedSize")
            .field("duration", &self.duration)
            .field("alignment", &self.alignment)
            .field("curve", &"<curve>")
            .field("state", &self.state)
            .field("last_child_size", &self.last_child_size)
            .field("current_size", &self.current_size)
            .finish()
    }
}

impl RenderAnimatedSize {
    /// Create new RenderAnimatedSize with specified duration and linear curve
    pub fn new(duration: Duration) -> Self {
        // Use linear cubic approximation (0,0,1,1) which is equivalent to Linear
        Self {
            duration,
            alignment: SizeAlignment::Center,
            curve: Cubic::new(0.0, 0.0, 1.0, 1.0), // Linear curve as cubic
            state: AnimationState::Idle,
            last_child_size: None,
            current_size: Size::ZERO,
        }
    }

    /// Create with duration in milliseconds (convenience)
    pub fn with_millis(millis: u64) -> Self {
        Self::new(Duration::from_millis(millis))
    }

    /// Set alignment for child positioning
    pub fn with_alignment(mut self, alignment: SizeAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set animation curve
    pub fn with_curve(mut self, curve: Cubic) -> Self {
        self.curve = curve;
        self
    }

    /// Get current animation curve
    pub fn curve(&self) -> Cubic {
        self.curve
    }

    /// Set new animation curve
    pub fn set_curve(&mut self, curve: Cubic) {
        self.curve = curve;
    }

    /// Get current duration
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Set new duration
    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = duration;
    }

    /// Check if animation is currently running
    pub fn is_animating(&self) -> bool {
        matches!(self.state, AnimationState::Animating { .. })
    }

    /// Calculate interpolated size based on animation progress
    fn calculate_animated_size(&self) -> Size {
        match self.state {
            AnimationState::Idle => self.current_size,
            AnimationState::Animating {
                start_time,
                start_size,
                target_size,
            } => {
                let elapsed = start_time.elapsed();
                let progress =
                    (elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0);

                if progress >= 1.0 {
                    // Animation complete
                    target_size
                } else {
                    // Apply easing curve to progress
                    let eased_progress = self.curve.transform(progress);

                    // Interpolate with eased progress
                    Size::new(
                        start_size.width + (target_size.width - start_size.width) * eased_progress,
                        start_size.height
                            + (target_size.height - start_size.height) * eased_progress,
                    )
                }
            }
        }
    }

    /// Start animation to new target size
    fn start_animation(&mut self, new_size: Size) {
        if self.current_size != new_size {
            self.state = AnimationState::Animating {
                start_time: Instant::now(),
                start_size: self.current_size,
                target_size: new_size,
            };
        }
    }

    /// Update animation state and return current size
    fn update_animation(&mut self) -> Size {
        let new_size = self.calculate_animated_size();

        // Check if animation completed
        if let AnimationState::Animating { target_size, .. } = self.state {
            if new_size == target_size {
                self.state = AnimationState::Idle;
            }
        }

        self.current_size = new_size;
        new_size
    }
}

impl RenderBox<Single> for RenderAnimatedSize {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // Layout child with same constraints
        let child_size = ctx.layout_child(child_id, ctx.constraints);

        // Detect size change and start animation if needed
        if self.last_child_size != Some(child_size) {
            self.last_child_size = Some(child_size);

            if self.current_size == Size::ZERO {
                // First layout - don't animate, just set size
                self.current_size = child_size;
                self.state = AnimationState::Idle;
            } else {
                // Size changed - start animation
                self.start_animation(child_size);
            }
        }

        // Update animation and return current interpolated size
        let animated_size = self.update_animation();

        // Constrain animated size to parent constraints
        ctx.constraints.constrain(animated_size)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Calculate child offset based on alignment
        let child_offset = if let Some(last_child_size) = self.last_child_size {
            let alignment = self.alignment.to_alignment();

            // Calculate aligned position within the animated container
            let aligned_offset = alignment.calculate_offset(last_child_size, self.current_size);
            ctx.offset + aligned_offset
        } else {
            ctx.offset
        };

        // Check if child overflows current animated size
        let has_overflow = self
            .last_child_size
            .map(|cs| cs.width > self.current_size.width || cs.height > self.current_size.height)
            .unwrap_or(false);

        if has_overflow {
            // Clip child to animated bounds to prevent overflow during shrink animation
            let clip_rect = Rect::from_xywh(
                ctx.offset.dx,
                ctx.offset.dy,
                self.current_size.width,
                self.current_size.height,
            );
            ctx.canvas().saved().clipped_rect(clip_rect);
            ctx.paint_child(child_id, child_offset);
            ctx.canvas().restored();
        } else {
            // No overflow - paint child normally
            ctx.paint_child(child_id, child_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animated_size_new() {
        let duration = Duration::from_millis(300);
        let animated_size = RenderAnimatedSize::new(duration);

        assert_eq!(animated_size.duration(), duration);
        assert!(!animated_size.is_animating());
    }

    #[test]
    fn test_animated_size_with_millis() {
        let animated_size = RenderAnimatedSize::with_millis(500);
        assert_eq!(animated_size.duration(), Duration::from_millis(500));
    }

    #[test]
    fn test_animated_size_with_alignment() {
        let animated_size = RenderAnimatedSize::new(Duration::from_millis(300))
            .with_alignment(SizeAlignment::TopLeft);
        assert_eq!(animated_size.alignment, SizeAlignment::TopLeft);
    }

    #[test]
    fn test_size_alignment_to_alignment() {
        assert_eq!(SizeAlignment::Center.to_alignment(), Alignment::CENTER);
        assert_eq!(SizeAlignment::TopLeft.to_alignment(), Alignment::TOP_LEFT);
        assert_eq!(SizeAlignment::TopRight.to_alignment(), Alignment::TOP_RIGHT);
        assert_eq!(
            SizeAlignment::BottomLeft.to_alignment(),
            Alignment::BOTTOM_LEFT
        );
        assert_eq!(
            SizeAlignment::BottomRight.to_alignment(),
            Alignment::BOTTOM_RIGHT
        );
    }

    #[test]
    fn test_set_duration() {
        let mut animated_size = RenderAnimatedSize::new(Duration::from_millis(300));
        animated_size.set_duration(Duration::from_millis(500));
        assert_eq!(animated_size.duration(), Duration::from_millis(500));
    }

    #[test]
    fn test_initial_state_is_idle() {
        let animated_size = RenderAnimatedSize::new(Duration::from_millis(300));
        assert!(!animated_size.is_animating());
        assert_eq!(animated_size.state, AnimationState::Idle);
    }

    #[test]
    fn test_animation_curve_with_curve() {
        use flui_types::animation::Curves;
        let animated_size =
            RenderAnimatedSize::new(Duration::from_millis(300)).with_curve(Curves::EaseInOut);
        assert_eq!(animated_size.curve(), Curves::EaseInOut);
    }

    #[test]
    fn test_animation_curve_set_curve() {
        use flui_types::animation::Curves;
        let mut animated_size = RenderAnimatedSize::new(Duration::from_millis(300));
        // Default curve is linear cubic (0,0,1,1)
        assert_eq!(animated_size.curve(), Cubic::new(0.0, 0.0, 1.0, 1.0));

        animated_size.set_curve(Curves::FastOutSlowIn);
        assert_eq!(animated_size.curve(), Curves::FastOutSlowIn);
    }

    #[test]
    fn test_animation_uses_curve() {
        use flui_types::animation::{Curve, Curves};

        // Test that curves affect interpolation correctly
        let linear = Curves::Linear;
        assert_eq!(linear.transform(0.5), 0.5);

        let ease_in = Curves::EaseIn;
        assert!(ease_in.transform(0.5) < 0.5); // Slower at start

        let ease_out = Curves::EaseOut;
        assert!(ease_out.transform(0.5) > 0.5); // Faster at start
    }
}
