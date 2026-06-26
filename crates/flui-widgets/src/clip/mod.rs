//! Clip widgets — clip a child to a shape derived from its bounds, over
//! `flui-objects`' `RenderClip` family. Layout is a pass-through; only painting
//! is affected. `ClipPath` (arbitrary custom-clipper) lands with the
//! user-clipper plumbing.

mod clip_oval;
mod clip_rect;
mod clip_rrect;

pub use clip_oval::ClipOval;
pub use clip_rect::ClipRect;
pub use clip_rrect::ClipRRect;
