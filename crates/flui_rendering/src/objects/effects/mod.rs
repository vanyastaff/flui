//! Effect RenderObjects (opacity, transforms, clips, decorations)

pub mod clip_rect;
pub mod opacity;

// Re-exports
pub use clip_rect::{RenderClipRect, Clip};
pub use opacity::RenderOpacity;


