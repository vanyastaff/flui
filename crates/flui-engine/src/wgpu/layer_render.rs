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
use flui_painting::DisplayListCore;

use crate::{
    commands::dispatch_commands,
    traits::{CommandRenderer, LayerStateStack},
};

// ============================================================================
// SUPERELLIPSE PATH GENERATION
// ============================================================================
//
// The `SuperellipseKey` cache key + bounded `SuperellipsePathCache` moved
// to `crate::wgpu::superellipse_cache` in U1 of the audit Step 5 item 14
// follow-up. The previous `thread_local!` static cache was unbounded; the
// new bounded cache lives on `WgpuPainter` (mirroring `PathCache`'s
// ownership) and is consulted via `CommandRenderer::superellipse_path`'s
// default + override impls. `generate_superellipse_path` remains here as
// the math reference; both the cache and the trait default invoke it.

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
pub trait LayerRender<R: CommandRenderer + LayerStateStack + ?Sized> {
    /// Render this layer using the provided command renderer.
    fn render(&self, renderer: &mut R);

    /// Clean up any state pushed by render().
    ///
    /// This is called after all children have been rendered to restore
    /// the renderer state (transforms, clips, effects).
    fn cleanup(&self, renderer: &mut R);
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

    fn cleanup(&self, _renderer: &mut R) {
        // Leaf layer - no state to clean up
    }
}

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for PictureLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ClipRectLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ClipRRectLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ClipPathLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R>
    for flui_layer::ClipSuperellipseLayer
{
    fn render(&self, renderer: &mut R) {
        if !self.clips() {
            return;
        }
        let path = renderer.superellipse_path(*self.clip_superellipse());
        renderer.push_clip_path(&path, self.clip_behavior());
    }

    fn cleanup(&self, renderer: &mut R) {
        if self.clips() {
            renderer.pop_clip();
        }
    }
}

/// Generates a proper superellipse (squircle) path from an `RSuperellipse`.
///
/// Uses the parametric superellipse equation with `n = 4` (iOS squircle):
/// ```text
/// x(t) = a * sign(cos(t)) * |cos(t)|^(2/n)
/// y(t) = b * sign(sin(t)) * |sin(t)|^(2/n)
/// ```
///
/// Each corner is generated independently using its own radii, with straight
/// edges connecting the corners. 64 points per corner ensure a smooth curve.
///
/// Exposed `pub(crate)` so `CommandRenderer::superellipse_path`'s default
/// impl can call it for backends that don't own a Painter-side cache.
pub(crate) fn generate_superellipse_path(
    superellipse: &flui_types::geometry::RSuperellipse,
) -> flui_types::painting::Path {
    use flui_types::geometry::{Pixels, Point, px};

    let rect = superellipse.outer_rect();
    let tl = superellipse.tl_radius();
    let tr = superellipse.tr_radius();
    let br = superellipse.br_radius();
    let bl = superellipse.bl_radius();

    let mut path = flui_types::painting::Path::new();

    // iOS squircle exponent
    let n: f32 = 4.0;
    let two_over_n = 2.0 / n;

    // Number of sample points per corner quarter-arc
    let segments_per_corner: usize = 16;

    let left = rect.left().0;
    let top = rect.top().0;
    let right = rect.right().0;
    let bottom = rect.bottom().0;

    // Helper: compute superellipse point for a corner quadrant.
    // `cx`, `cy` is the corner center, `rx`, `ry` are the radii,
    // `t` sweeps through the quarter, `sx`/`sy` select the quadrant direction.
    let se_point =
        |cx: f32, cy: f32, rx: f32, ry: f32, t: f32, sx: f32, sy: f32| -> Point<Pixels> {
            let cos_t = t.cos();
            let sin_t = t.sin();
            let x = cx + sx * rx * cos_t.abs().powf(two_over_n);
            let y = cy + sy * ry * sin_t.abs().powf(two_over_n);
            Point::new(px(x), px(y))
        };

    // Start at top edge, after top-left corner
    // Top-left corner: center at (left + tl.x, top + tl.y)
    // Sweep from PI (left) to PI/2 (top), i.e. t goes PI -> PI/2
    // Direction: sx = -1 (left of center), sy = -1 (above center)
    {
        let cx = left + tl.x.0;
        let cy = top + tl.y.0;
        let rx = tl.x.0;
        let ry = tl.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                // Sweep from PI/2 to 0 (parametric), mapping to top-left quadrant
                let t = std::f32::consts::FRAC_PI_2 * (1.0 - i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, -1.0, -1.0);
                if i == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
        } else {
            path.move_to(Point::new(px(left), px(top)));
        }
    }

    // Top edge -> top-right corner
    // Top-right corner: center at (right - tr.x, top + tr.y)
    // Direction: sx = +1 (right of center), sy = -1 (above center)
    {
        let cx = right - tr.x.0;
        let cy = top + tr.y.0;
        let rx = tr.x.0;
        let ry = tr.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f32::consts::FRAC_PI_2 * (i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, 1.0, -1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(px(right), px(top)));
        }
    }

    // Right edge -> bottom-right corner
    // Bottom-right corner: center at (right - br.x, bottom - br.y)
    // Direction: sx = +1, sy = +1
    {
        let cx = right - br.x.0;
        let cy = bottom - br.y.0;
        let rx = br.x.0;
        let ry = br.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f32::consts::FRAC_PI_2 * (1.0 - i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, 1.0, 1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(px(right), px(bottom)));
        }
    }

    // Bottom edge -> bottom-left corner
    // Bottom-left corner: center at (left + bl.x, bottom - bl.y)
    // Direction: sx = -1, sy = +1
    {
        let cx = left + bl.x.0;
        let cy = bottom - bl.y.0;
        let rx = bl.x.0;
        let ry = bl.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f32::consts::FRAC_PI_2 * (i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, -1.0, 1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(px(left), px(bottom)));
        }
    }

    path.close();
    path
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for OpacityLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for ColorFilterLayer {
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
            &flui_types::geometry::Matrix4::IDENTITY,
        );
        // Clip children to mask bounds so content outside is discarded
        renderer.push_clip_rect(&self.bounds(), flui_types::painting::Clip::AntiAlias);
    }

    fn cleanup(&self, renderer: &mut R) {
        // Pop in reverse order: first clip, then compositing layer
        renderer.pop_clip();
        renderer.restore_layer(&flui_types::geometry::Matrix4::IDENTITY);
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for PlatformViewLayer {
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

impl<R: CommandRenderer + LayerStateStack + ?Sized> LayerRender<R> for LeaderLayer {
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_layer::{
        BackdropFilterLayer, ClipRectLayer, OffsetLayer, OpacityLayer, ShaderMaskLayer,
        TransformLayer,
    };
    use flui_painting::{BlendMode, Paint, PointMode};
    use flui_types::{
        geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, px},
        painting::{Clip, FilterQuality, Image, ImageFilter, Path, TextureId},
        styling::Color,
        typography::TextStyle,
    };

    // ========================================================================
    // MockRenderer — records push/pop/save/restore calls
    // ========================================================================

    struct MockRenderer {
        calls: Vec<String>,
    }

    impl MockRenderer {
        fn new() -> Self {
            Self { calls: Vec::new() }
        }
    }

    impl CommandRenderer for MockRenderer {
        // ===== Primitive Shapes (no-ops) =====
        fn render_rect(&mut self, _rect: Rect<Pixels>, _paint: &Paint, _transform: &Matrix4) {}
        fn render_rrect(&mut self, _rrect: RRect, _paint: &Paint, _transform: &Matrix4) {}
        fn render_circle(
            &mut self,
            _center: Point<Pixels>,
            _radius: f32,
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
        }
        fn render_oval(&mut self, _rect: Rect<Pixels>, _paint: &Paint, _transform: &Matrix4) {}
        fn render_line(
            &mut self,
            _p1: Point<Pixels>,
            _p2: Point<Pixels>,
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
        }
        fn render_path(&mut self, _path: &Path, _paint: &Paint, _transform: &Matrix4) {}

        // ===== Advanced Shapes (no-ops) =====
        fn render_arc(
            &mut self,
            _rect: Rect<Pixels>,
            _start_angle: f32,
            _sweep_angle: f32,
            _use_center: bool,
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
        }
        fn render_drrect(
            &mut self,
            _outer: RRect,
            _inner: RRect,
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
        }
        fn render_points(
            &mut self,
            _mode: PointMode,
            _points: &[Point<Pixels>],
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
        }

        // ===== Text (no-ops) =====
        fn render_text(
            &mut self,
            _text: &str,
            _offset: Offset<Pixels>,
            _style: &TextStyle,
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
        }
        fn render_text_span(
            &mut self,
            _span: &flui_types::typography::InlineSpan,
            _offset: Offset<Pixels>,
            _text_scale_factor: f64,
            _transform: &Matrix4,
        ) {
        }

        // ===== Images (no-ops) =====
        fn render_image(
            &mut self,
            _image: &Image,
            _dst: Rect<Pixels>,
            _paint: Option<&Paint>,
            _transform: &Matrix4,
        ) {
        }
        fn render_atlas(
            &mut self,
            _image: &Image,
            _sprites: &[Rect<Pixels>],
            _transforms: &[Matrix4],
            _colors: Option<&[Color]>,
            _blend_mode: BlendMode,
            _paint: Option<&Paint>,
            _transform: &Matrix4,
        ) {
        }
        fn render_image_repeat(
            &mut self,
            _image: &Image,
            _dst: Rect<Pixels>,
            _repeat: flui_painting::display_list::ImageRepeat,
            _paint: Option<&Paint>,
            _transform: &Matrix4,
        ) {
        }
        fn render_image_nine_slice(
            &mut self,
            _image: &Image,
            _center_slice: Rect<Pixels>,
            _dst: Rect<Pixels>,
            _paint: Option<&Paint>,
            _transform: &Matrix4,
        ) {
        }
        fn render_image_filtered(
            &mut self,
            _image: &Image,
            _dst: Rect<Pixels>,
            _filter: flui_painting::display_list::ColorFilter,
            _paint: Option<&Paint>,
            _transform: &Matrix4,
        ) {
        }
        fn render_texture(
            &mut self,
            _texture_id: TextureId,
            _dst: Rect<Pixels>,
            _src: Option<Rect<Pixels>>,
            _filter_quality: FilterQuality,
            _opacity: f32,
            _transform: &Matrix4,
        ) {
        }

        // ===== Effects (no-ops) =====
        fn render_shadow(
            &mut self,
            _path: &Path,
            _color: Color,
            _elevation: f32,
            _transform: &Matrix4,
        ) {
        }
        fn render_shader_mask(
            &mut self,
            _child: &flui_painting::DisplayList,
            _shader: &flui_painting::Shader,
            _bounds: Rect<Pixels>,
            _blend_mode: BlendMode,
            _transform: &Matrix4,
        ) {
        }

        // ===== Gradients (no-ops) =====
        fn render_gradient(
            &mut self,
            _rect: Rect<Pixels>,
            _shader: &flui_painting::Shader,
            _transform: &Matrix4,
        ) {
        }
        fn render_gradient_rrect(
            &mut self,
            _rrect: RRect,
            _shader: &flui_painting::Shader,
            _transform: &Matrix4,
        ) {
        }
        fn render_color(&mut self, _color: Color, _blend_mode: BlendMode, _transform: &Matrix4) {}
        fn render_paint(&mut self, _paint: &Paint, _transform: &Matrix4) {}
        fn render_backdrop_filter(
            &mut self,
            _child: Option<&flui_painting::DisplayList>,
            _filter: &flui_painting::display_list::ImageFilter,
            _bounds: Rect<Pixels>,
            _blend_mode: BlendMode,
            _transform: &Matrix4,
        ) {
        }

        // ===== Custom Geometry (no-op) =====
        fn render_vertices(
            &mut self,
            _vertices: &[Point<Pixels>],
            _colors: Option<&[Color]>,
            _tex_coords: Option<&[Point<Pixels>]>,
            _indices: &[u16],
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
        }

        // ===== Clipping (no-ops) =====
        fn clip_rect(
            &mut self,
            _rect: Rect<Pixels>,
            _clip_op: flui_types::painting::ClipOp,
            _clip_behavior: flui_types::painting::Clip,
            _transform: &Matrix4,
        ) {
        }
        fn clip_rrect(
            &mut self,
            _rrect: RRect,
            _clip_op: flui_types::painting::ClipOp,
            _clip_behavior: flui_types::painting::Clip,
            _transform: &Matrix4,
        ) {
        }
        fn clip_path(
            &mut self,
            _path: &Path,
            _clip_op: flui_types::painting::ClipOp,
            _clip_behavior: flui_types::painting::Clip,
            _transform: &Matrix4,
        ) {
        }

        // ===== Viewport =====
        fn viewport_bounds(&self) -> Rect<Pixels> {
            Rect::from_xywh(px(0.0), px(0.0), px(800.0), px(600.0))
        }

        // ===== Layer Operations (recorded) =====
        fn save_layer(
            &mut self,
            _bounds: Option<Rect<Pixels>>,
            _paint: &Paint,
            _transform: &Matrix4,
        ) {
            self.calls.push("save_layer".to_string());
        }
        fn restore_layer(&mut self, _transform: &Matrix4) {
            self.calls.push("restore_layer".to_string());
        }

        // Cycle 4 E-9: push/pop methods moved to impl LayerStateStack below.

        // ===== Performance Overlay (recorded) =====
        fn add_performance_overlay(
            &mut self,
            _options_mask: u32,
            _bounds: Rect<Pixels>,
            _fps: f32,
            _frame_time_ms: f32,
            _total_frames: u64,
        ) {
            self.calls.push("add_performance_overlay".to_string());
        }
    }

    // Cycle 4 E-9: layer-tree state-stack impl split into a dedicated
    // trait. MockRenderer records each push/pop as a string for the
    // ordering assertions in the test suite.
    impl LayerStateStack for MockRenderer {
        fn push_clip_rect(&mut self, _rect: &Rect<Pixels>, _clip_behavior: Clip) {
            self.calls.push("push_clip_rect".to_string());
        }
        fn push_clip_rrect(&mut self, _rrect: &RRect, _clip_behavior: Clip) {
            self.calls.push("push_clip_rrect".to_string());
        }
        fn push_clip_path(&mut self, _path: &Path, _clip_behavior: Clip) {
            self.calls.push("push_clip_path".to_string());
        }
        fn pop_clip(&mut self) {
            self.calls.push("pop_clip".to_string());
        }
        fn push_offset(&mut self, _offset: Offset<Pixels>) {
            self.calls.push("push_offset".to_string());
        }
        fn push_transform(&mut self, _transform: &Matrix4) {
            self.calls.push("push_transform".to_string());
        }
        fn pop_transform(&mut self) {
            self.calls.push("pop_transform".to_string());
        }
        fn push_opacity(&mut self, _alpha: f32) {
            self.calls.push("push_opacity".to_string());
        }
        fn pop_opacity(&mut self) {
            self.calls.push("pop_opacity".to_string());
        }
        fn push_color_filter(&mut self, _filter: &flui_types::painting::ColorMatrix) {
            self.calls.push("push_color_filter".to_string());
        }
        fn pop_color_filter(&mut self) {
            self.calls.push("pop_color_filter".to_string());
        }
        fn push_image_filter(&mut self, _filter: &flui_painting::display_list::ImageFilter) {
            self.calls.push("push_image_filter".to_string());
        }
        fn pop_image_filter(&mut self) {
            self.calls.push("pop_image_filter".to_string());
        }
    }

    // ========================================================================
    // OffsetLayer tests
    // ========================================================================

    #[test]
    fn test_offset_layer_pushes_and_pops_transform() {
        let mut renderer = MockRenderer::new();
        let layer = OffsetLayer::from_xy(10.0, 20.0);

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_offset"]);

        layer.cleanup(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_offset", "pop_transform"]);
    }

    #[test]
    fn test_offset_layer_zero_is_noop() {
        let mut renderer = MockRenderer::new();
        let layer = OffsetLayer::zero();

        layer.render(&mut renderer);
        assert!(renderer.calls.is_empty(), "zero offset should not push");

        layer.cleanup(&mut renderer);
        assert!(renderer.calls.is_empty(), "zero offset should not pop");
    }

    // ========================================================================
    // TransformLayer tests
    // ========================================================================

    #[test]
    fn test_transform_layer_pushes_and_pops() {
        let mut renderer = MockRenderer::new();
        let layer = TransformLayer::translation(10.0, 20.0);

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_transform"]);

        layer.cleanup(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_transform", "pop_transform"]);
    }

    #[test]
    fn test_transform_layer_identity_is_noop() {
        let mut renderer = MockRenderer::new();
        let layer = TransformLayer::identity();

        layer.render(&mut renderer);
        assert!(renderer.calls.is_empty(), "identity should not push");

        layer.cleanup(&mut renderer);
        assert!(renderer.calls.is_empty(), "identity should not pop");
    }

    // ========================================================================
    // OpacityLayer tests
    // ========================================================================

    #[test]
    fn test_opacity_layer_pushes_and_pops() {
        let mut renderer = MockRenderer::new();
        let layer = OpacityLayer::new(0.5);

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_opacity"]);

        layer.cleanup(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_opacity", "pop_opacity"]);
    }

    #[test]
    fn test_opacity_layer_with_offset_pushes_offset_then_opacity() {
        let mut renderer = MockRenderer::new();
        let layer = OpacityLayer::with_offset(0.5, Offset::new(px(10.0), px(20.0)));

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_offset", "push_opacity"]);

        layer.cleanup(&mut renderer);
        assert_eq!(
            renderer.calls,
            vec![
                "push_offset",
                "push_opacity",
                "pop_opacity",
                "pop_transform"
            ]
        );
    }

    #[test]
    fn test_opacity_layer_invisible_is_noop() {
        let mut renderer = MockRenderer::new();
        let layer = OpacityLayer::transparent();

        layer.render(&mut renderer);
        assert!(renderer.calls.is_empty(), "invisible should skip render");

        layer.cleanup(&mut renderer);
        assert!(renderer.calls.is_empty(), "invisible should skip cleanup");
    }

    #[test]
    fn test_opacity_layer_opaque_is_noop() {
        let mut renderer = MockRenderer::new();
        let layer = OpacityLayer::opaque();

        layer.render(&mut renderer);
        assert!(renderer.calls.is_empty(), "opaque should skip render");

        layer.cleanup(&mut renderer);
        assert!(renderer.calls.is_empty(), "opaque should skip cleanup");
    }

    // ========================================================================
    // ClipRectLayer tests
    // ========================================================================

    #[test]
    fn test_clip_rect_layer_pushes_and_pops() {
        let mut renderer = MockRenderer::new();
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let layer = ClipRectLayer::new(rect, Clip::HardEdge);

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_clip_rect"]);

        layer.cleanup(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_clip_rect", "pop_clip"]);
    }

    #[test]
    fn test_clip_rect_layer_no_clip_is_noop() {
        let mut renderer = MockRenderer::new();
        let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let layer = ClipRectLayer::new(rect, Clip::None);

        layer.render(&mut renderer);
        assert!(renderer.calls.is_empty(), "Clip::None should not push");

        layer.cleanup(&mut renderer);
        assert!(renderer.calls.is_empty(), "Clip::None should not pop");
    }

    // ========================================================================
    // ShaderMaskLayer tests
    // ========================================================================

    #[test]
    fn test_shader_mask_layer_saves_and_clips() {
        use flui_types::{
            painting::BlendMode as TBlendMode, painting::Shader as TShader, styling::Color,
        };

        let mut renderer = MockRenderer::new();
        let shader = TShader::solid(Color::WHITE);
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
        let layer = ShaderMaskLayer::new(shader, TBlendMode::SrcOver, bounds);

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, vec!["save_layer", "push_clip_rect"]);

        layer.cleanup(&mut renderer);
        assert_eq!(
            renderer.calls,
            vec!["save_layer", "push_clip_rect", "pop_clip", "restore_layer"]
        );
    }

    // ========================================================================
    // BackdropFilterLayer tests
    // ========================================================================

    #[test]
    fn test_backdrop_filter_layer_is_noop() {
        // BackdropFilterLayer rendering is handled at the Renderer level
        // (render_layer_recursive intercepts it for mid-frame flush + blur).
        // The LayerRender impl is intentionally a no-op.
        let mut renderer = MockRenderer::new();
        let filter = ImageFilter::blur(5.0);
        let bounds = Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(150.0));
        let layer =
            BackdropFilterLayer::new(filter, flui_types::painting::BlendMode::SrcOver, bounds);

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, Vec::<String>::new());

        layer.cleanup(&mut renderer);
        assert_eq!(renderer.calls, Vec::<String>::new());
    }

    // ========================================================================
    // Layer enum dispatch tests
    // ========================================================================

    #[test]
    fn test_layer_enum_dispatches_to_offset() {
        let mut renderer = MockRenderer::new();
        let layer = Layer::Offset(OffsetLayer::from_xy(5.0, 10.0));

        layer.render(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_offset"]);

        layer.cleanup(&mut renderer);
        assert_eq!(renderer.calls, vec!["push_offset", "pop_transform"]);
    }

    #[test]
    fn test_layer_enum_annotated_region_is_noop() {
        use std::sync::Arc;
        let mut renderer = MockRenderer::new();
        let layer = Layer::AnnotatedRegion(flui_layer::AnnotatedRegionLayer::new(
            Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0)),
            Arc::new("test annotation".to_string()),
        ));

        layer.render(&mut renderer);
        assert!(renderer.calls.is_empty());

        layer.cleanup(&mut renderer);
        assert!(renderer.calls.is_empty());
    }
}
