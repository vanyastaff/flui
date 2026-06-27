//! Clip widgets — clip a child to a shape derived from its bounds, over
//! `flui-objects`' `RenderClip` family. Layout is a pass-through; only painting
//! is affected. `ClipPath` clips to an arbitrary user-supplied `Path`.

mod clip_oval;
mod clip_path;
mod clip_rect;
mod clip_rrect;

pub use clip_oval::ClipOval;
pub use clip_path::ClipPath;
pub use clip_rect::ClipRect;
pub use clip_rrect::ClipRRect;
