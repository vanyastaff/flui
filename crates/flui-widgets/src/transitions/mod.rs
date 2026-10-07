//! Transition widgets. Each explicit transition keeps a persistent
//! render-object subscription to its animation, so a tick marks paint or
//! layer work without rebuilding the child subtree; [`AnimatedBuilder`] is the
//! rebuild-per-tick form for anything the transitions do not cover.

mod animated_builder;
mod fade_transition;
mod rotation_transition;
mod scale_transition;
mod slide_transition;
mod transform_view;

pub use animated_builder::{AnimatedBuilder, AnimatedBuilderState};
pub use fade_transition::{FadeTransition, FadeTransitionState};
pub use rotation_transition::{RotationTransition, RotationTransitionState};
pub use scale_transition::{ScaleTransition, ScaleTransitionState};
pub use slide_transition::{SlideTransition, SlideTransitionState};
