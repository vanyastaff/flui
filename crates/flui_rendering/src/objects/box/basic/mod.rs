//! Basic single-child render objects.
//!
//! Simple modifications to a single child, such as adding padding,
//! alignment, or constraints.
//!
//! # Objects
//!
//! | Object | Purpose |
//! |--------|---------|
//! | [`RenderPadding`] | Adds padding around child |
//! | [`RenderAlign`] | Aligns child within available space |
//! | [`RenderConstrainedBox`] | Applies additional constraints |
//! | [`RenderSizedBox`] | Forces specific size |
//! | [`RenderAspectRatio`] | Maintains aspect ratio |

mod align;
mod aspect_ratio;
mod constrained_box;
mod padding;
mod sized_box;

pub use align::*;
pub use aspect_ratio::*;
pub use constrained_box::*;
pub use padding::*;
pub use sized_box::*;
