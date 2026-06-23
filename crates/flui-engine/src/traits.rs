//! Abstract rendering traits
//!
//! This module defines the abstract traits that rendering backends must
//! implement. These traits enable multiple backend implementations (wgpu, skia,
//! vello, software) without changing the high-level rendering code.
//!
//! # Design Principles
//!
//! - **Backend Agnostic**: Traits define what to render, not how
//! - **Dependency Inversion**: High-level code depends on abstractions (SOLID)
//! - **Extensible**: New backends implement these traits

use flui_painting::{BlendMode, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, RSuperellipse, Rect},
    painting::{Image, Path, TextureId},
    styling::Color,
    typography::TextStyle,
};

// ============================================================================
// COMMAND RENDERER TRAIT
// ============================================================================

/// Visitor interface for rendering DrawCommands
///
/// Backends implement this trait to provide concrete rendering logic.
/// Each method corresponds to one DrawCommand variant.
///
/// This trait enables:
/// - Multiple rendering backends without changing DisplayList
/// - Type-safe dispatch without giant match statements
/// - Easy testing via TestRenderer implementation
///
/// # Example
///
/// ```rust,ignore
/// pub struct WgpuBackend { /* ... */ }
///
/// impl CommandRenderer for WgpuBackend {
///     fn render_rect(&mut self, rect: Rect<Pixels>, paint: &Paint, transform: &Matrix4) {
///         self.with_transform(transform, |painter| {
///             painter.rect(rect, paint);
///         });
///     }
///     // ... other methods
/// }
/// ```
pub trait CommandRenderer {
    // ===== Primitive Shapes =====

    /// Render a filled or stroked rectangle
    fn render_rect(&mut self, rect: Rect<Pixels>, paint: &Paint, transform: &Matrix4);

    /// Render a rounded rectangle
    fn render_rrect(&mut self, rrect: RRect, paint: &Paint, transform: &Matrix4);

    /// Render a circle
    fn render_circle(
        &mut self,
        center: Point<Pixels>,
        radius: f32,
        paint: &Paint,
        transform: &Matrix4,
    );

    /// Render an oval (ellipse)
    fn render_oval(&mut self, rect: Rect<Pixels>, paint: &Paint, transform: &Matrix4);

    /// Render a line segment
    fn render_line(
        &mut self,
        p1: Point<Pixels>,
        p2: Point<Pixels>,
        paint: &Paint,
        transform: &Matrix4,
    );

    /// Render an arbitrary path
    fn render_path(&mut self, path: &Path, paint: &Paint, transform: &Matrix4);

    // ===== Advanced Shapes =====

    /// Render an arc segment
    fn render_arc(
        &mut self,
        rect: Rect<Pixels>,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
        transform: &Matrix4,
    );

    /// Render a double rounded rectangle (ring/border)
    fn render_drrect(&mut self, outer: RRect, inner: RRect, paint: &Paint, transform: &Matrix4);

    /// Render a set of points
    fn render_points(
        &mut self,
        mode: PointMode,
        points: &[Point<Pixels>],
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Text =====

    /// Render text with given style
    fn render_text(
        &mut self,
        text: &str,
        offset: Offset<Pixels>,
        style: &TextStyle,
        paint: &Paint,
        transform: &Matrix4,
    );

    /// Render rich text span
    fn render_text_span(
        &mut self,
        span: &flui_types::typography::InlineSpan,
        offset: Offset<Pixels>,
        text_scale_factor: f64,
        wrap_width: Option<f32>,
        transform: &Matrix4,
    );

    // ===== Images =====

    /// Render an image to destination rectangle
    fn render_image(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render a texture atlas with sprites
    #[allow(clippy::too_many_arguments)]
    fn render_atlas(
        &mut self,
        image: &Image,
        sprites: &[Rect<Pixels>],
        transforms: &[Matrix4],
        colors: Option<&[Color]>,
        blend_mode: BlendMode,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with repeat/tiling
    fn render_image_repeat(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        repeat: flui_painting::display_list::ImageRepeat,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with 9-slice/9-patch scaling
    fn render_image_nine_slice(
        &mut self,
        image: &Image,
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with a color filter applied
    fn render_image_filtered(
        &mut self,
        image: &Image,
        dst: Rect<Pixels>,
        filter: flui_painting::display_list::ColorFilter,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render a GPU texture referenced by ID
    fn render_texture(
        &mut self,
        texture_id: TextureId,
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
        filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
        transform: &Matrix4,
    );

    // ===== Effects =====

    /// Render a shadow for a path
    fn render_shadow(&mut self, path: &Path, color: Color, elevation: f32, transform: &Matrix4);

    /// Apply a shader as a mask to child content
    fn render_shader_mask(
        &mut self,
        child: &flui_painting::DisplayList,
        shader: &flui_painting::Shader,
        bounds: Rect<Pixels>,
        blend_mode: BlendMode,
        transform: &Matrix4,
    );

    // ===== Gradients =====

    /// Render a gradient-filled rectangle
    fn render_gradient(
        &mut self,
        rect: Rect<Pixels>,
        shader: &flui_painting::Shader,
        transform: &Matrix4,
    );

    /// Render a gradient-filled rounded rectangle
    fn render_gradient_rrect(
        &mut self,
        rrect: RRect,
        shader: &flui_painting::Shader,
        transform: &Matrix4,
    );

    /// Fill entire viewport with color
    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4);

    /// Fill entire viewport with paint (supports shaders, blend modes, etc.)
    fn render_paint(&mut self, paint: &Paint, transform: &Matrix4);

    /// Render backdrop filter effect (blur, color adjustments, etc.)
    fn render_backdrop_filter(
        &mut self,
        child: Option<&flui_painting::DisplayList>,
        filter: &flui_painting::display_list::ImageFilter,
        bounds: Rect<Pixels>,
        blend_mode: BlendMode,
        transform: &Matrix4,
    );

    // ===== Custom Geometry =====

    /// Render custom vertex geometry
    fn render_vertices(
        &mut self,
        vertices: &[Point<Pixels>],
        colors: Option<&[Color]>,
        tex_coords: Option<&[Point<Pixels>]>,
        indices: &[u16],
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Clipping =====

    /// Set rectangular clip region
    fn clip_rect(
        &mut self,
        rect: Rect<Pixels>,
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    );

    /// Set rounded rectangular clip region
    fn clip_rrect(
        &mut self,
        rrect: RRect,
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    );

    /// Set rounded-superellipse clip region (Flutter `RSuperellipse`).
    ///
    /// The rounded-superellipse uses a smoother corner falloff than the
    /// elliptical arcs of `RRect`. The default implementation falls back to
    /// `clip_rrect` against an approximating rounded rectangle built from
    /// the superellipse's outer rect and per-corner radii. Backends that
    /// can render the iOS-squircle SDF directly should override this for
    /// pixel-perfect parity with Flutter.
    fn clip_rsuperellipse(
        &mut self,
        rsuperellipse: RSuperellipse,
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    ) {
        // Default approximation: rrect built from outer_rect + per-corner radii.
        // Visually close enough for hard-edge and most anti-aliased cases until
        // a real superellipse SDF lands in the engine fragment shader.
        let rrect = RRect::from_rect_and_corners(
            rsuperellipse.outer_rect(),
            rsuperellipse.tl_radius(),
            rsuperellipse.tr_radius(),
            rsuperellipse.br_radius(),
            rsuperellipse.bl_radius(),
        );
        self.clip_rrect(rrect, clip_op, clip_behavior, transform);
    }

    /// Set arbitrary path clip region
    fn clip_path(
        &mut self,
        path: &Path,
        clip_op: flui_types::painting::ClipOp,
        clip_behavior: flui_types::painting::Clip,
        transform: &Matrix4,
    );

    /// Generate (or retrieve from cache) a tessellated path for an
    /// `RSuperellipse`.
    ///
    /// Used by `ClipSuperellipseLayer::render` for the layer-tree
    /// path-tessellation route. The default implementation freshly
    /// generates the path every call via the iOS-squircle math
    /// (`n = 4`, ~64 sample points per corner) and does NOT cache —
    /// suitable for `DebugBackend` / `MockRenderer` where performance
    /// is not the concern. The production `Backend` overrides to
    /// consult its `Painter`-owned `SuperellipsePathCache` so identical
    /// superellipses across frames reuse the cached tessellation.
    fn superellipse_path(&mut self, rse: RSuperellipse) -> Path {
        crate::wgpu::layer_render::generate_superellipse_path(&rse)
    }

    // ===== Viewport Information =====

    /// Get the viewport bounds
    fn viewport_bounds(&self) -> Rect<Pixels>;

    // ===== Layer Operations =====

    /// Save canvas state and create a new compositing layer
    fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint, transform: &Matrix4);

    /// Restore canvas state and composite the saved layer
    fn restore_layer(&mut self, transform: &Matrix4);

    // ===== Performance Overlay =====

    /// Add a performance overlay to the scene
    ///
    /// This is the equivalent of Flutter's
    /// `SceneBuilder.addPerformanceOverlay()`. Renders FPS counter and
    /// frame timing statistics at the specified location.
    ///
    /// # Arguments
    ///
    /// * `options_mask` - Bitmask of `PerformanceOverlayOption` flags
    /// * `bounds` - Rectangle where the overlay should be displayed
    /// * `fps` - Current frames per second
    /// * `frame_time_ms` - Average frame time in milliseconds
    /// * `total_frames` - Total frames rendered
    fn add_performance_overlay(
        &mut self,
        options_mask: u32,
        bounds: Rect<Pixels>,
        fps: f32,
        frame_time_ms: f32,
        total_frames: u64,
    );
}

// ============================================================================
// LAYER-STATE-STACK TRAIT (cycle 4 E-9 split)
// ============================================================================

/// Compositor hand-off interface for the flui-layer clip/transform/effect
/// stacks.
///
/// Pre-cycle these 13 methods lived on [`CommandRenderer`] alongside the
/// 34 per-command visitor methods. Cycle 4 E-9 split them into this
/// dedicated trait because:
///
/// - The visitor methods (render_rect / render_text / ...) are the
///   `DrawCommand` dispatch contract every backend implements -- a
///   software fallback, a debug recorder, a Skia backend, all
///   conceptually answer "given this draw command, produce these
///   pixels".
/// - The push/pop methods are flui-layer's clip-stack hand-off
///   mechanism. They are framework-internal: the layer tree (see
///   `crates/flui-engine/src/wgpu/layer_render.rs`) walks layers
///   recursively, calling `push_clip_*` / `pop_clip` / `push_opacity`
///   / etc. to mirror the layer-tree's nesting onto the painter's
///   internal state stack.
///
/// Backends that ONLY emit commands (a `DebugBackend` recorder, a
/// command-stream test fixture, a software fallback that does its
/// own state tracking) implement only [`CommandRenderer`]. Backends
/// that participate in flui-layer's clip-stack handshake -- the
/// compositor route -- implement both. The trait split lets new
/// command-only backends materialize without the 13-method overhead.
///
/// # Cycle 4 audit
///
/// See `docs/research/2026-05-22-flui-rendering-engine-audit.md`
/// E-9. Same trait-split intuition as cycle 2 PR #100/U21
/// SemanticsConfiguration split.
pub trait LayerStateStack {
    /// Push a rectangular clip onto the clip stack
    fn push_clip_rect(&mut self, rect: &Rect<Pixels>, clip_behavior: flui_types::painting::Clip);

    /// Push a rounded rectangular clip onto the clip stack
    fn push_clip_rrect(&mut self, rrect: &RRect, clip_behavior: flui_types::painting::Clip);

    /// Push an arbitrary path clip onto the clip stack
    fn push_clip_path(&mut self, path: &Path, clip_behavior: flui_types::painting::Clip);

    /// Pop the most recent clip from the clip stack
    fn pop_clip(&mut self);

    /// Push a translation offset onto the transform stack
    fn push_offset(&mut self, offset: Offset<Pixels>);

    /// Push a full matrix transformation onto the transform stack
    fn push_transform(&mut self, transform: &Matrix4);

    /// Pop the most recent transform from the transform stack
    fn pop_transform(&mut self);

    /// Push an opacity value onto the effect stack
    fn push_opacity(&mut self, alpha: f32);

    /// Push an opacity layer with an explicit blend mode onto the effect stack.
    ///
    /// The default implementation forwards to [`push_opacity`](Self::push_opacity),
    /// which is correct for command-only backends that do not participate in the
    /// dst-read compositor path.  The `wgpu` backend overrides this to route
    /// advanced blend modes through `save_layer` with the blend propagated.
    fn push_opacity_blend(&mut self, alpha: f32, blend: flui_types::painting::BlendMode) {
        let _ = blend;
        self.push_opacity(alpha);
    }

    /// Pop the most recent opacity from the effect stack
    fn pop_opacity(&mut self);

    /// Push a color filter onto the effect stack.
    ///
    /// Accepts the full [`flui_types::painting::ColorFilter`] enum — `Matrix`,
    /// `Mode`, `LinearToSrgbGamma`, and `SrgbToLinearGamma` — so all engine
    /// GPU passes are reachable from a single trait method.
    fn push_color_filter(&mut self, filter: &flui_types::painting::ColorFilter);

    /// Pop the most recent color filter from the effect stack
    fn pop_color_filter(&mut self);

    /// Push an image filter onto the effect stack
    fn push_image_filter(&mut self, filter: &flui_painting::display_list::ImageFilter);

    /// Pop the most recent image filter from the effect stack
    fn pop_image_filter(&mut self);
}
