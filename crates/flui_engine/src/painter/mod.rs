//! Backend-agnostic painting abstraction
//!
//! The Painter trait defines a backend-agnostic interface for rendering.
//! Different backends (egui, wgpu, skia) implement this trait to provide
//! actual rendering capabilities.

use flui_types::{Offset, Rect, Point};

// Backend implementations
#[cfg(feature = "egui")]
pub mod egui;

/// Paint style information
#[derive(Debug, Clone)]
pub struct Paint {
    /// Fill color (RGBA)
    pub color: [f32; 4],

    /// Stroke width (0.0 = fill only)
    pub stroke_width: f32,

    /// Anti-aliasing enabled
    pub anti_alias: bool,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            color: [0.0, 0.0, 0.0, 1.0], // Black
            stroke_width: 0.0,
            anti_alias: true,
        }
    }
}

/// Rounded rectangle
#[derive(Debug, Clone, Copy)]
pub struct RRect {
    pub rect: Rect,
    pub corner_radius: f32,
}

/// Backend-agnostic painter trait
///
/// This trait abstracts over different rendering backends (egui, wgpu, skia, etc).
/// Implementations provide the actual drawing primitives.
///
/// # Design Philosophy
///
/// - **Backend Agnostic**: RenderObjects paint to this trait, not to concrete backends
/// - **Layered**: Paint operations build up a scene graph, not immediate rendering
/// - **Flexible**: Easy to add new backends by implementing this trait
///
/// # Example
///
/// ```rust,ignore
/// fn paint(&self, painter: &mut dyn Painter) {
///     let paint = Paint {
///         color: [1.0, 0.0, 0.0, 1.0], // Red
///         ..Default::default()
///     };
///     painter.rect(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0), &paint);
/// }
/// ```
pub trait Painter {
    // ========== Drawing Primitives ==========

    /// Draw a filled or stroked rectangle
    fn rect(&mut self, rect: Rect, paint: &Paint);

    /// Draw a rounded rectangle
    fn rrect(&mut self, rrect: RRect, paint: &Paint);

    /// Draw a circle
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint);

    /// Draw a line
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint);

    // ========== Transform Stack ==========

    /// Save current transform state
    fn save(&mut self);

    /// Restore previous transform state
    fn restore(&mut self);

    /// Translate coordinate system
    fn translate(&mut self, offset: Offset);

    /// Rotate coordinate system (radians)
    fn rotate(&mut self, angle: f32);

    /// Scale coordinate system
    fn scale(&mut self, sx: f32, sy: f32);

    // ========== Clipping ==========

    /// Clip to rectangle (intersects with current clip)
    fn clip_rect(&mut self, rect: Rect);

    /// Clip to rounded rectangle
    fn clip_rrect(&mut self, rrect: RRect);

    // ========== Advanced ==========

    /// Set opacity for subsequent draws (0.0 = transparent, 1.0 = opaque)
    fn set_opacity(&mut self, opacity: f32);
}

