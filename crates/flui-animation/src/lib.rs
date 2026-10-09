//! # `flui_animation`
//!
//! Complete animation system for the FLUI framework.
//!
//! This crate provides all animation primitives: curves, tweens, status types,
//! and stateful animation controllers, in Rust idioms.
//!
//! ## Key Components
//!
//! - [`Animation<T>`] - Base trait for all animations (extends [`Listenable`])
//! - [`AnimationController`] - Primary animation driver (generates 0.0..1.0)
//! - [`CurvedAnimation`] - Applies easing curves to animations
//! - [`Curve`] - Easing curve trait with predefined curves in [`Curves`]
//!   (full Penner catalog, M3 [`ThreePointCubic`] emphasized set, [`Split`])
//! - [`Tween`] - Maps animation values to any type T; [`ColorTween`]
//!   interpolates colors in Oklab with premultiplied alpha
//! - [`Keyframes`] - A value as a pure function of time: segments timed by
//!   `Duration`, eased, cubic, held or jumping; [`Stagger`] offsets one
//!   track per index
//! - [`simulation`] - Validated physics: springs, friction and a bouncing
//!   scroll fling that rest at a precomputed time
//! - [`AnimatedValue`] - Interruptible spring value with velocity-preserving
//!   retargeting (`#[derive(TwoWayConverter)]` for custom types)
//! - [`AnimationError`] - Error type for animation operations
//!
//! ## Persistent Object Pattern
//!
//! Animation objects share owner-local state through [`Rc`] and survive widget rebuilds:
//!
//! ```
//! # use std::rc::Rc;
//! # use std::time::Duration;
//! # use flui_animation::{AnimationController, FloatTween, TweenAnimation};
//! # use flui_scheduler::UpdateScheduler;
//! # let scheduler = UpdateScheduler::new();
//! # let tween = FloatTween::new(0.0, 100.0);
//! // Create once (outside widget build)
//! let controller = AnimationController::builder(Duration::from_millis(300)).build();
//!
//! // Use many times (in widget build); `clone()` shares the controller
//! let animation = TweenAnimation::new(tween, Rc::new(controller.clone()));
//!
//! // Cleanup when done
//! drop(controller);
//! ```
//!
//! ## Usage Example
//!
//! ```
//! # fn main() -> Result<(), flui_animation::AnimationError> {
//! use flui_animation::{AnimationController, Animation};
//! use flui_scheduler::UpdateScheduler;
//! use std::time::Duration;
//!
//! // Create scheduler and controller
//! let scheduler = UpdateScheduler::new();
//! let controller = AnimationController::builder(Duration::from_millis(300)).build();
//!
//! // Start animation
//! controller.forward()?;
//!
//! // Get current value
//! let value = controller.value();
//!
//! // Cleanup when done
//! drop(controller);
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
//! [`Rc`]: std::rc::Rc

// Every public item is documented; keep it that way.
#![deny(missing_docs)]
// Crate-local bars above the workspace lint table (a member using
// `[lints] workspace = true` cannot add its own `[lints.clippy]` entries).
// `missing_panics_doc`, `missing_errors_doc`, `allow_attributes_without_reason`,
// `cast_possible_truncation`, `cast_sign_loss` and `clone_on_ref_ptr` still
// have hits in the controller, vsync, proxy, switch and compound modules; they
// turn on here once those modules are reworked.
#![warn(
    clippy::derive_partial_eq_without_eq,
    clippy::return_self_not_must_use,
    clippy::lossy_float_literal,
    clippy::unwrap_in_result,
    clippy::fallible_impl_from
)]

// Core animation modules
// Derive expansions use the same absolute owner path in library and integration targets.
#[allow(
    unused_extern_crates,
    reason = "derive expansions resolve the owner by its absolute crate name"
)]
extern crate self as flui_animation;

#[cfg(test)]
mod test_cases;

pub mod animation;
pub mod builder;
pub mod constant;
pub mod controller;
pub mod curved;
mod driven;
pub mod error;
pub mod ext;
pub mod keyframes;
pub mod motion;
pub mod proxy;
pub mod retarget;
pub mod reverse;
mod run_future;
pub mod simulation;
pub mod spring;
pub mod stagger;
pub mod switch;
pub mod tween;
pub mod vsync;

// Data types
pub mod curve;
pub mod status;
mod status_subscription;
pub mod tween_types;

// Re-exports from animation modules
pub use animation::{Animation, AnimationDirection, StatusCallback};
pub use builder::{AnimationControllerBuilder, ValueRange};
pub use constant::{ALWAYS_COMPLETE, ALWAYS_DISMISSED, ConstantAnimation};
pub use controller::AnimationController;
pub use curved::CurvedAnimation;
pub use driven::DrivenController;
pub use error::AnimationError;
pub use ext::AnimatableExt;
pub use keyframes::{Keyframes, KeyframesBuilder, KeyframesError};
pub use motion::{AnimationTime, FrameTick, InvalidPlaybackRate, MotionClock, PlaybackRate};
pub use proxy::ProxyAnimation;
pub use retarget::MotionSpec;
pub use reverse::ReverseAnimation;
pub use run_future::{AnimationRunFuture, RunCanceled};
pub use simulation::{
    BouncingScrollSimulation, BoundedFrictionSimulation, FrictionSimulation, Simulation,
    SimulationBounds, SimulationError, SimulationParameter, SpringDescription, SpringSimulation,
    SpringType, Tolerance,
};
pub use spring::{
    AnimatedValue, AnimatedValueView, AnimationVector, MotionUpdate, TwoWayConverter,
};
pub use stagger::{Stagger, StaggerOrigin};
pub use status_subscription::StatusSubscription;
// `#[derive(TwoWayConverter)]` generates `TwoWayConverter` and `Lerp` impls. It
// shares the trait's name but lives in the macro namespace (the serde
// `Serialize` trait+derive pattern), so one `use flui_animation::TwoWayConverter`
// brings in both.
pub use flui_foundation::geometry::Lerp;
pub use flui_macros::TwoWayConverter;
pub use switch::AnimationSwitch;
pub use tween::{TweenAnimation, animate};
pub use vsync::{Vsync, VsyncRegistration, VsyncRegistrationError};

// Re-exports from data type modules
pub use curve::{
    ArcCurve, BounceInCurve, BounceInOutCurve, BounceOutCurve, Cubic, Curve, CurveError, Curves,
    DecelerateCurve, ElasticInCurve, ElasticInOutCurve, ElasticOutCurve, FlippedCurve, Interval,
    JumpAt, Linear, Split, Steps, ThreePointCubic,
};
pub use status::{AnimationBehavior, AnimationStatus};
pub use tween_types::{
    AlignmentTween, Animatable, BorderRadiusTween, ChainedTween, ColorTween, ConstantTween,
    CurveTween, EdgeInsetsTween, FloatTween, IntTween, Matrix4Tween, OffsetTween, RectTween,
    ReverseTween, SizeTween, StepTween, Tween,
};

// Every `rust` block in the crate's prose docs compiles as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_examples {}

#[cfg(doctest)]
#[doc = include_str!("../docs/GUIDE.md")]
mod guide_examples {}

#[cfg(doctest)]
#[doc = include_str!("../docs/PERFORMANCE.md")]
mod performance_examples {}
