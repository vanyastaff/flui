//! RenderAnimatedSize - Animates size changes of its child
//!
//! Implements Flutter's animated size transitions with smooth interpolation between
//! child size changes.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderAnimatedSize` | `RenderAnimatedSize` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `duration` | `duration` property (Duration) |
//! | `alignment` | `alignment` property (AlignmentGeometry) |
//! | `SizeAlignment` | `AlignmentGeometry` class |
//! | `is_animating()` | Animation state check |
//!
//! # Layout Protocol
//!
//! 1. **Layout child**
//!    - Child receives same constraints
//!    - Get child's natural size
//!
//! 2. **Detect size change**
//!    - Compare child size with previous size
//!    - If changed: start size animation
//!    - If first layout: set size immediately (no animation)
//!
//! 3. **Return animated size**
//!    - If animating: return interpolated size between old and new
//!    - If idle: return current stable size
//!    - Constrain to parent constraints
//!
//! # Paint Protocol
//!
//! 1. **Calculate child alignment**
//!    - Based on alignment property (Center, TopLeft, etc.)
//!    - Child positioned within animated container
//!
//! 2. **Paint child at aligned offset**
//!    - Child painted at calculated position
//!    - May be clipped if exceeds animated bounds (TODO)
//!
//! # Performance
//!
//! - **Layout**: O(1) - child layout + size interpolation
//! - **Paint**: O(1) - alignment calculation + child paint
//! - **Memory**: ~56 bytes (Duration + state + sizes)
//!
//! # Use Cases
//!
//! - **Expand/collapse animations**: Smooth height changes for collapsible panels
//! - **Responsive layouts**: Animate size changes on screen resize
//! - **Dynamic content**: Smooth transitions when content size changes
//! - **Cards/tiles**: Animate card expansion on selection
//! - **Text expansion**: "Read more" with smooth height transition
//! - **Image loading**: Smooth size adjustment as images load
//!
//! # Simplified Implementation Note
//!
//! **Current implementation:**
//! - Uses linear interpolation based on elapsed time
//! - Simple Instant-based timing (no Ticker/vsync)
//! - No custom animation curves (linear easing only)
//!
//! **Production enhancements needed:**
//! - AnimationController integration
//! - Customizable curves (ease-in, ease-out, bounce, etc.)
//! - Ticker synchronization with display refresh (vsync)
//! - Reverse animation support
//! - Animation completion callbacks
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderAnimatedSize, SizeAlignment};
//! use std::time::Duration;
//!
//! // Basic animated size (300ms, centered)
//! let animated = RenderAnimatedSize::new(Duration::from_millis(300));
//!
//! // With custom alignment (top-left)
//! let aligned = RenderAnimatedSize::new(Duration::from_millis(500))
//!     .with_alignment(SizeAlignment::TopLeft);
//!
//! // Convenience constructor with milliseconds
//! let quick = RenderAnimatedSize::with_millis(200);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::{Alignment, Size};
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

/// RenderObject that smoothly animates size changes of its child.
///
/// Automatically animates when child size changes, creating smooth transitions
/// instead of abrupt jumps. Uses linear interpolation over specified duration.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Expand/collapse UI**: Smooth panel/accordion animations
/// - **Dynamic content**: Animate size when content loads or changes
/// - **Card expansion**: Smooth card growth on hover/selection
/// - **Text overflow**: "Read more" with smooth height transition
/// - **Responsive design**: Animate layout changes on resize
/// - **Image placeholders**: Smooth size adjustment as images load
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderAnimatedSize behavior:
/// - Automatically detects child size changes
/// - Smoothly interpolates from old to new size
/// - Uses duration property for animation timing
/// - Supports alignment for child positioning during animation
/// - First layout has no animation (immediate size)
/// - Constrains animated size to parent constraints
///
/// # Implementation Note
///
/// **Simplified version:**
/// - Linear interpolation (no custom curves)
/// - Instant-based timing (no Ticker/vsync)
/// - No AnimationController integration
///
/// **Production TODO:**
/// - Add curve support (ease, bounce, etc.)
/// - Integrate with animation framework
/// - Add vsync for smooth refresh-rate-aligned animation
/// - Add clipping when child exceeds animated bounds
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderAnimatedSize, SizeAlignment};
/// use std::time::Duration;
///
/// // Create with 300ms animation, centered
/// let animated = RenderAnimatedSize::new(Duration::from_millis(300));
///
/// // With top-left alignment
/// let aligned = RenderAnimatedSize::with_millis(500)
///     .with_alignment(SizeAlignment::TopLeft);
/// ```
#[derive(Debug)]
pub struct RenderAnimatedSize {
    /// Duration of the size animation
    duration: Duration,

    /// Alignment of child during animation
    alignment: SizeAlignment,

    /// Current animation state
    state: AnimationState,

    /// Last computed size (for detecting changes)
    last_child_size: Option<Size>,

    /// Current animated size
    current_size: Size,
}

impl RenderAnimatedSize {
    /// Create new RenderAnimatedSize with specified duration
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            alignment: SizeAlignment::Center,
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
                    // Linear interpolation (TODO: support curves)
                    Size::new(
                        start_size.width + (target_size.width - start_size.width) * progress,
                        start_size.height + (target_size.height - start_size.height) * progress,
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

impl RenderObject for RenderAnimatedSize {}

impl RenderBox<Single> for RenderAnimatedSize {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Layout child with same constraints
        let child_size = ctx.layout_child(child_id, ctx.constraints)?;

        // Detect size change and start animation if needed
        if self.last_child_size != Some(child_size) {
            self.last_child_size = Some(child_size);

            if self.current_size == Size::ZERO {
                // First layout - don't animate, just set size immediately
                self.current_size = child_size;
                self.state = AnimationState::Idle;
            } else {
                // Size changed - start smooth animation to new size
                self.start_animation(child_size);
            }
        }

        // Update animation and return current interpolated size
        let animated_size = self.update_animation();

        // Constrain animated size to parent constraints
        Ok(ctx.constraints.constrain(animated_size))
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Calculate child offset based on alignment
        let child_offset = if let Some(last_child_size) = self.last_child_size {
            let alignment = self.alignment.to_alignment();

            // Calculate aligned position within the animated container
            let aligned_offset = alignment.calculate_offset(last_child_size, self.current_size);
            ctx.offset + aligned_offset
        } else {
            ctx.offset
        };

        // Paint child at calculated offset
        // TODO: Add clipping if child exceeds current animated size
        ctx.paint_child(child_id, child_offset);
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
}
