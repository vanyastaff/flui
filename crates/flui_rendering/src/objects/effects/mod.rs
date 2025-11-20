//! Effect RenderObjects (opacity, transforms, clips, decorations)

// Single Arity - Migrated ✅
pub mod animated_opacity;
pub mod backdrop_filter;
pub mod clip_oval;
pub mod clip_path;
pub mod clip_rect;
pub mod clip_rrect;
pub mod custom_paint;
pub mod offstage;
pub mod opacity;
pub mod repaint_boundary;
pub mod shader_mask;
pub mod transform;
pub mod visibility;

// Optional Arity - Migrated ✅
pub mod decorated_box;

// TODO: Re-enable after migration
// pub mod animated_size;      // Variable arity
// pub mod physical_model;     // Optional arity
// pub mod physical_shape;     // Optional arity

pub mod clip_base; // Helper module

// Re-exports - Single Arity ✅
pub use animated_opacity::RenderAnimatedOpacity;
pub use backdrop_filter::RenderBackdropFilter;
pub use clip_oval::RenderClipOval;
pub use clip_path::{PathClipper, RenderClipPath};
pub use clip_rect::{RectShape, RenderClipRect};
pub use clip_rrect::{RRectShape, RenderClipRRect};
pub use custom_paint::RenderCustomPaint;
pub use offstage::RenderOffstage;
pub use opacity::RenderOpacity;
pub use repaint_boundary::RenderRepaintBoundary;
pub use shader_mask::{RenderShaderMask, ShaderSpec};
pub use transform::RenderTransform;
pub use visibility::RenderVisibility;

// Optional Arity ✅
pub use decorated_box::{DecorationPosition, RenderDecoratedBox};

// TODO: Re-enable after migration
// pub use animated_size::{RenderAnimatedSize, SizeAlignment};
// pub use physical_model::{PhysicalShape, RenderPhysicalModel};
// pub use physical_shape::{RenderPhysicalShape, ShapeClipper};
