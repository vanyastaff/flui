//! Composed motion: one [`Effect`] vocabulary, two ways to combine it.
//!
//! - [`Transition`] describes how a child appears and disappears. It holds one
//!   effect per kind, merges with `+` (the right operand wins a kind both
//!   name), and is reversible: an interrupted exit plays back from where it
//!   stopped.
//! - [`Effects`] is a choreography: an ordered timeline whose entries inherit
//!   delay, duration and curve, with [`then`](Effects::then) sequencing and
//!   [`stagger`](Effects::stagger) offsetting siblings.
//!
//! [`Animate`] plays either on a child, started by a [`Trigger`];
//! [`AnimateExt::animate`] is the same on any view. Effects compose in a fixed
//! order — opacity, rotation, scale, translation — through the explicit
//! transitions, so a frame repaints without rebuilding the child.
//! [`AnimatedSwitcher`] plays a [`Transition`] (or a custom builder) between
//! the children it is given in turn.

mod animate;
mod effect;
mod effects;
mod switcher;
mod transition;

pub use animate::{Animate, AnimateExt, AnimateState, Trigger};
pub use effect::Effect;
pub use effects::Effects;
pub use switcher::{
    AnimatedSwitcher, AnimatedSwitcherLayoutBuilder, AnimatedSwitcherState,
    AnimatedSwitcherTransitionBuilder,
};
pub use transition::Transition;
