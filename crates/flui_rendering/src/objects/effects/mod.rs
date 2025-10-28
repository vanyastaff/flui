//! Effect RenderObjects (opacity, transforms, clips, decorations)

pub mod animated_opacity;
pub mod backdrop_filter;
pub mod clip_oval;
pub mod clip_path;
pub mod clip_rect;
pub mod clip_rrect;
pub mod custom_paint;
pub mod decorated_box;
pub mod offstage;
pub mod opacity;
pub mod physical_model;
pub mod repaint_boundary;
pub mod shader_mask;
pub mod transform;












// Re-exports
pub use animated_opacity::RenderAnimatedOpacity;
pub use backdrop_filter::{RenderBackdropFilter, ImageFilter};
pub use clip_oval::RenderClipOval;
pub use clip_path::{RenderClipPath, PathClipper};
pub use clip_rect::RenderClipRect;
pub use clip_rrect::RenderClipRRect;
pub use custom_paint::RenderCustomPaint;
pub use decorated_box::{RenderDecoratedBox, DecorationPosition, DecoratedBoxData};
pub use offstage::RenderOffstage;
pub use opacity::RenderOpacity;
pub use physical_model::{RenderPhysicalModel, PhysicalShape};
pub use repaint_boundary::RenderRepaintBoundary;
pub use shader_mask::{RenderShaderMask, ShaderSpec};
pub use transform::RenderTransform;














