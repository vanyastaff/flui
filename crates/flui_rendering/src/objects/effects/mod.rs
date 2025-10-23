//! Effect RenderObjects (opacity, transforms, clips, decorations)

pub mod clip_oval;
pub mod clip_path;
pub mod clip_rect;
pub mod clip_rrect;
pub mod custom_paint;
pub mod decorated_box;
pub mod offstage;
pub mod opacity;
pub mod transform;







// Re-exports
pub use clip_oval::RenderClipOval;
pub use clip_path::RenderClipPath;
pub use clip_rect::RenderClipRect;
pub use clip_rrect::RenderClipRRect;
pub use custom_paint::RenderCustomPaint;
pub use decorated_box::{RenderDecoratedBox, DecorationPosition};
pub use offstage::RenderOffstage;
pub use opacity::RenderOpacity;
pub use transform::RenderTransform;









