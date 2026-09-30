//! LayerRender trait - GPU rendering extension for layer types.
//!
//! This module adds GPU rendering capabilities to the core layer types
//! from flui-layer.

use flui_layer::{
    BackdropFilterLayer, CanvasLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
    ColorFilterLayer, FollowerLayer, ImageFilterLayer, Layer, LeaderLayer, OffsetLayer,
    OpacityLayer, PerformanceOverlayLayer, PictureLayer, PlatformViewLayer, ShaderMaskLayer,
    TextureLayer, TransformLayer,
};

use crate::{
    command_renderer::CommandRenderer, dispatch::dispatch_commands,
    layer_state_stack::LayerStateStack,
};

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
/// `LayerDispatcher` implements this for every `flui_layer::Layer` variant; the
/// layer walk calls `render` on enter and `cleanup` on exit.
pub(crate) trait LayerRender<R: CommandRenderer + LayerStateStack + ?Sized> {
    /// Render this layer using the provided command renderer.
    fn render(&self, renderer: &mut R);

    /// Clean up any state pushed by render().
    ///
    /// This is called after all children have been rendered to restore
    /// the renderer state (transforms, clips, effects).
    ///
    /// The default is a no-op, correct for every layer whose `render`
    /// pushes nothing: the four clip layers are the ones that override it,
    /// through `clip_layer_cleanup!` below.
    fn cleanup(&self, _renderer: &mut R) {}
}

/// Generate the shared `cleanup` body for the SDF clip layers.
///
/// Each clip layer's `render` returns early when it clips nothing, so its
/// `cleanup` must pop only what was actually pushed — the pairing is the
/// contract, and one body keeps the two from drifting apart.
macro_rules! clip_layer_cleanup {
    () => {
        fn cleanup(&self, renderer: &mut R) {
            if self.clips() {
                renderer.pop_clip();
            }
        }
    };
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for Layer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for CanvasLayer {
    fn render(&self, renderer: &mut R) {
        dispatch_commands(self.display_list().commands(), renderer);
    }
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for PictureLayer {
    fn render(&self, renderer: &mut R) {
        dispatch_commands(self.picture().commands(), renderer);
    }
}

// ============================================================================
// CLIP LAYERS
// ============================================================================

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ClipRectLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        let rect = self.clip_rect();
        renderer.push_clip_rect(&rect, self.clip_behavior());
    }

    clip_layer_cleanup!();
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ClipRRectLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        let rrect = self.clip_rrect();
        renderer.push_clip_rrect(rrect, self.clip_behavior());
    }

    clip_layer_cleanup!();
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ClipPathLayer {
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        let path = self.clip_path();
        renderer.push_clip_path(path, self.clip_behavior());
    }

    clip_layer_cleanup!();
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R>
    for flui_layer::ClipSuperellipseLayer
{
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        // The squircle goes to the shaper of squircles, not through a
        // tessellated path. `push_clip_path` reached a painter call that
        // installed nothing, so this layer used to tessellate a shape and hand
        // it to a discard, leaving its subtree unclipped (issue #921). That
        // call now clips to the path's bounding box (issue #934) — which for a
        // squircle is its outer rect, still not the shape, so the routing
        // below is what earns the corners.
        renderer.push_clip_rsuperellipse(self.clip_superellipse(), self.clip_behavior());
    }

    clip_layer_cleanup!();
}

// ============================================================================
// TRANSFORM LAYERS
// ============================================================================

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for OffsetLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for TransformLayer {
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

/// Whether an opacity layer's composite is the identity — opaque, SrcOver —
/// so no opacity group is pushed. An opaque advanced-blend layer MUST still
/// be pushed so the compositor applies the dst-read blend to its children.
fn opacity_is_identity(layer: &OpacityLayer) -> bool {
    layer.is_opaque() && !layer.blend().is_advanced()
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for OpacityLayer {
    /// The layer's offset applies to its children whatever the alpha, and
    /// alpha 0 is a real opacity group, not a skip — the walk still descends,
    /// so the children composite at zero alpha. That keeps the pushed
    /// translation equal to `Layer::local_translation`, which the follower
    /// resolver sums.
    fn render(&self, renderer: &mut R) {
        if self.has_offset() {
            renderer.push_offset(self.offset());
        }
        if !opacity_is_identity(self) {
            renderer.push_opacity_blend(self.alpha() as f32, self.blend());
        }
    }

    fn cleanup(&self, renderer: &mut R) {
        // Pop in reverse order: first opacity, then offset.
        if !opacity_is_identity(self) {
            renderer.pop_opacity();
        }
        if self.has_offset() {
            renderer.pop_transform();
        }
    }
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ColorFilterLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_identity() {
            return;
        }
        // `color_filter()` returns `ColorFilter` by value (Copy); take a reference
        // to match the `&ColorFilter` trait parameter.
        let filter = self.color_filter();
        renderer.push_color_filter(&filter);
    }

    fn cleanup(&self, renderer: &mut R) {
        if !self.is_identity() {
            renderer.pop_color_filter();
        }
    }
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ImageFilterLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ShaderMaskLayer {
    fn render(&self, renderer: &mut R) {
        // Create a compositing layer bounded to the mask area.
        // Children will be rendered into this layer, then composited
        // with the shader mask applied during restore.
        let paint = flui_painting::Paint::default();
        renderer.save_layer(
            Some(self.bounds()),
            &paint,
            &flui_foundation::geometry::Matrix4::IDENTITY,
        );
        // Clip children to mask bounds so content outside is discarded
        renderer.push_clip_rect(&self.bounds(), flui_painting::paint::Clip::AntiAlias);
    }

    fn cleanup(&self, renderer: &mut R) {
        // Pop in reverse order: first clip, then compositing layer
        renderer.pop_clip();
        renderer.restore_layer(&flui_foundation::geometry::Matrix4::IDENTITY);
    }
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for BackdropFilterLayer {
    fn render(&self, _renderer: &mut R) {
        // Backdrop blur is handled at the Renderer level in render_layer_recursive,
        // which has access to the surface texture for mid-frame flush + copy + blur.
        // This LayerRender impl is a no-op; the Renderer intercepts Layer::BackdropFilter
        // before calling render()/cleanup().
    }

    fn cleanup(&self, _renderer: &mut R) {
        // No-op — see render() comment above.
    }
}

// ============================================================================
// EXTERNAL CONTENT LAYERS
// ============================================================================

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for TextureLayer {
    fn render(&self, renderer: &mut R) {
        if self.is_invisible() {
            return;
        }
        renderer.render_texture(
            self.texture_id(),
            self.bounds(),
            None,
            self.filter_quality(),
            self.opacity() as f32,
            &flui_foundation::geometry::Matrix4::IDENTITY,
        );
    }
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for PlatformViewLayer {
    fn render(&self, _renderer: &mut R) {
        // Platform views are composited by the platform embedder
    }
}

// ============================================================================
// LINKING LAYERS
// ============================================================================

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for LeaderLayer {
    fn render(&self, renderer: &mut R) {
        if !self.offset().is_zero() {
            renderer.push_offset(self.offset());
        }
    }

    fn cleanup(&self, renderer: &mut R) {
        if !self.offset().is_zero() {
            renderer.pop_transform();
        }
    }
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for FollowerLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for PerformanceOverlayLayer {
    /// Replays the readout, shaped upstream, clipped to the bounds: the
    /// bounds are all the damage producer repaints for the overlay each
    /// frame, so a readout recorded past them must not ink outside them. A
    /// hard-edge rect clip is a scissor and opens no offscreen.
    fn render(&self, renderer: &mut R) {
        renderer.push_clip_rect(&self.bounds(), flui_painting::paint::Clip::HardEdge);
        dispatch_commands(self.readout().commands(), renderer);
        renderer.pop_clip();
    }
}
