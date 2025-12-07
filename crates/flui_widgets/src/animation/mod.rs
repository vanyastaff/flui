//! Animation widgets - explicit and implicit animations
//!
//! This module provides animation widgets that integrate with `flui_animation`.
//!
//! ## Explicit Animations (User-controlled)
//!
//! These widgets require the user to provide and manage an `AnimationController`:
//!
//! - [`FadeTransition`] - Animates opacity based on controller
//! - [`SlideTransition`] - Animates position based on controller
//!
//! ## Implicit Animations (Auto-animated)
//!
//! These widgets automatically animate when their properties change:
//!
//! - [`AnimatedOpacity`] - Automatically animates opacity changes
//!
//! ## Example (Explicit Animation)
//!
//! ```rust,ignore
//! use flui_animation::AnimationController;
//! use flui_widgets::animation::FadeTransition;
//! use std::time::Duration;
//!
//! let controller = AnimationController::new(
//!     Duration::from_millis(300),
//!     scheduler
//! );
//! controller.forward();
//!
//! FadeTransition::new(controller, child)
//! ```
//!
//! ## Example (Implicit Animation)
//!
//! ```rust,ignore
//! use flui_widgets::animation::AnimatedOpacity;
//! use std::time::Duration;
//!
//! // Just change opacity - animation happens automatically!
//! AnimatedOpacity::builder()
//!     .opacity(0.5)
//!     .duration(Duration::from_millis(300))
//!     .child(my_widget)
//!     .build()
//! ```

mod fade_transition;
// mod slide_transition;
// mod animated_opacity;

pub use fade_transition::FadeTransition;
// pub use slide_transition::SlideTransition;
// pub use animated_opacity::AnimatedOpacity;
