//! Implicitly-animated widgets — Flutter's `ImplicitlyAnimatedWidget` family.
//!
//! Each widget here animates a visual property *implicitly*: you rebuild it with
//! a new target value and it animates from the old value to the new one, with no
//! explicit `Animation` to manage. Internally each holds a persistent
//! [`AnimationController`](flui_animation::AnimationController) and returns an
//! [`AnimatedBuilder`](crate::AnimatedBuilder) over it.
//!
//! Drive them deterministically by wrapping the subtree in a [`VsyncScope`] over
//! a binding's [`Vsync`](flui_animation::Vsync); without a scope, each is driven
//! by its own scheduler ticker on a real display.

mod animated_align;
mod animated_container;
mod animated_opacity;
mod animated_padding;
mod animated_size;
mod implicitly_animated;
mod ticker_mode;
mod vsync_scope;

pub use animated_align::{AnimatedAlign, AnimatedAlignState};
pub use animated_container::{AnimatedContainer, AnimatedContainerState};
pub use animated_opacity::{AnimatedOpacity, AnimatedOpacityState};
pub use animated_padding::{AnimatedPadding, AnimatedPaddingState};
pub use animated_size::{AnimatedSize, AnimatedSizeState};
pub use ticker_mode::{TickerMode, TickerModeState};
pub use vsync_scope::VsyncScope;
