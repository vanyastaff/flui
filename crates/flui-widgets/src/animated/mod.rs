//! Implicitly-animated widgets.
//!
//! Each widget here animates a visual property *implicitly*: you rebuild it with
//! a new target value and it animates from the old value to the new one, with no
//! explicit `Animation` to manage. Owning motion retains values and velocities
//! across reconfiguration; inner builders observe its published samples.
//!
//! Drive them deterministically by wrapping the subtree in a [`VsyncScope`] over
//! a binding's [`Vsync`](flui_animation::Vsync); without a scope, newly admitted
//! motion settles synchronously.

mod animated;
mod animated_align;
mod animated_container;
mod animated_lerp;
mod animated_opacity;
mod animated_padding;
mod animated_positioned;
mod animated_rotation;
mod animated_scale;
mod animated_size;
mod animated_slide;
mod animated_switcher;
pub(crate) mod implicitly_animated;
mod property_motion;
mod ticker_mode;
mod vsync_scope;

pub use animated::{Animated, AnimatedState};
pub use animated_align::{AnimatedAlign, AnimatedAlignState};
pub use animated_container::{AnimatedContainer, AnimatedContainerState};
pub use animated_lerp::{AnimatedLerp, AnimatedLerpState};
pub use animated_opacity::{AnimatedOpacity, AnimatedOpacityState};
pub use animated_padding::{AnimatedPadding, AnimatedPaddingState};
pub use animated_positioned::{AnimatedPositioned, AnimatedPositionedState};
pub use animated_rotation::{AnimatedRotation, AnimatedRotationState, RotationPath};
pub use animated_scale::{AnimatedScale, AnimatedScaleState};
pub use animated_size::{AnimatedSize, AnimatedSizeState};
pub use animated_slide::{AnimatedSlide, AnimatedSlideState};
pub use animated_switcher::{
    AnimatedSwitcher, AnimatedSwitcherLayoutBuilder, AnimatedSwitcherState,
    AnimatedSwitcherTransitionBuilder,
};
pub use implicitly_animated::MotionBuilder;
pub use ticker_mode::{TickerMode, TickerModeState};
pub use vsync_scope::VsyncScope;
