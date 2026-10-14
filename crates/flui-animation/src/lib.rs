//! # `flui_animation`
//!
//! Complete animation system for the FLUI framework.
//!
//! This crate provides all animation primitives: curves, tweens, status types,
//! and stateful animation controllers, in Rust idioms.
//!
//! ## Key Components
//!
//! - [`Animation<T>`] - Base trait for all animations (extends [`flui_foundation::Listenable`])
//! - [`AnimationController`] - Explicit progress driver with checked custom or unbounded ranges
//! - [`CurvedAnimation`] - Applies easing curves to animations
//! - [`Curve`] - Easing curve trait with predefined curves in [`curves`]
//!   (CSS easing presets, polynomial-style approximations, elastic and bounce
//!   formulas, [`ThreePointCubic`] emphasized easing and [`Split`])
//! - [`Tween`] - Maps progress to any `Lerp` value, including color in Oklab
//!   with premultiplied alpha
//! - [`Keyframes`] - A value as a pure function of time: segments timed by
//!   `Duration`, eased, cubic, held or jumping; [`Stagger`] offsets one
//!   track per index
//! - [`simulation`] - Validated physics: springs, friction and a bouncing
//!   scroll fling that rest at a precomputed time
//! - [`AnimatedValue`] - An owned value driven by [`MotionSpec`], with
//!   velocity-preserving retargeting (`#[derive(TwoWayConverter)]` for custom types)
//! - [`VsyncRetirement`] - Terminal registry withdrawal separates committed
//!   kernel closure from cancellation delivery and outgoing capture retirement.
//!   Runtime shutdown prepares every closing presentation before callbacks;
//!   retained widget owners cannot keep their runs active (ADR-0175).
//! - [`AnimationError`] - Error type for animation operations
//!
//! ## Persistent Object Pattern
//!
//! Animation objects share owner-local state through [`Rc`] and survive widget rebuilds:
//!
//! ```
//! # use std::rc::Rc;
//! # use std::time::Duration;
//! # use flui_animation::{AnimationController, Tween, TweenAnimation};
//! # let tween = Tween::new(0.0, 100.0);
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
//! use std::time::Duration;
//!
//! // Create a manually sampled controller
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
//! - `diagnostics` - Optional structured transition events and CPU spans (ADR-0202)
//! - `serde` - Enable serialization/deserialization support for animation types
//!
//! [`Animation<T>`]: crate::Animation
//! [`AnimationController`]: crate::AnimationController
//! [`CurvedAnimation`]: crate::CurvedAnimation
//! [`AnimationError`]: crate::AnimationError
//! [`Curve`]: crate::Curve
//! [`curves`]: crate::curves
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

mod animated_value;
mod animation;
mod builder;
mod channels;
mod constant;
mod controller;
mod curved;
mod driven;
mod error;
mod keyframes;
mod motion;
mod observability;
mod origin;
mod proxy;
mod refusal;
mod repeat;
mod retarget;
mod reverse;
mod run_future;
pub mod simulation;
mod stagger;
mod switch;
mod tween;
mod value_conversion;
mod vsync;
mod vsync_update;

// Data types
pub mod curve;
#[doc(alias = "Curves")]
pub mod curves;
pub mod prelude;
mod status;
mod status_subscription;
mod tween_types;

// Re-exports from animation modules
pub use animated_value::{AnimatedValue, AnimatedValueView, MotionUpdate};
pub use animation::{Animation, StatusCallback, StatusObserver};
pub use builder::{AnimationControllerBuilder, ValueRange};
pub use constant::{ConstantAnimation, UNIT_END_ANIMATION, UNIT_START_ANIMATION};
pub use controller::AnimationController;
pub use curved::CurvedAnimation;
pub use driven::DrivenController;
pub use error::{AnimationError, AnimationInput, RangeDefect};
pub use keyframes::{Keyframes, KeyframesBuilder, KeyframesError};
pub use motion::{
    AnimationTime, FrameTick, InvalidPlaybackRate, MotionClock, MotionOverride, MotionPolicy,
    PlaybackRate,
};
pub use proxy::ProxyAnimation;
pub use refusal::REFUSAL_TARGET;
pub use repeat::{Iterations, RepeatMode, RepeatSpec};
pub use retarget::MotionSpec;
pub use reverse::ReverseAnimation;
pub use run_future::{AnimationRunFuture, RunCanceled};
pub use simulation::{
    BouncingScrollSimulation, BoundedFrictionSimulation, FrictionSimulation, Simulation,
    SimulationBounds, SimulationError, SimulationParameter, SpringDescription, SpringSimulation,
    SpringType, Tolerance,
};
pub use stagger::{Stagger, StaggerOrigin};
pub use status_subscription::StatusSubscription;
pub use value_conversion::{AnimationVector, TwoWayConverter};
pub use vsync_update::{VsyncPublication, VsyncUpdate};
// `#[derive(TwoWayConverter)]` generates `TwoWayConverter` and `Lerp` impls. It
// shares the trait's name but lives in the macro namespace (the serde
// `Serialize` trait+derive pattern), so one `use flui_animation::TwoWayConverter`
// brings in both.
pub use flui_foundation::geometry::Lerp;
pub use flui_macros::TwoWayConverter;
pub use switch::AnimationSwitch;
pub use tween::TweenAnimation;
pub use vsync::{Vsync, VsyncRegistration, VsyncRegistrationError, VsyncRetirement};

// Re-exports from data type modules
pub use curve::{
    ArcCurve, Bounce, Cubic, Curve, CurveError, CurveParameter, Decelerate, Ease, Elastic, Flipped,
    Interval, JumpAt, Linear, LinearStop, PiecewiseLinear, PiecewiseLinearError, Split,
    SpringCurve, Steps, ThreePointCubic,
};
pub use status::{AnimationBehavior, AnimationStatus};
pub use tween_types::{Animatable, IntTween, Rounding, Tween};

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
