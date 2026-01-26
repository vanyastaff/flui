//! LayerRender trait - GPU rendering extension for layer types.
//!
//! This module adds GPU rendering capabilities to the core layer types
//! from flui-layer.

use super::commands::dispatch_commands;
use super::commands::CommandRenderer;
use flui_layer::{
    BackdropFilterLayer, CanvasLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
    ColorFilterLayer, FollowerLayer, ImageFilterLayer, Layer, LeaderLayer, OffsetLayer,
    OpacityLayer, PerformanceOverlayLayer, PictureLayer, PlatformViewLayer, ShaderMaskLayer,
    TextureLayer, TransformLayer,
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

    /// Clean up any state pushed by render().
    ///
    /// This is called after all children have been rendered to restore
    /// the renderer state (transforms, clips, effects).
    fn cleanup(&self, renderer: &mut R);
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for Layer {
    fn render(&self, renderer: &mut R) {
        match self {
            // Leaf layers
            Layer::Canvas(layer) => layer.render(renderer),
            Layer::Picture(layer) => layer.render(renderer),

            // Clip layers
            Layer::ClipRect(layer) => layer.render(renderer),
            Layer::ClipRRect(layer) => layer.render(renderer),
            Layer::ClipPath(layer) => layer.render(renderer),
            Layer::ClipSuperellipse(layer) => layer.render(renderer),

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

            // Debug/Performance layers
            Layer::PerformanceOverlay(layer) => layer.render(renderer),
        }
    }

    fn cleanup(&self, renderer: &mut R) {
        match self {
            // Leaf layers - no cleanup needed
            Layer::Canvas(layer) => layer.cleanup(renderer),
            Layer::Picture(layer) => layer.cleanup(renderer),

            // Clip layers
            Layer::ClipRect(layer) => layer.cleanup(renderer),
            Layer::ClipRRect(layer) => layer.cleanup(renderer),
            Layer::ClipPath(layer) => layer.cleanup(renderer),
            Layer::ClipSuperellipse(layer) => layer.cleanup(renderer),

            // Transform layers
            Layer::Offset(layer) => layer.cleanup(renderer),
            Layer::Transform(layer) => layer.cleanup(renderer),

            // Effect layers
            Layer::Opacity(layer) => layer.cleanup(renderer),
            Layer::ColorFilter(layer) => layer.cleanup(renderer),
            Layer::ImageFilter(layer) => layer.cleanup(renderer),
            Layer::ShaderMask(layer) => layer.cleanup(renderer),
            Layer::BackdropFilter(layer) => layer.cleanup(renderer),

            // Leaf layers (external content)
            Layer::Texture(layer) => layer.cleanup(renderer),
            Layer::PlatformView(layer) => layer.cleanup(renderer),

            // Linking layers
            Layer::Leader(layer) => layer.cleanup(renderer),
            Layer::Follower(layer) => layer.cleanup(renderer),

            // Annotation layers (metadata only, no cleanup needed)
            Layer::AnnotatedRegion(_) => {}

            // Debug/Performance layers
            Layer::PerformanceOverlay(layer) => layer.cleanup(renderer),
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

    fn cleanup(&self, _renderer: &mut R) {
        // Leaf layer - no state to clean up
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for PictureLayer {
    fn render(&self, renderer: &mut R) {
        dispatch_commands(self.picture().commands(), renderer);
    }

    fn cleanup(&self, _renderer: &mut R) {
        // Leaf layer - no state to clean up
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

    fn cleanup(&self, renderer: &mut R) {
        if self.clips() {
            renderer.pop_clip();
        }
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

    fn cleanup(&self, renderer: &mut R) {
        if self.clips() {
            renderer.pop_clip();
        }
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

    fn cleanup(&self, renderer: &mut R) {
        if self.clips() {
            renderer.pop_clip();
        }
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for flui_layer::ClipSuperellipseLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        // Superellipse clipping - fallback to RRect approximation
        // TODO: Implement native superellipse clipping with proper squircle curve
        let superellipse = self.clip_superellipse();
        let rect = superellipse.outer_rect();
        // Use the corner radii from superellipse to create an approximate RRect
        let rrect = flui_types::geometry::RRect::from_rect_and_corners(
            rect,
            superellipse.tl_radius(),
            superellipse.tr_radius(),
            superellipse.br_radius(),
            superellipse.bl_radius(),
        );
        renderer.push_clip_rrect(&rrect, self.clip_behavior());
    }

    fn cleanup(&self, renderer: &mut R) {
        if self.clips() {
            renderer.pop_clip();
        }
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

    fn cleanup(&self, renderer: &mut R) {
        if !self.is_zero() {
            renderer.pop_transform();
        }
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for TransformLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_identity() {
            return;
        }
        renderer.push_transform(self.transform());
    }

    fn cleanup(&self, renderer: &mut R) {
        if !self.is_identity() {
            renderer.pop_transform();
        }
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

    fn cleanup(&self, renderer: &mut R) {
        if self.is_invisible() || self.is_opaque() {
            return;
        }
        // Pop in reverse order: first opacity, then offset
        renderer.pop_opacity();
        if self.has_offset() {
            renderer.pop_transform();
        }
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ColorFilterLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_identity() {
            return;
        }
        renderer.push_color_filter(self.color_filter());
    }

    fn cleanup(&self, renderer: &mut R) {
        if !self.is_identity() {
            renderer.pop_color_filter();
        }
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ImageFilterLayer {
    fn render(&self, renderer: &mut R) {
        if self.has_offset() {
            renderer.push_offset(self.offset());
        }
        renderer.push_image_filter(self.filter());
    }

    fn cleanup(&self, renderer: &mut R) {
        // Pop in reverse order: first filter, then offset
        renderer.pop_image_filter();
        if self.has_offset() {
            renderer.pop_transform();
        }
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for ShaderMaskLayer {
    fn render(&self, _renderer: &mut R) {
        // TODO: Implement shader mask GPU rendering
    }

    fn cleanup(&self, _renderer: &mut R) {
        // TODO: Implement shader mask cleanup
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for BackdropFilterLayer {
    fn render(&self, _renderer: &mut R) {
        // TODO: Implement backdrop filter GPU rendering
    }

    fn cleanup(&self, _renderer: &mut R) {
        // TODO: Implement backdrop filter cleanup
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

    fn cleanup(&self, _renderer: &mut R) {
        // Leaf layer - no state to clean up
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for PlatformViewLayer {
    fn render(&self, _renderer: &mut R) {
        // Platform views are composited by the platform embedder
    }

    fn cleanup(&self, _renderer: &mut R) {
        // Leaf layer - no state to clean up
    }
}

// ============================================================================
// LINKING LAYERS
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for LeaderLayer {
    fn render(&self, renderer: &mut R) {
        let offset = self.get_offset();
        if offset.dx.0 != 0.0 || offset.dy.0 != 0.0 {
            renderer.push_offset(offset);
        }
    }

    fn cleanup(&self, renderer: &mut R) {
        let offset = self.get_offset();
        if offset.dx.0 != 0.0 || offset.dy.0 != 0.0 {
            renderer.pop_transform();
        }
    }
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for FollowerLayer {
    fn render(&self, _renderer: &mut R) {
        // Transform is calculated by the compositor
    }

    fn cleanup(&self, _renderer: &mut R) {
        // No state to clean up
    }
}

// ============================================================================
// PERFORMANCE OVERLAY LAYER
// ============================================================================

impl<R: CommandRenderer + ?Sized> LayerRender<R> for PerformanceOverlayLayer {
    fn render(&self, renderer: &mut R) {
        renderer.add_performance_overlay(
            self.options_mask(),
            self.bounds(),
            self.fps(),
            self.frame_time_ms(),
            self.total_frames(),
        );
    }

    fn cleanup(&self, _renderer: &mut R) {
        // Leaf layer - no state to clean up
    }
}
