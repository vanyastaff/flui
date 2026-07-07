//! # `flui_animation`
//!
//! Complete animation system for the FLUI framework.
//!
//! This crate provides all animation primitives: curves, tweens, status types,
//! and stateful animation controllers, following Flutter's animation architecture
//! with Rust idioms.
//!
//! ## Key Components
//!
//! - [`Animation<T>`] - Base trait for all animations (extends [`Listenable`])
//! - [`AnimationController`] - Primary animation driver (generates 0.0..1.0)
//! - [`CurvedAnimation`] - Applies easing curves to animations
//! - [`Curve`] - Easing curve trait with predefined curves in [`Curves`]
//!   (full Penner catalog, M3 [`ThreePointCubic`] emphasized set, [`Split`])
//! - [`Tween`] - Maps animation values to any type T;
//!   [`OklabColorTween`] interpolates colors perceptually (Oklab) instead of
//!   componentwise sRGB
//! - [`smoothing`] - Frame-rate-independent followers beyond Flutter:
//!   [`exp_decay`]/[`Smoothed`] (half-life exponential decay) and
//!   [`SmoothDamp`] (critically damped, max-speed-clamped)
//! - [`AnimatedValue`] - Interruptible spring value with velocity-preserving
//!   retargeting (`#[derive(Animatable)]` for custom types)
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
//! [`Curve`]: crate::Curve
//! [`Curves`]: crate::Curves
//! [`Tween`]: crate::Tween
//! [`Listenable`]: flui_foundation::Listenable
//! [`Arc`]: std::sync::Arc

#![warn(missing_docs)]
// Tracked ship-quality debt:
#![allow(clippy::unwrap_used)] // TODO(ship-wave-3): convert to Result / `expect("BUG: …")` per docs/PANIC-POLICY.md

// Core animation modules
pub mod animation;
pub mod builder;
pub mod compound;
pub mod constant;
pub mod controller;
pub mod curved;
pub mod error;
pub mod ext;
pub mod proxy;
pub mod reverse;
pub mod simulation;
pub mod smoothing;
pub mod spring;
pub mod switch;
pub mod tween;
pub mod vsync;

// Data types (moved from flui_types)
pub mod curve;
pub mod status;
pub mod tween_types;

// Re-exports from animation modules
pub use animation::{Animation, AnimationDirection, StatusCallback};
pub use builder::AnimationControllerBuilder;
pub use compound::{AnimationOperator, CompoundAnimation};
pub use constant::{ALWAYS_COMPLETE, ALWAYS_DISMISSED, ConstantAnimation};
pub use controller::AnimationController;
pub use curved::CurvedAnimation;
pub use error::AnimationError;
pub use ext::{AnimatableExt, AnimationExt};
pub use proxy::ProxyAnimation;
pub use reverse::ReverseAnimation;
pub use simulation::{
    BoundedFrictionSimulation, ClampedSimulation, FrictionSimulation, GravitySimulation,
    ScrollSpringSimulation, Simulation, SpringDescription, SpringSimulation, SpringType, Tolerance,
};
pub use smoothing::{SmoothDamp, Smoothed, exp_decay, exp_decay_half_life};
pub use spring::{AnimatedValue, TwoWayConverter};
// `#[derive(Animatable)]` generates a `TwoWayConverter` impl. It shares the name
// `Animatable` with the trait above but lives in the macro namespace (the serde
// `Serialize` trait+derive pattern), so a single `use flui_animation::Animatable`
// brings in both.
pub use flui_macros::Animatable;
pub use switch::AnimationSwitch;
pub use tween::{TweenAnimation, animate};
pub use vsync::{Vsync, VsyncRegistration};

// Re-exports from data type modules
pub use curve::{
    ArcCurve, BounceInCurve, BounceInOutCurve, BounceOutCurve, CatmullRomCurve, CatmullRomSpline,
    Cubic, Curve, Curve2D, Curve2DSample, Curves, DecelerateCurve, ElasticInCurve,
    ElasticInOutCurve, ElasticOutCurve, FlippedCurve, Interval, Linear, ParametricCurve,
    ReverseCurve, SawTooth, Split, ThreePointCubic, Threshold,
};
pub use status::{AnimationBehavior, AnimationStatus};
pub use tween_types::{
    AlignmentTween, Animatable, AnimatableExt as TweenAnimatableExt, BorderRadiusTween,
    ChainedTween, ColorTween, ConstantTween, CurveExt, CurveTween, EdgeInsetsTween, FloatTween,
    IntTween, Matrix4Tween, OffsetTween, OklabColorTween, RectTween, ReverseTween, SizeTween,
    StepTween, Tween, TweenSequence, TweenSequenceItem,
};

// Re-export scheduler types for convenience.
//
// `SchedulerBinding` was deleted upstream when scheduler binding methods were
// inlined onto `Scheduler` (see scheduler.rs section "Binding Methods
// (formerly on SchedulerBinding trait)"). The animation crate now uses
// `Scheduler` directly.
pub use flui_scheduler::ticker::TickerState;
pub use flui_scheduler::{
    BudgetPolicy, FrameBudget, FramePhase, FrameTiming, Priority, Scheduler, TaskQueue, Ticker,
    TickerCallback, TickerProvider, VsyncCallback, VsyncScheduler,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::animation::{Animation, AnimationDirection};
    pub use crate::builder::AnimationControllerBuilder;
    pub use crate::compound::{AnimationOperator, CompoundAnimation};
    pub use crate::constant::{ALWAYS_COMPLETE, ALWAYS_DISMISSED, ConstantAnimation};
    pub use crate::controller::AnimationController;
    pub use crate::curve::{Curve, Curves};
    pub use crate::curved::CurvedAnimation;
    pub use crate::error::AnimationError;
    pub use crate::ext::{AnimatableExt, AnimationExt};
    pub use crate::proxy::ProxyAnimation;
    pub use crate::reverse::ReverseAnimation;
    pub use crate::simulation::{
        FrictionSimulation, GravitySimulation, Simulation, SpringDescription, SpringSimulation,
        SpringType, Tolerance,
    };
    pub use crate::status::{AnimationBehavior, AnimationStatus};
    pub use crate::switch::AnimationSwitch;
    pub use crate::tween::TweenAnimation;
    pub use crate::tween_types::{Animatable, CurveExt, Tween, TweenSequence};

    // Re-export scheduler types
    pub use crate::{
        FrameBudget, FramePhase, Priority, Scheduler, TaskQueue, Ticker, TickerProvider,
    };
}
