//! Transition widgets — rebuild their child each animation tick, mapping an
//! `Animation` value onto a visual property. The reactive spine is
//! [`AnimatedView`](flui_view::prelude::AnimatedView): the element subscribes to
//! the animation's listenable, and a tick schedules a rebuild that re-reads the
//! value.

mod fade_transition;
mod rotation_transition;
mod scale_transition;

pub use fade_transition::{FadeTransition, FadeTransitionState};
pub use rotation_transition::{RotationTransition, RotationTransitionState};
pub use scale_transition::{ScaleTransition, ScaleTransitionState};
