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

    // ===== Effects =====

    /// Render a shadow for a path
    fn render_shadow(&mut self, path: &Path, color: Color, elevation: f32, transform: &Matrix4);

    /// Fill entire viewport with color
    fn render_color(&mut self, color: Color, blend_mode: BlendMode, transform: &Matrix4);

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
}
