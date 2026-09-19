//! The command dispatch surface between the layer walk and the GPU backend.
//!
//! [`CommandRenderer`](self) mirrors the closed `DrawCommand` enum: one method per
//! variant, and `dispatch_command` (in `crate::dispatch`) is the match that
//! routes each variant to its method. The layer-tree state hand-off — clips,
//! transforms, effects — is the sibling trait in `crate::layer_state_stack`.
//!
//! Both halves are crate plumbing: the layer walk is the only production
//! caller, and `Backend` is the only production implementor.

use flui_painting::{BlendMode, Paint, PointMode};
use std::sync::Arc;

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, RSuperellipse, Rect},
    painting::{Image, Path, TextureId},
    styling::Color,
};

/// The command half of the backend dispatch surface: one method per
/// `flui_painting::DrawCommand` variant.
///
/// `dispatch_command` is the match that routes each variant here, so the two
/// are read together.
///
/// # Implementors
///
/// `Backend` (the production path) and, in tests, a command recorder. The
/// dispatch itself is `dispatch_command`, which is the match over
/// `flui_painting::DrawCommand` — read them together.
pub(crate) trait CommandRenderer {
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

    /// Render a shaped paragraph with its top-left at `offset`; `color`
    /// paints every glyph that carries no span colour of its own.
    fn render_paragraph(
        &mut self,
        layout: &Arc<flui_painting::TextLayout>,
        offset: Offset<Pixels>,
        color: Color,
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
    #[expect(clippy::too_many_arguments)]
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
        repeat: flui_types::painting::image::ImageRepeat,
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
        filter: flui_types::painting::image::ColorFilter,
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

    // ===== Full-target fills =====

    /// Fill entire viewport with color
    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4);

    /// Fill entire viewport with paint (supports shaders, blend modes, etc.)
    fn render_paint(&mut self, paint: &Paint, transform: &Matrix4);

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

    // ===== Layer Operations =====

    /// Save canvas state and create a new compositing layer
    fn save_layer(&mut self, bounds: Option<Rect<Pixels>>, paint: &Paint, transform: &Matrix4);

    /// Restore canvas state and composite the saved layer
    fn restore_layer(&mut self, transform: &Matrix4);

    // ===== Plain State Scope =====

    /// Push the current transform + clip state.
    ///
    /// The plain-state sibling of [`Self::save_layer`]: no offscreen target,
    /// no compositing. A backend that narrows its clip on `clip_*` needs this
    /// marker to know which narrowings to undo, and [`Self::restore_state`] to
    /// undo them.
    fn save_state(&mut self);

    /// Pop the state pushed by the matching [`Self::save_state`].
    ///
    /// Unbalanced pops are a recording bug, not a rendering one — a backend
    /// should report and ignore rather than corrupt its stack.
    fn restore_state(&mut self);

    // ===== Performance Overlay =====

    /// Add a performance overlay to the scene
    ///
    /// This is the equivalent of Flutter's
    /// `SceneBuilder.addPerformanceOverlay()`. Renders FPS counter and
    /// frame timing statistics at the specified location.
    ///
    /// # Arguments
    ///
    /// * `options` - Which readouts to draw
    /// * `bounds` - Rectangle where the overlay should be displayed
    /// * `fps` - Current frames per second
    /// * `frame_time_ms` - Average frame time in milliseconds
    /// * `total_frames` - Total frames rendered
    /// * `diagnostic_line` - Optional runtime-owned structured-metric summary
    fn add_performance_overlay(
        &mut self,
        options: flui_layer::PerformanceOverlayOption,
        bounds: Rect<Pixels>,
        fps: f32,
        frame_time_ms: f32,
        total_frames: u64,
        diagnostic_line: Option<&str>,
    );
}
