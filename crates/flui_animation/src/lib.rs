//! # flui_animation
//!
//! Persistent animation objects for the FLUI framework.
//!
//! This crate provides stateful animation controllers that survive widget rebuilds,
//! following Flutter's animation architecture with Rust idioms.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                   flui_widgets                          │
//! │  AnimatedWidget, AnimatedBuilder, ImplicitAnimations    │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │ uses
//! ┌──────────────────────▼──────────────────────────────────┐
//! │                 flui_animation                          │
//! │  Animation<T>, AnimationController, CurvedAnimation     │
//! └──────────────────────┬──────────────────────────────────┘
//!                        │ uses
//! ┌──────────────────────▼──────────────────────────────────┐
//! │             flui_types/animation                        │
//! │  Curve, Tween<T>, AnimationStatus (data only)          │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//!
//! - [`Animation<T>`] - Base trait for all animations (extends [`Listenable`])
//! - [`AnimationController`] - Primary animation driver (generates 0.0..1.0)
//! - [`CurvedAnimation`] - Applies easing curves to animations
//! - [`Tween`] - Maps animation values to any type T
//! - [`AnimationError`] - Error type for animation operations
//!
//! ## Persistent Object Pattern
//!
//! Animation objects are **persistent** ([`Arc`]-based) and survive widget rebuilds:
//!
//! ```rust,ignore
//! // Create once (outside widget build)
//! let controller = AnimationController::new(
//!     Duration::from_millis(300),
//!     scheduler,
//! );
//!
//! // Use many times (in widget build)
//! let animation = TweenAnimation::new(tween, controller.clone());
//!
//! // Cleanup when done
//! controller.dispose();
//! ```
//!
//! ## Usage Example
//!
//! ```
//! # fn main() -> Result<(), flui_animation::AnimationError> {
//! use flui_animation::{AnimationController, Animation};
//! use flui_scheduler::Scheduler;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! // Create scheduler and controller
//! let scheduler = Arc::new(Scheduler::new());
//! let controller = AnimationController::new(
//!     Duration::from_millis(300),
//!     scheduler,
//! );
//!
//! // Start animation
//! controller.forward()?;
//!
//! // Get current value
//! let value = controller.value();
//!
//! // Cleanup when done
//! controller.dispose();
//! # Ok(())
//! # }
//! ```
//!
//! ## Feature Flags
//!
//! - `serde` - Enable serialization/deserialization support for animation types
//!
//! [`Animation<T>`]: crate::Animation
//! [`AnimationController`]: crate::AnimationController
//! [`CurvedAnimation`]: crate::CurvedAnimation
//! [`AnimationError`]: crate::AnimationError
//! [`Tween`]: flui_types::animation::Tween
//! [`Listenable`]: flui_foundation::Listenable
//! [`Arc`]: std::sync::Arc

#![warn(missing_docs)]
#![warn(clippy::all)]
pub mod animation;
pub mod builder;
pub mod compound;
pub mod controller;
pub mod curved;
pub mod error;
pub mod ext;
pub mod proxy;
pub mod reverse;
pub mod tween;

// Re-exports for convenience
pub use animation::{Animation, AnimationDirection, DynAnimation, StatusCallback};
pub use builder::AnimationControllerBuilder;
pub use compound::{AnimationOperator, CompoundAnimation};
pub use controller::AnimationController;
pub use curved::CurvedAnimation;
pub use error::AnimationError;
pub use ext::{AnimatableExt, AnimationExt};
pub use proxy::ProxyAnimation;
pub use reverse::ReverseAnimation;
pub use tween::{animate, TweenAnimation};

// Re-export scheduler types for convenience
pub use flui_scheduler::ticker::TickerState;
pub use flui_scheduler::{
    BudgetPolicy, FrameBudget, FramePhase, FrameTiming, Priority, Scheduler, SchedulerBinding,
    TaskQueue, Ticker, TickerCallback, TickerProvider, VsyncCallback, VsyncScheduler,
};

// Re-export types from flui_types for convenience
pub use flui_types::animation::{
    AlignmentTween,
    // Core traits and types
    Animatable,
    // Extension traits
    AnimatableExt as TypesAnimatableExt,
    AnimationBehavior,
    AnimationStatus,
    BorderRadiusTween,
    // Curve types
    BounceInCurve,
    BounceInOutCurve,
    BounceOutCurve,
    // Tween types
    ChainedTween,
    ColorTween,
    ConstantTween,
    Cubic,
    Curve,
    CurveExt,
    CurveTween,
    Curves,
    DecelerateCurve,
    EdgeInsetsTween,
    ElasticInCurve,
    ElasticInOutCurve,
    ElasticOutCurve,
    FlippedCurve,
    FloatTween,
    IntTween,
    Interval,
    Linear,
    OffsetTween,
    RectTween,
    ReverseCurve,
    ReverseTween,
    SawTooth,
    SizeTween,
    StepTween,
    Threshold,
    Tween,
    TweenSequence,
    TweenSequenceItem,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::animation::{Animation, AnimationDirection};
    pub use crate::builder::AnimationControllerBuilder;
    pub use crate::compound::{AnimationOperator, CompoundAnimation};
    pub use crate::controller::AnimationController;
    pub use crate::curved::CurvedAnimation;
    pub use crate::error::AnimationError;
    pub use crate::ext::{AnimatableExt, AnimationExt};
    pub use crate::proxy::ProxyAnimation;
    pub use crate::reverse::ReverseAnimation;
    pub use crate::tween::TweenAnimation;
    pub use flui_types::animation::{
        Animatable, AnimationBehavior, AnimationStatus, Curve, CurveExt, Curves, Tween,
        TweenSequence,
    };

    // Re-export scheduler types
    pub use crate::{
        FrameBudget, FramePhase, Priority, Scheduler, SchedulerBinding, TaskQueue, Ticker,
        TickerProvider,
    };
}
