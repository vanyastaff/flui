//! Animation widgets - explicit and implicit animations.
//!
//! This module provides animation widgets that integrate with `flui_animation`.
//!
//! ## Explicit Animations (Transition widgets)
//!
//! These widgets require the user to provide and manage an `AnimationController`.
//! The widget rebuilds when the animation value changes:
//!
//! - [`FadeTransition`] - Animates opacity
//! - [`SlideTransition`] - Animates position (fractional offset)
//! - [`ScaleTransition`] - Animates scale
//! - [`RotationTransition`] - Animates rotation
//!
//! ## Implicit Animations (coming soon)
//!
//! These widgets automatically animate when their properties change:
//!
//! - `AnimatedOpacity` - Auto-animates opacity changes
//! - `AnimatedContainer` - Auto-animates size/color/padding changes
//!
//! ## Example (Explicit Animation)
//!
//! ```rust,ignore
//! use flui_animation::{AnimationController, Animation};
//! use flui_widgets::animation::FadeTransition;
//! use std::time::Duration;
//!
//! let controller = AnimationController::new(
//!     Duration::from_millis(300),
//!     scheduler
//! );
//! controller.forward();
//!
//! FadeTransition::new(controller.clone())
//!     .child(my_widget)
//! ```

mod fade_transition;
mod rotation_transition;
mod scale_transition;
mod slide_transition;

pub use fade_transition::FadeTransition;
pub use rotation_transition::RotationTransition;
pub use scale_transition::ScaleTransition;
pub use slide_transition::SlideTransition;
