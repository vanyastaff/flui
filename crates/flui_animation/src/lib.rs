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
//! - **Animation\<T\>**: Base trait for all animations (extends Listenable)
//! - **AnimationController**: Primary animation driver (generates 0.0..1.0)
//! - **CurvedAnimation**: Applies easing curves to animations
//! - **Tween\<T\>**: Maps animation values to any type T
//!
//! ## Persistent Object Pattern
//!
//! Animation objects are **persistent** (Arc-based) and survive widget rebuilds:
//!
//! ```rust,ignore
//! // Create once (outside widget build)
//! let controller = AnimationController::new(
//!     duration: Duration::from_millis(300),
//!     vsync: ticker_provider,
//! );
//!
//! // Use many times (in widget build)
//! let animation = Tween::new(0.0, 1.0).animate(controller.clone());
//!
//! // Cleanup when done
//! controller.dispose();
//! ```
//!
//! ## Usage Example
//!
//! ```rust,ignore
//! use flui_animation::{AnimationController, Animation};
//! use flui_types::animation::Curves;
//! use std::time::Duration;
//!
//! // Create controller
//! let mut controller = AnimationController::new(
//!     Duration::from_millis(300),
//!     ticker_provider,
//! );
//!
//! // Apply curve
//! let curved = controller.curved(Curves::EASE_IN_OUT);
//!
//! // Listen to changes
//! curved.add_listener(|| {
//!     println!("Value: {}", curved.value());
//! });
//!
//! // Start animation
//! controller.forward();
//!
//! // Wait for completion
//! controller.on_complete(|| println!("Done!"));
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
pub mod animation;
pub mod animation_controller;
pub mod compound_animation;
pub mod curved_animation;
pub mod proxy_animation;
pub mod reverse_animation;
pub mod tween_animation;

// Re-exports for convenience
pub use animation::{Animation, AnimationDirection, DynAnimation, StatusCallback};
pub use animation_controller::{AnimationController, AnimationError};
pub use compound_animation::{AnimationOperator, CompoundAnimation};
pub use curved_animation::CurvedAnimation;
pub use proxy_animation::ProxyAnimation;
pub use reverse_animation::ReverseAnimation;
pub use tween_animation::{animate, TweenAnimation};

// Re-export scheduler types for convenience
pub use flui_scheduler::{
    Scheduler, SchedulerBinding, Priority, TaskQueue,
    Ticker, TickerProvider, TickerCallback,
    FrameBudget, BudgetPolicy, FramePhase, FrameTiming,
    VsyncScheduler, VsyncCallback,
};
pub use flui_scheduler::ticker::TickerState;

// Re-export types from flui_types for convenience
pub use flui_types::animation::{Animatable, AnimationStatus, Curve, Curves, Tween, TweenSequence};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::animation::{Animation, AnimationDirection};
    pub use crate::animation_controller::AnimationController;
    pub use crate::compound_animation::{AnimationOperator, CompoundAnimation};
    pub use crate::curved_animation::CurvedAnimation;
    pub use crate::proxy_animation::ProxyAnimation;
    pub use crate::reverse_animation::ReverseAnimation;
    pub use crate::tween_animation::TweenAnimation;
    pub use flui_types::animation::{Animatable, AnimationStatus, Curve, Curves, Tween};

    // Re-export scheduler types
    pub use crate::{
        Scheduler, SchedulerBinding, Priority, TaskQueue,
        Ticker, TickerProvider, FrameBudget, FramePhase,
    };
}
