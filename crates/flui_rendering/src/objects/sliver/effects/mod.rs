//! Effects sliver render objects.
//!
//! Visual effects and interaction modifiers for slivers.
//!
//! # Objects
//!
//! - [`RenderSliverOpacity`]: Applies opacity to a sliver
//! - [`RenderSliverIgnorePointer`]: Ignores pointer events on a sliver
//! - [`RenderSliverVisibility`]: Controls visibility with fine-grained options

mod ignore_pointer;
mod opacity;
mod visibility;

pub use ignore_pointer::RenderSliverIgnorePointer;
pub use opacity::RenderSliverOpacity;
pub use visibility::RenderSliverVisibility;
