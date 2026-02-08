//! Advanced trait patterns for the scheduler
//!
//! This module provides:
//! - Sealed traits to prevent external implementations
//! - Extension traits for adding methods to existing types
//! - GAT-based provider traits
//!
//! ## Sealed Traits
//!
//! Sealed traits prevent users from implementing them outside this crate.
//! This allows adding methods to traits without breaking changes.
//!
//! ## Extension Traits
//!
//! Extension traits add methods to types without modifying them.
//! Similar to Kotlin extension functions or C# extension methods.

use crate::budget::FrameBudget;
use crate::duration::{FrameDuration, Milliseconds, Percentage, Seconds};
use crate::frame::FrameTiming;
use crate::task::Priority;

// =============================================================================
// Sealed Trait Pattern for Priority
// =============================================================================

mod sealed {
    /// Sealed trait marker - cannot be implemented outside this crate
    pub trait PriorityLevel {}

    impl PriorityLevel for super::UserInputPriority {}
    impl PriorityLevel for super::AnimationPriority {}
    impl PriorityLevel for super::BuildPriority {}
    impl PriorityLevel for super::IdlePriority {}
}

/// Type-level priority for UserInput tasks
#[derive(Debug, Clone, Copy, Default)]
pub struct UserInputPriority;

/// Type-level priority for Animation tasks
#[derive(Debug, Clone, Copy, Default)]
pub struct AnimationPriority;

/// Type-level priority for Build tasks
#[derive(Debug, Clone, Copy, Default)]
pub struct BuildPriority;

/// Type-level priority for Idle tasks
#[derive(Debug, Clone, Copy, Default)]
pub struct IdlePriority;

/// Trait for priority level types - sealed to prevent external implementations
///
/// This trait allows compile-time priority checking in generic code.
///
/// # Sealed Trait
///
/// This trait is **sealed** and cannot be implemented outside of this crate.
/// This allows the library to add new methods without breaking changes.
/// The available priority levels are:
/// - [`UserInputPriority`]
/// - [`AnimationPriority`]
/// - [`BuildPriority`]
/// - [`IdlePriority`]
pub trait PriorityLevel: sealed::PriorityLevel + Send + Sync + 'static {
    /// The runtime Priority value
    const VALUE: Priority;

    /// Human-readable name
    const NAME: &'static str;

    /// Numeric value for ordering (higher = more important)
    const LEVEL: u8;
}

impl PriorityLevel for UserInputPriority {
    const VALUE: Priority = Priority::UserInput;
    const NAME: &'static str = "UserInput";
    const LEVEL: u8 = 3;
}

impl PriorityLevel for AnimationPriority {
    const VALUE: Priority = Priority::Animation;
    const NAME: &'static str = "Animation";
    const LEVEL: u8 = 2;
}

impl PriorityLevel for BuildPriority {
    const VALUE: Priority = Priority::Build;
    const NAME: &'static str = "Build";
    const LEVEL: u8 = 1;
}

impl PriorityLevel for IdlePriority {
    const VALUE: Priority = Priority::Idle;
    const NAME: &'static str = "Idle";
    const LEVEL: u8 = 0;
}

// =============================================================================
// Extension Traits
// =============================================================================

/// Extension methods for Priority enum
pub trait PriorityExt {
    /// Check if this priority should be skipped under the given budget policy
    fn should_skip(&self, policy: crate::budget::BudgetPolicy) -> bool;

    /// Get the minimum frame budget utilization at which this priority may be skipped
    fn skip_threshold(&self) -> Percentage;

    /// Check if this priority is higher than another
    fn is_higher_than(&self, other: Priority) -> bool;

    /// Check if this priority is interactive (UserInput or Animation)
    fn is_interactive(&self) -> bool;
}

impl PriorityExt for Priority {
    fn should_skip(&self, policy: crate::budget::BudgetPolicy) -> bool {
        use crate::budget::BudgetPolicy;

        match policy {
            BudgetPolicy::Continue => false,
            BudgetPolicy::SkipIdle => matches!(self, Priority::Idle),
            BudgetPolicy::SkipIdleAndBuild => matches!(self, Priority::Idle | Priority::Build),
            BudgetPolicy::StopAll => true,
        }
    }

    fn skip_threshold(&self) -> Percentage {
        match self {
            Priority::UserInput => Percentage::new(100.0), // Never skip
            Priority::Animation => Percentage::new(100.0), // Never skip
            Priority::Build => Percentage::new(90.0),      // Skip if >90% used
            Priority::Idle => Percentage::new(80.0),       // Skip if >80% used
        }
    }

    fn is_higher_than(&self, other: Priority) -> bool {
        self.as_u8() > other.as_u8()
    }

    fn is_interactive(&self) -> bool {
        matches!(self, Priority::UserInput | Priority::Animation)
    }
}

/// Extension methods for FrameTiming
pub trait FrameTimingExt {
    /// Get elapsed time as type-safe Milliseconds
    fn elapsed(&self) -> Milliseconds;

    /// Get elapsed time as type-safe Seconds
    fn elapsed_seconds(&self) -> Seconds;

    /// Get remaining budget as type-safe Milliseconds
    fn remaining(&self) -> Milliseconds;

    /// Get frame duration configuration
    fn frame_duration(&self) -> FrameDuration;

    /// Get budget utilization as percentage
    fn utilization(&self) -> Percentage;
}

impl FrameTimingExt for FrameTiming {
    fn elapsed(&self) -> Milliseconds {
        Milliseconds::new(self.elapsed_ms())
    }

    fn elapsed_seconds(&self) -> Seconds {
        Seconds::new(self.elapsed_secs())
    }

    fn remaining(&self) -> Milliseconds {
        Milliseconds::new(self.remaining_budget_ms())
    }

    fn frame_duration(&self) -> FrameDuration {
        self.frame_duration
    }

    fn utilization(&self) -> Percentage {
        Percentage::from_ratio(self.elapsed_ms() / self.target_duration_ms())
    }
}

/// Extension methods for FrameBudget
pub trait FrameBudgetExt {
    /// Get elapsed time as type-safe Milliseconds
    fn elapsed(&self) -> Milliseconds;

    /// Get remaining budget as type-safe Milliseconds
    fn remaining(&self) -> Milliseconds;

    /// Get frame duration configuration
    fn frame_duration(&self) -> FrameDuration;

    /// Get budget utilization as percentage
    fn utilization_percent(&self) -> Percentage;

    /// Check if a given priority should execute
    fn should_execute(&self, priority: Priority) -> bool;
}

impl FrameBudgetExt for FrameBudget {
    fn elapsed(&self) -> Milliseconds {
        Milliseconds::new(self.elapsed_ms())
    }

    fn remaining(&self) -> Milliseconds {
        Milliseconds::new(self.remaining_ms())
    }

    fn frame_duration(&self) -> FrameDuration {
        FrameDuration::from_fps(self.target_fps())
    }

    fn utilization_percent(&self) -> Percentage {
        Percentage::from_ratio(self.utilization())
    }

    fn should_execute(&self, priority: Priority) -> bool {
        let util = self.utilization_percent();
        util < priority.skip_threshold()
    }
}

// =============================================================================
// Conversion Traits
// =============================================================================

/// Convert to/from milliseconds
pub trait ToMilliseconds {
    /// Convert to milliseconds
    fn to_ms(&self) -> Milliseconds;
}

impl ToMilliseconds for std::time::Duration {
    fn to_ms(&self) -> Milliseconds {
        Milliseconds::new(self.as_secs_f64() * 1000.0)
    }
}

impl ToMilliseconds for f64 {
    fn to_ms(&self) -> Milliseconds {
        Milliseconds::new(*self)
    }
}

/// Convert to/from seconds
pub trait ToSeconds {
    /// Convert to seconds
    fn to_secs(&self) -> Seconds;
}

impl ToSeconds for std::time::Duration {
    fn to_secs(&self) -> Seconds {
        Seconds::new(self.as_secs_f64())
    }
}

impl ToSeconds for f64 {
    fn to_secs(&self) -> Seconds {
        Seconds::new(*self)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetPolicy;

    #[test]
    fn test_priority_ext_should_skip() {
        assert!(!Priority::UserInput.should_skip(BudgetPolicy::Continue));
        assert!(!Priority::UserInput.should_skip(BudgetPolicy::SkipIdle));
        assert!(!Priority::UserInput.should_skip(BudgetPolicy::SkipIdleAndBuild));
        assert!(Priority::UserInput.should_skip(BudgetPolicy::StopAll));

        assert!(Priority::Idle.should_skip(BudgetPolicy::SkipIdle));
        assert!(Priority::Build.should_skip(BudgetPolicy::SkipIdleAndBuild));
    }

    #[test]
    fn test_priority_ext_is_higher() {
        assert!(Priority::UserInput.is_higher_than(Priority::Animation));
        assert!(Priority::Animation.is_higher_than(Priority::Build));
        assert!(Priority::Build.is_higher_than(Priority::Idle));
        assert!(!Priority::Idle.is_higher_than(Priority::Build));
    }

    #[test]
    fn test_priority_ext_interactive() {
        assert!(Priority::UserInput.is_interactive());
        assert!(Priority::Animation.is_interactive());
        assert!(!Priority::Build.is_interactive());
        assert!(!Priority::Idle.is_interactive());
    }

    #[test]
    fn test_priority_level_sealed() {
        // Compile-time verification that PriorityLevel is implemented
        fn _assert_priority<P: PriorityLevel>() {}

        _assert_priority::<UserInputPriority>();
        _assert_priority::<AnimationPriority>();
        _assert_priority::<BuildPriority>();
        _assert_priority::<IdlePriority>();
    }

    #[test]
    fn test_to_milliseconds() {
        let duration = std::time::Duration::from_millis(100);
        assert_eq!(duration.to_ms().value(), 100.0);

        let f: f64 = 50.0;
        assert_eq!(f.to_ms().value(), 50.0);
    }

    #[test]
    fn test_to_seconds() {
        let duration = std::time::Duration::from_secs(2);
        assert_eq!(duration.to_secs().value(), 2.0);
    }
}
