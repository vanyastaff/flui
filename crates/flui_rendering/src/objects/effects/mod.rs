//! Effect RenderObjects (opacity, transforms, clips, decorations)

pub mod clip_rect;
pub mod decorated_box;
pub mod opacity;

// Re-exports
pub use clip_rect::{RenderClipRect, Clip};
pub use decorated_box::{RenderDecoratedBox, DecorationPosition};
pub use opacity::RenderOpacity;



