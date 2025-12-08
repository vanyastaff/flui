//! LayerRender trait - GPU rendering extension for layer types.
//!
//! This module adds GPU rendering capabilities to the core layer types
//! from flui-layer.

use super::commands::dispatch_commands;
use super::commands::CommandRenderer;
use flui_layer::{
    BackdropFilterLayer, CanvasLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
    ColorFilterLayer, FollowerLayer, ImageFilterLayer, Layer, LeaderLayer, OffsetLayer,
    OpacityLayer, PlatformViewLayer, ShaderMaskLayer, TextureLayer, TransformLayer,
};
use flui_painting::DisplayListCore;

// ============================================================================
// LAYER RENDER TRAIT
// ============================================================================

/// Extension trait for rendering layers via CommandRenderer.
///
/// This trait adds GPU rendering capabilities to the core layer types
/// from flui-layer.
///
/// Uses static dispatch via generics for zero-overhead renderer calls.
/// The generic parameter `R` is on the trait level for cleaner implementations.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::wgpu::{LayerRender, Backend};
/// use flui_layer::{Layer, CanvasLayer};
///
/// let layer = Layer::Canvas(CanvasLayer::new());
/// layer.render(&mut backend);
/// ```
pub trait LayerRender<R: CommandRenderer + ?Sized> {
    /// Render this layer using the provided command renderer.
    fn render(&self, renderer: &mut R);
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for Layer {
    fn render(&self, renderer: &mut R) {
        match self {
            // Leaf layers
            Layer::Canvas(layer) => layer.render(renderer),

            // Clip layers
            Layer::ClipRect(layer) => layer.render(renderer),
            Layer::ClipRRect(layer) => layer.render(renderer),
            Layer::ClipPath(layer) => layer.render(renderer),

            // Transform layers
            Layer::Offset(layer) => layer.render(renderer),
            Layer::Transform(layer) => layer.render(renderer),

            // Effect layers
            Layer::Opacity(layer) => layer.render(renderer),
            Layer::ColorFilter(layer) => layer.render(renderer),
            Layer::ImageFilter(layer) => layer.render(renderer),
            Layer::ShaderMask(layer) => layer.render(renderer),
            Layer::BackdropFilter(layer) => layer.render(renderer),

            // Leaf layers (external content)
            Layer::Texture(layer) => layer.render(renderer),
            Layer::PlatformView(layer) => layer.render(renderer),

            // Linking layers
            Layer::Leader(layer) => layer.render(renderer),
            Layer::Follower(layer) => layer.render(renderer),

            // Annotation layers (metadata only, no visual rendering)
            Layer::AnnotatedRegion(_) => {
                // AnnotatedRegion is metadata-only, no visual rendering needed
            }
        }
    }
}

// ============================================================================
// LEAF LAYERS
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for CanvasLayer {
    fn render(&self, renderer: &mut R) {
        dispatch_commands(self.display_list().commands(), renderer);
    }
}

// ============================================================================
// CLIP LAYERS
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ClipRectLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        let rect = self.clip_rect();
        renderer.push_clip_rect(&rect, self.clip_behavior());
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ClipRRectLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        let rrect = self.clip_rrect();
        renderer.push_clip_rrect(rrect, self.clip_behavior());
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ClipPathLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        let path = self.clip_path();
        renderer.push_clip_path(path, self.clip_behavior());
    }
}

// ============================================================================
// TRANSFORM LAYERS
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for OffsetLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_zero() {
            return;
        }
        renderer.push_offset(self.offset());
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for TransformLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_identity() {
            return;
        }
        renderer.push_transform(self.transform());
    }
}

// ============================================================================
// EFFECT LAYERS
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for OpacityLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_invisible() {
            return;
        }
        if self.is_opaque() {
            return;
        }
        if self.has_offset() {
            renderer.push_offset(self.offset());
        }
        renderer.push_opacity(self.alpha());
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ColorFilterLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_identity() {
            return;
        }
        renderer.push_color_filter(self.color_filter());
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ImageFilterLayer {
    fn render(&self, renderer: &mut R) {
        if self.has_offset() {
            renderer.push_offset(self.offset());
        }
        renderer.push_image_filter(self.filter());
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ShaderMaskLayer {
    fn render(&self, _renderer: &mut R) {
        // TODO: Implement shader mask GPU rendering
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for BackdropFilterLayer {
    fn render(&self, _renderer: &mut R) {
        // TODO: Implement backdrop filter GPU rendering
    }
}

// ============================================================================
// EXTERNAL CONTENT LAYERS
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for TextureLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_invisible() {
            return;
        }
        renderer.render_texture(
            self.texture_id(),
            self.rect(),
            None,
            self.filter_quality(),
            self.opacity(),
            &flui_types::geometry::Matrix4::IDENTITY,
        );
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for PlatformViewLayer {
    fn render(&self, _renderer: &mut R) {
        // Platform views are composited by the platform embedder
    }
}

// ============================================================================
// LINKING LAYERS
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for LeaderLayer {
    fn render(&self, renderer: &mut R) {
        let offset = self.get_offset();
        if offset.dx != 0.0 || offset.dy != 0.0 {
            renderer.push_offset(offset);
        }
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for FollowerLayer {
    fn render(&self, _renderer: &mut R) {
        // Transform is calculated by the compositor
    }
}
