//! Declarative animated lists: [`AnimatedList`], [`AnimatedGrid`] and
//! [`SliverAnimatedList`].
//!
//! The three share one keyed diff (`roster`) and one stateful sliver: the
//! caller rebuilds with new items, and the list animates the difference —
//! entries in, leaving entries out from their last view, a returning key
//! back with its state, moved items into their new places.

mod roster;
mod sliver;
mod views;

pub use sliver::{SliverAnimatedList, SliverAnimatedListState};
pub use views::{AnimatedGrid, AnimatedList};
