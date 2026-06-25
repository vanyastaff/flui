//! Clip widgets — clip a child to a shape derived from its bounds, over
//! `flui-objects`' `RenderClip` family. Layout is a pass-through; only painting
//! is affected. Rounded-rectangle and path clips (`ClipRRect`/`ClipPath`) land
//! with the border-radius / custom-clipper plumbing.

mod clip_oval;
mod clip_rect;

pub use clip_oval::ClipOval;
pub use clip_rect::ClipRect;
