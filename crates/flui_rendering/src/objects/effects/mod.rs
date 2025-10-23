//! Effect RenderObjects (opacity, transforms, clips, decorations)

pub mod clip_rect;
pub mod clip_rrect;
pub mod decorated_box;
pub mod offstage;
pub mod opacity;
pub mod transform;




// Re-exports
pub use clip_rect::{RenderClipRect, Clip};
pub use decorated_box::{RenderDecoratedBox, DecorationPosition};
pub use offstage::RenderOffstage;
pub use opacity::RenderOpacity;






