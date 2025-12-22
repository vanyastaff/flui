//! Implementation of `Layering` trait for `SceneBuilder`.
//!
//! This module bridges the abstract `Layering` trait from `flui-foundation`
//! with the concrete `SceneBuilder` implementation.

use crate::compositor::SceneBuilder;
use flui_foundation::painting::{BlendMode, ClipBehavior, Layering, PaintShader};
use flui_types::geometry::{Matrix4, RRect, Rect};
use flui_types::painting::{Clip, ImageFilter, Paint, Path};

impl Layering for SceneBuilder<'_> {
    fn push_layer(&mut self, bounds: Rect, paint: Option<&Paint>) {
        // For a generic layer with bounds, we use an offset layer at the bounds origin
        // If paint has opacity, we wrap with opacity layer
        if let Some(paint) = paint {
            let opacity = paint.color.alpha_f32();
            if opacity < 1.0 {
                self.push_opacity(opacity);
                return;
            }
        }
        // Default: push an offset layer at bounds origin
        self.push_offset(flui_types::Offset::new(bounds.left(), bounds.top()));
    }

    fn pop_layer(&mut self) {
        self.pop();
    }

    fn push_clip_rect(&mut self, rect: Rect, clip_behavior: ClipBehavior) {
        let clip = convert_clip_behavior(clip_behavior);
        SceneBuilder::push_clip_rect(self, rect, clip);
    }

    fn push_clip_rrect(&mut self, rrect: RRect, clip_behavior: ClipBehavior) {
        let clip = convert_clip_behavior(clip_behavior);
        SceneBuilder::push_clip_rrect(self, rrect, clip);
    }

    fn push_clip_path(&mut self, path: &Path, clip_behavior: ClipBehavior) {
        let clip = convert_clip_behavior(clip_behavior);
        SceneBuilder::push_clip_path(self, path.clone(), clip);
    }

    fn push_transform(&mut self, matrix: Matrix4) {
        SceneBuilder::push_transform(self, matrix);
    }

    fn push_opacity(&mut self, opacity: f32, _bounds: Option<Rect>) {
        SceneBuilder::push_opacity(self, opacity);
    }

    fn push_backdrop_filter(&mut self, filter: &ImageFilter, bounds: Rect) {
        use flui_types::painting::BlendMode as TypesBlendMode;
        SceneBuilder::push_backdrop_filter(self, filter.clone(), TypesBlendMode::SrcOver, bounds);
    }

    fn push_shader_mask(&mut self, _shader: &dyn PaintShader, bounds: Rect, blend_mode: BlendMode) {
        // TODO: Convert PaintShader to ShaderSpec when we have proper conversion
        // For now, use a placeholder linear gradient
        use flui_types::painting::ShaderSpec;
        use flui_types::styling::Color32;

        let shader = ShaderSpec::LinearGradient {
            start: (0.0, 0.0),
            end: (1.0, 1.0),
            colors: vec![Color32::WHITE, Color32::TRANSPARENT],
        };

        let types_blend_mode = convert_blend_mode(blend_mode);
        SceneBuilder::push_shader_mask(self, shader, types_blend_mode, bounds);
    }

    fn pop(&mut self) {
        SceneBuilder::pop(self);
    }

    fn depth(&self) -> usize {
        SceneBuilder::depth(self)
    }
}

/// Converts `flui_foundation::painting::ClipBehavior` to `flui_types::painting::Clip`.
fn convert_clip_behavior(behavior: ClipBehavior) -> Clip {
    match behavior {
        ClipBehavior::None => Clip::None,
        ClipBehavior::HardEdge => Clip::HardEdge,
        ClipBehavior::AntiAlias => Clip::AntiAlias,
        ClipBehavior::AntiAliasWithSaveLayer => Clip::AntiAliasWithSaveLayer,
    }
}

/// Converts `flui_foundation::painting::BlendMode` to `flui_types::painting::BlendMode`.
fn convert_blend_mode(mode: BlendMode) -> flui_types::painting::BlendMode {
    use flui_types::painting::BlendMode as TypesBlendMode;

    match mode {
        BlendMode::Clear => TypesBlendMode::Clear,
        BlendMode::Src => TypesBlendMode::Src,
        BlendMode::Dst => TypesBlendMode::Dst,
        BlendMode::SrcOver => TypesBlendMode::SrcOver,
        BlendMode::DstOver => TypesBlendMode::DstOver,
        BlendMode::SrcIn => TypesBlendMode::SrcIn,
        BlendMode::DstIn => TypesBlendMode::DstIn,
        BlendMode::SrcOut => TypesBlendMode::SrcOut,
        BlendMode::DstOut => TypesBlendMode::DstOut,
        BlendMode::SrcAtop => TypesBlendMode::SrcATop,
        BlendMode::DstAtop => TypesBlendMode::DstATop,
        BlendMode::Xor => TypesBlendMode::Xor,
        BlendMode::Plus => TypesBlendMode::Plus,
        BlendMode::Modulate => TypesBlendMode::Modulate,
        BlendMode::Screen => TypesBlendMode::Screen,
        BlendMode::Overlay => TypesBlendMode::Overlay,
        BlendMode::Darken => TypesBlendMode::Darken,
        BlendMode::Lighten => TypesBlendMode::Lighten,
        BlendMode::ColorDodge => TypesBlendMode::ColorDodge,
        BlendMode::ColorBurn => TypesBlendMode::ColorBurn,
        BlendMode::HardLight => TypesBlendMode::HardLight,
        BlendMode::SoftLight => TypesBlendMode::SoftLight,
        BlendMode::Difference => TypesBlendMode::Difference,
        BlendMode::Exclusion => TypesBlendMode::Exclusion,
        BlendMode::Multiply => TypesBlendMode::Multiply,
        BlendMode::Hue => TypesBlendMode::Hue,
        BlendMode::Saturation => TypesBlendMode::Saturation,
        BlendMode::Color => TypesBlendMode::Color,
        BlendMode::Luminosity => TypesBlendMode::Luminosity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::LayerTree;
    use flui_foundation::painting::Layering;
    use flui_types::geometry::Rect;

    #[test]
    fn test_layering_push_pop() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        assert_eq!(Layering::depth(&builder), 0);

        Layering::push_transform(&mut builder, Matrix4::identity());
        assert_eq!(Layering::depth(&builder), 1);

        Layering::push_opacity(&mut builder, 0.5, None);
        assert_eq!(Layering::depth(&builder), 2);

        Layering::pop(&mut builder);
        assert_eq!(Layering::depth(&builder), 1);

        Layering::pop(&mut builder);
        assert_eq!(Layering::depth(&builder), 0);
    }

    #[test]
    fn test_layering_clip_rect() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        Layering::push_clip_rect(&mut builder, rect, ClipBehavior::AntiAlias);

        assert_eq!(Layering::depth(&builder), 1);

        Layering::pop(&mut builder);
        assert_eq!(Layering::depth(&builder), 0);
    }

    #[test]
    fn test_layering_transform() {
        let mut tree = LayerTree::new();
        let mut builder = SceneBuilder::new(&mut tree);

        let matrix = Matrix4::translation(10.0, 20.0, 0.0);
        Layering::push_transform(&mut builder, matrix);

        assert_eq!(Layering::depth(&builder), 1);
    }
}
