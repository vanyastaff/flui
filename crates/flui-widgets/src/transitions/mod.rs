//! Transition widgets. Most map animation ticks through `AnimatedView` rebuilds;
//! [`FadeTransition`] instead keeps a persistent render-layer subscription so
//! opacity ticks never rebuild its child subtree.

mod animated_builder;
mod fade_transition;
mod rotation_transition;
mod scale_transition;
mod slide_transition;

pub use animated_builder::{AnimatedBuilder, AnimatedBuilderState};
pub use fade_transition::{FadeTransition, FadeTransitionState};
pub use rotation_transition::{RotationTransition, RotationTransitionState};
pub use scale_transition::{ScaleTransition, ScaleTransitionState};
pub use slide_transition::{SlideTransition, SlideTransitionState};
