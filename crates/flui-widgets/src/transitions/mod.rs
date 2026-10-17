//! Transition widgets. Each explicit transition keeps a persistent
//! render-object subscription to its animation, so a tick marks layout, paint
//! or layer work without rebuilding the child subtree; [`AnimatedBuilder`] is
//! the rebuild-per-tick form for anything the transitions do not cover.

mod align_transition;
mod align_view;
mod animated_builder;
mod decorated_box_transition;
mod fade_transition;
mod positioned_transition;
mod proxy_slot;
mod rotation_transition;
mod scale_transition;
mod shimmer_mask_transition;
mod size_transition;
mod slide_transition;
mod sliver_fade_transition;
mod transform_view;

pub use align_transition::{AlignTransition, AlignTransitionState};
pub use animated_builder::{AnimatedBuilder, AnimatedBuilderState};
pub use decorated_box_transition::{DecoratedBoxTransition, DecoratedBoxTransitionState};
pub use fade_transition::{FadeTransition, FadeTransitionState};
pub use positioned_transition::{PositionedTransition, PositionedTransitionState};
pub use rotation_transition::{RotationTransition, RotationTransitionState};
pub use scale_transition::{ScaleTransition, ScaleTransitionState};
pub(crate) use shimmer_mask_transition::ShimmerMaskTransition;
pub use size_transition::{SizeTransition, SizeTransitionState};
pub use slide_transition::{SlideTransition, SlideTransitionState};
pub use sliver_fade_transition::{SliverFadeTransition, SliverFadeTransitionState};
