//! Command renderer trait - visitor interface for DrawCommand execution
//!
//! This trait defines the visitor interface that rendering backends implement
//! to execute drawing commands. It follows the Visitor pattern to separate
//! command data (DrawCommand) from execution logic (CommandRenderer).
//!
//! # Design Principles
//!
//! - **Visitor Pattern**: Commands "accept" a renderer and call the appropriate method
//! - **Dependency Inversion**: High-level code depends on this abstraction (SOLID)
//! - **Strategy Pattern**: Swap implementations at runtime (Wgpu, Debug, Test)
//! - **Single Responsibility**: Each renderer handles one backend
//!
//! # Example
//!
//! ```rust,ignore
//! pub struct WgpuRenderer { /* ... */ }
//!
//! impl CommandRenderer for WgpuRenderer {
//!     fn render_rect(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4) {
//!         self.with_transform(transform, |painter| {
//!             painter.rect(rect, paint);
//!         });
//!     }
//!     // ... other methods
//! }
//! ```

use flui_painting::{BlendMode, Paint, PointMode};
use flui_types::{
    geometry::{Matrix4, Offset, Point, RRect, Rect},
    painting::{Image, Path},
    styling::Color,
    typography::TextStyle,
};

/// Visitor interface for rendering DrawCommands
///
/// Backends implement this trait to provide concrete rendering logic.
/// Each method corresponds to one DrawCommand variant.
///
/// This trait enables:
/// - Multiple rendering backends without changing DisplayList
/// - Type-safe dispatch without giant match statements
/// - Easy testing via TestRenderer implementation
pub trait CommandRenderer {
    // ===== Primitive Shapes =====

    /// Render a filled or stroked rectangle
    fn render_rect(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4);

    /// Render a rounded rectangle
    fn render_rrect(&mut self, rrect: RRect, paint: &Paint, transform: &Matrix4);

    /// Render a circle
    fn render_circle(&mut self, center: Point, radius: f32, paint: &Paint, transform: &Matrix4);

    /// Render an oval (ellipse)
    fn render_oval(&mut self, rect: Rect, paint: &Paint, transform: &Matrix4);

    /// Render a line segment
    fn render_line(&mut self, p1: Point, p2: Point, paint: &Paint, transform: &Matrix4);

    /// Render an arbitrary path
    fn render_path(&mut self, path: &Path, paint: &Paint, transform: &Matrix4);

    // ===== Advanced Shapes =====

    /// Render an arc segment
    fn render_arc(
        &mut self,
        rect: Rect,
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
        points: &[Point],
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Text =====

    /// Render text with given style
    fn render_text(
        &mut self,
        text: &str,
        offset: Offset,
        style: &TextStyle,
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Images =====

    /// Render an image to destination rectangle
    fn render_image(
        &mut self,
        image: &Image,
        dst: Rect,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render a texture atlas with sprites
    #[allow(clippy::too_many_arguments)]
    fn render_atlas(
        &mut self,
        image: &Image,
        sprites: &[Rect],
        transforms: &[Matrix4],
        colors: Option<&[Color]>,
        blend_mode: BlendMode,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with repeat/tiling
    ///
    /// Tiles the image to fill the destination rectangle based on the repeat mode.
    ///
    /// # Arguments
    ///
    /// * `image` - Image to tile
    /// * `dst` - Destination rectangle to fill
    /// * `repeat` - How to repeat the image (repeat-x, repeat-y, repeat, no-repeat)
    /// * `paint` - Optional paint for tinting/opacity
    /// * `transform` - Transform matrix at recording time
    fn render_image_repeat(
        &mut self,
        image: &Image,
        dst: Rect,
        repeat: flui_painting::display_list::ImageRepeat,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with 9-slice/9-patch scaling
    ///
    /// Draws the image with a center slice that scales while corners and edges
    /// maintain their natural size. Used for resizable UI elements like buttons.
    ///
    /// # Arguments
    ///
    /// * `image` - Image to draw
    /// * `center_slice` - Center region that scales (in image coordinates)
    /// * `dst` - Destination rectangle
    /// * `paint` - Optional paint for tinting/opacity
    /// * `transform` - Transform matrix at recording time
    fn render_image_nine_slice(
        &mut self,
        image: &Image,
        center_slice: Rect,
        dst: Rect,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render an image with a color filter applied
    ///
    /// Applies a color transformation (tint, grayscale, sepia, etc.) to the image.
    ///
    /// # Arguments
    ///
    /// * `image` - Image to draw
    /// * `dst` - Destination rectangle
    /// * `filter` - Color filter to apply
    /// * `paint` - Optional paint for additional effects
    /// * `transform` - Transform matrix at recording time
    fn render_image_filtered(
        &mut self,
        image: &Image,
        dst: Rect,
        filter: flui_painting::display_list::ColorFilter,
        paint: Option<&Paint>,
        transform: &Matrix4,
    );

    /// Render a GPU texture referenced by ID
    ///
    /// Renders an external GPU texture (video frame, camera preview, platform view)
    /// to the destination rectangle. The texture must be registered with the
    /// rendering engine's texture registry.
    ///
    /// # Arguments
    ///
    /// * `texture_id` - GPU texture identifier
    /// * `dst` - Destination rectangle
    /// * `src` - Optional source rectangle within texture (None = entire texture)
    /// * `filter_quality` - Quality of texture sampling
    /// * `opacity` - Opacity (0.0 = transparent, 1.0 = opaque)
    /// * `transform` - Transform matrix at recording time
    fn render_texture(
        &mut self,
        texture_id: flui_types::painting::TextureId,
        dst: Rect,
        src: Option<Rect>,
        filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
        transform: &Matrix4,
    );

    // ===== Effects =====

    /// Render a shadow for a path
    fn render_shadow(&mut self, path: &Path, color: Color, elevation: f32, transform: &Matrix4);

    /// Apply a shader as a mask to child content
    ///
    /// This method renders child content to an offscreen texture, applies a shader
    /// as an alpha mask, and composites the result to the framebuffer.
    ///
    /// # Arguments
    ///
    /// * `child` - Child drawing commands to be masked
    /// * `shader` - Shader specification (gradient, solid color, etc.)
    /// * `bounds` - Bounding rectangle for the masked region
    /// * `blend_mode` - Blend mode for final compositing
    /// * `transform` - Transform matrix at recording time
    fn render_shader_mask(
        &mut self,
        child: &flui_painting::DisplayList,
        shader: &flui_painting::Shader,
        bounds: Rect,
        blend_mode: BlendMode,
        transform: &Matrix4,
    );

    // ===== Gradients =====

    /// Render a gradient-filled rectangle
    ///
    /// # Arguments
    ///
    /// * `rect` - Rectangle to fill with gradient
    /// * `shader` - Gradient shader specification
    /// * `transform` - Transform matrix at recording time
    fn render_gradient(&mut self, rect: Rect, shader: &flui_painting::Shader, transform: &Matrix4);

    /// Render a gradient-filled rounded rectangle
    ///
    /// # Arguments
    ///
    /// * `rrect` - Rounded rectangle to fill with gradient
    /// * `shader` - Gradient shader specification
    /// * `transform` - Transform matrix at recording time
    fn render_gradient_rrect(
        &mut self,
        rrect: RRect,
        shader: &flui_painting::Shader,
        transform: &Matrix4,
    );

    /// Fill entire viewport with color
    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4);

    /// Render backdrop filter effect (blur, color adjustments, etc.)
    ///
    /// This captures the backdrop behind the bounds, applies a filter to it,
    /// optionally renders child content on top, and composites the result.
    ///
    /// # Arguments
    ///
    /// * `child` - Optional child drawing commands to render on top
    /// * `filter` - Image filter to apply (blur, color adjustments, etc.)
    /// * `bounds` - Bounding rectangle for backdrop capture
    /// * `blend_mode` - Blend mode for final compositing
    /// * `transform` - Transform matrix at recording time
    fn render_backdrop_filter(
        &mut self,
        child: Option<&flui_painting::DisplayList>,
        filter: &flui_painting::display_list::ImageFilter,
        bounds: Rect,
        blend_mode: BlendMode,
        transform: &Matrix4,
    );

    // ===== Custom Geometry =====

    /// Render custom vertex geometry
    fn render_vertices(
        &mut self,
        vertices: &[Point],
        colors: Option<&[Color]>,
        tex_coords: Option<&[Point]>,
        indices: &[u16],
        paint: &Paint,
        transform: &Matrix4,
    );

    // ===== Clipping =====

    /// Set rectangular clip region
    fn clip_rect(&mut self, rect: Rect, transform: &Matrix4);

    /// Set rounded rectangular clip region
    fn clip_rrect(&mut self, rrect: RRect, transform: &Matrix4);

    /// Set arbitrary path clip region
    fn clip_path(&mut self, path: &Path, transform: &Matrix4);

    // ===== Viewport Information =====

    /// Get the viewport bounds
    fn viewport_bounds(&self) -> Rect;

    // ===== Layer Operations =====

    /// Save canvas state and create a new compositing layer
    ///
    /// This creates an offscreen buffer for subsequent drawing commands.
    /// When `restore_layer` is called, the layer is composited back with
    /// the specified paint settings (opacity, blend mode, etc.).
    ///
    /// # Arguments
    ///
    /// * `bounds` - Optional bounds for the layer (None = unbounded)
    /// * `paint` - Paint to apply when compositing (opacity, blend mode)
    /// * `transform` - Transform matrix at recording time
    fn save_layer(&mut self, bounds: Option<Rect>, paint: &Paint, transform: &Matrix4);

    /// Restore canvas state and composite the saved layer
    ///
    /// Pops the save stack and composites the layer created by `save_layer`
    /// using the paint settings specified when the layer was saved.
    ///
    /// # Arguments
    ///
    /// * `transform` - Transform matrix at recording time
    fn restore_layer(&mut self, transform: &Matrix4);
}
