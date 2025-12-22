//! Painting traits for the FLUI rendering system.
//!
//! This module defines the core traits for painting operations that can be implemented
//! by different backends (Skia, wgpu, Canvas2D, etc.). The traits are designed to be
//! minimal and composable, with rich API provided by `PaintContext` in `flui_rendering`.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     flui-foundation                         │
//! │  ┌────────────────────────────────────────────────────────┐│
//! │  │ painting.rs                                            ││
//! │  │ - trait Painter (draw operations)                      ││
//! │  │ - trait Layering (layer composition)                   ││
//! │  │ - trait Effects (visual effects)                       ││
//! │  │ - trait Caching (optimization)                         ││
//! │  └────────────────────────────────────────────────────────┘│
//! └──────────────────────────┬──────────────────────────────────┘
//!                            │ depends on
//!            ┌───────────────┼───────────────┐
//!            ▼               ▼               ▼
//!     ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
//!     │flui_painting│ │ flui-layer  │ │ flui_engine │
//!     │             │ │             │ │             │
//!     │impl Painter │ │impl Layering│ │impl Effects │
//!     │for Canvas   │ │for Scene    │ │for Wgpu     │
//!     └─────────────┘ └─────────────┘ └─────────────┘
//! ```
//!
//! # Traits Overview
//!
//! - [`Painter`]: Core drawing operations (shapes, paths, images, text)
//! - [`Layering`]: Layer composition and clipping
//! - [`Effects`]: Visual effects (blur, opacity, filters)
//! - [`Caching`]: Paint caching and optimization hints
//!
//! # Example
//!
//! ```ignore
//! use flui_foundation::painting::{Painter, Layering, CacheHint};
//!
//! fn paint_widget(painter: &mut dyn Painter, layering: &mut dyn Layering) {
//!     painter.save();
//!     painter.translate(10.0, 20.0);
//!     painter.draw_rect(rect, &paint);
//!     painter.restore();
//! }
//! ```

use flui_types::geometry::{Matrix4, Offset, Point, RRect, Rect};
use flui_types::painting::{ImageFilter, Paint, Path};

// ============================================================================
// PAINTER TRAIT
// ============================================================================

/// Core drawing operations trait.
///
/// This trait defines the fundamental drawing primitives that any rendering
/// backend must implement. It includes state management (save/restore),
/// transformations, clipping, and drawing operations.
///
/// # State Management
///
/// The painter maintains a stack of states. Each state includes:
/// - Current transformation matrix
/// - Current clip region
/// - Other backend-specific state
///
/// Use [`save`](Painter::save) and [`restore`](Painter::restore) to manage state.
///
/// # Transformations
///
/// Transformations are applied to all subsequent drawing operations until
/// the state is restored. They accumulate (multiply) with existing transforms.
///
/// # Drawing Order
///
/// Drawing operations are performed in the order they are called. Later
/// operations draw on top of earlier ones (painter's algorithm).
pub trait Painter: Send + Sync {
    // ════════════════════════════════════════════════════════════════════════
    // STATE MANAGEMENT
    // ════════════════════════════════════════════════════════════════════════

    /// Saves the current state onto an internal stack.
    ///
    /// Call [`restore`](Painter::restore) to return to this state.
    /// Save/restore pairs can be nested.
    fn save(&mut self);

    /// Restores the state to what it was at the last [`save`](Painter::save) call.
    ///
    /// # Panics
    ///
    /// May panic if called more times than [`save`](Painter::save).
    fn restore(&mut self);

    /// Returns the current save count (number of unrestored saves).
    fn save_count(&self) -> usize;

    /// Restores state to a specific save count.
    ///
    /// Useful for restoring multiple states at once.
    fn restore_to_count(&mut self, count: usize) {
        while self.save_count() > count {
            self.restore();
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // TRANSFORMATIONS
    // ════════════════════════════════════════════════════════════════════════

    /// Translates the canvas by (dx, dy).
    ///
    /// All subsequent drawing operations will be offset by this amount.
    fn translate(&mut self, dx: f32, dy: f32);

    /// Rotates the canvas by the given angle in radians.
    ///
    /// Positive angles rotate clockwise.
    fn rotate(&mut self, radians: f32);

    /// Scales the canvas by (sx, sy).
    ///
    /// Values greater than 1.0 enlarge, less than 1.0 shrink.
    fn scale(&mut self, sx: f32, sy: f32);

    /// Skews the canvas.
    ///
    /// - `sx`: Horizontal skew factor
    /// - `sy`: Vertical skew factor
    fn skew(&mut self, sx: f32, sy: f32);

    /// Applies a 4x4 transformation matrix.
    ///
    /// The matrix is multiplied with the current transformation.
    fn transform(&mut self, matrix: &Matrix4);

    /// Resets the transformation to identity.
    fn reset_transform(&mut self);

    /// Gets the current transformation matrix.
    fn get_transform(&self) -> Matrix4;

    // ════════════════════════════════════════════════════════════════════════
    // CLIPPING
    // ════════════════════════════════════════════════════════════════════════

    /// Clips to a rectangle.
    ///
    /// All subsequent drawing will be clipped to this rectangle
    /// (intersected with any existing clip).
    fn clip_rect(&mut self, rect: Rect);

    /// Clips to a rounded rectangle.
    fn clip_rrect(&mut self, rrect: RRect);

    /// Clips to a path.
    fn clip_path(&mut self, path: &Path);

    // ════════════════════════════════════════════════════════════════════════
    // DRAWING PRIMITIVES
    // ════════════════════════════════════════════════════════════════════════

    /// Draws a rectangle.
    fn draw_rect(&mut self, rect: Rect, paint: &Paint);

    /// Draws a rounded rectangle.
    fn draw_rrect(&mut self, rrect: RRect, paint: &Paint);

    /// Draws a circle.
    fn draw_circle(&mut self, center: Point, radius: f32, paint: &Paint);

    /// Draws an oval (ellipse) inscribed in the given rectangle.
    fn draw_oval(&mut self, rect: Rect, paint: &Paint);

    /// Draws a line between two points.
    fn draw_line(&mut self, p1: Point, p2: Point, paint: &Paint);

    /// Draws a path.
    fn draw_path(&mut self, path: &Path, paint: &Paint);

    /// Draws an arc.
    ///
    /// - `rect`: Bounding rectangle of the oval containing the arc
    /// - `start_angle`: Starting angle in radians (0 = 3 o'clock)
    /// - `sweep_angle`: Sweep angle in radians (positive = clockwise)
    /// - `use_center`: If true, includes lines to center (pie slice)
    fn draw_arc(
        &mut self,
        rect: Rect,
        start_angle: f32,
        sweep_angle: f32,
        use_center: bool,
        paint: &Paint,
    );

    /// Draws multiple points.
    fn draw_points(&mut self, points: &[Point], paint: &Paint);

    // ════════════════════════════════════════════════════════════════════════
    // COMPLEX DRAWING
    // ════════════════════════════════════════════════════════════════════════

    /// Draws an image at the given offset.
    fn draw_image(&mut self, image: &dyn PaintImage, offset: Offset, paint: &Paint);

    /// Draws a portion of an image into a destination rectangle.
    ///
    /// - `src`: Source rectangle in image coordinates
    /// - `dst`: Destination rectangle in canvas coordinates
    fn draw_image_rect(&mut self, image: &dyn PaintImage, src: Rect, dst: Rect, paint: &Paint);

    /// Draws a nine-patch image.
    ///
    /// The image is divided into 9 regions using `center` rect, and the
    /// corners are drawn without scaling while edges and center are stretched.
    fn draw_image_nine(&mut self, image: &dyn PaintImage, center: Rect, dst: Rect, paint: &Paint);

    /// Draws a text paragraph at the given offset.
    fn draw_paragraph(&mut self, paragraph: &dyn PaintParagraph, offset: Offset);

    // ════════════════════════════════════════════════════════════════════════
    // LAYER OPERATIONS
    // ════════════════════════════════════════════════════════════════════════

    /// Saves state and creates a new layer with the given bounds.
    ///
    /// The layer acts as an off-screen buffer. When [`restore`](Painter::restore)
    /// is called, the layer is composited back using the given paint.
    fn save_layer(&mut self, bounds: Option<Rect>, paint: Option<&Paint>);

    /// Saves state and creates a new layer with the given opacity.
    ///
    /// This is a convenience method equivalent to `save_layer` with a paint
    /// that has the specified alpha value.
    fn save_layer_alpha(&mut self, bounds: Option<Rect>, alpha: u8);

    // ════════════════════════════════════════════════════════════════════════
    // MISCELLANEOUS
    // ════════════════════════════════════════════════════════════════════════

    /// Fills the entire canvas with the given color.
    fn clear(&mut self, color: u32);

    /// Draws a shadow for the given path.
    fn draw_shadow(&mut self, path: &Path, color: u32, elevation: f32, transparent_occluder: bool);
}

// ============================================================================
// LAYERING TRAIT
// ============================================================================

/// Layer composition operations trait.
///
/// This trait handles the composition of visual layers, including push/pop
/// operations for various layer types (clips, transforms, opacity).
///
/// Unlike [`Painter`] which works with immediate drawing commands, `Layering`
/// builds a tree structure of composited layers that can be efficiently
/// rasterized and cached.
///
/// # Layer Types
///
/// - **Clip layers**: Restrict drawing to a region
/// - **Transform layers**: Apply transformations to children
/// - **Opacity layers**: Apply transparency to children
/// - **Effect layers**: Apply visual effects (blur, filters)
pub trait Layering: Send + Sync {
    /// Pushes a new layer with optional bounds and paint.
    fn push_layer(&mut self, bounds: Rect, paint: Option<&Paint>);

    /// Pops the most recent layer.
    fn pop_layer(&mut self);

    /// Pushes a rectangular clip.
    fn push_clip_rect(&mut self, rect: Rect, clip_behavior: ClipBehavior);

    /// Pushes a rounded rectangle clip.
    fn push_clip_rrect(&mut self, rrect: RRect, clip_behavior: ClipBehavior);

    /// Pushes a path clip.
    fn push_clip_path(&mut self, path: &Path, clip_behavior: ClipBehavior);

    /// Pushes a transformation layer.
    fn push_transform(&mut self, matrix: Matrix4);

    /// Pushes an opacity layer.
    fn push_opacity(&mut self, opacity: f32, bounds: Option<Rect>);

    /// Pushes a backdrop filter layer.
    fn push_backdrop_filter(&mut self, filter: &ImageFilter, bounds: Rect);

    /// Pushes a shader mask layer.
    fn push_shader_mask(&mut self, shader: &dyn PaintShader, bounds: Rect, blend_mode: BlendMode);

    /// Pops the most recent push operation (clip, transform, opacity, etc.).
    fn pop(&mut self);

    /// Returns the current layer depth.
    fn depth(&self) -> usize;
}

// ============================================================================
// EFFECTS TRAIT
// ============================================================================

/// Visual effects trait.
///
/// This trait provides methods for applying visual effects such as blur,
/// color filters, and other image processing operations.
///
/// Effects are typically applied to layers or specific regions of the canvas.
pub trait Effects: Send + Sync {
    /// Applies gaussian blur to a region.
    fn apply_blur(&mut self, sigma_x: f32, sigma_y: f32, bounds: Rect);

    /// Applies a color filter.
    fn apply_color_filter(&mut self, filter: &dyn PaintColorFilter, bounds: Rect);

    /// Applies a backdrop filter (affects content behind the bounds).
    fn apply_backdrop_filter(&mut self, filter: &ImageFilter, bounds: Rect);

    /// Applies a shader effect.
    fn apply_shader(&mut self, shader: &dyn PaintShader, bounds: Rect);

    /// Applies a drop shadow.
    fn apply_drop_shadow(&mut self, offset: Offset, blur_radius: f32, color: u32, bounds: Rect);

    /// Applies an inner shadow.
    fn apply_inner_shadow(&mut self, offset: Offset, blur_radius: f32, color: u32, bounds: Rect);
}

// ============================================================================
// CACHING TRAIT
// ============================================================================

/// Paint caching and optimization trait.
///
/// This trait provides hints and controls for the rendering system to
/// optimize painting through caching.
///
/// # Cache Boundaries
///
/// A "repaint boundary" is a point in the render tree where the system
/// can cache the painted content and reuse it when only parts of the UI
/// change. This is crucial for performance in complex UIs.
pub trait Caching: Send + Sync {
    /// Returns whether this content should be cached.
    fn should_cache(&self) -> bool;

    /// Returns a hint about how to cache this content.
    fn cache_hint(&self) -> CacheHint;

    /// Invalidates any cached content.
    fn invalidate(&mut self);

    /// Marks that repaint is needed.
    fn mark_needs_repaint(&mut self);

    /// Returns whether this is a repaint boundary.
    ///
    /// Repaint boundaries isolate subtrees for caching purposes.
    fn is_repaint_boundary(&self) -> bool;

    /// Sets whether this is a repaint boundary.
    fn set_repaint_boundary(&mut self, is_boundary: bool);

    /// Returns the cache key for this content, if any.
    fn cache_key(&self) -> Option<u64>;

    /// Returns whether cached content is still valid.
    fn is_cache_valid(&self) -> bool;
}

// ============================================================================
// SUPPORTING TYPES
// ============================================================================

/// Hint for how content should be cached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum CacheHint {
    /// No caching preference.
    #[default]
    None,
    /// Optimize for rendering speed (may use more memory).
    Speed,
    /// Optimize for memory usage (may be slower).
    Quality,
    /// Content changes frequently, minimal caching.
    Volatile,
    /// Content rarely changes, aggressive caching.
    Static,
}

/// Specifies how clipping should be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ClipBehavior {
    /// No clipping.
    #[default]
    None,
    /// Clip with hard edges (aliased).
    HardEdge,
    /// Clip with anti-aliased edges.
    AntiAlias,
    /// Clip with anti-aliased edges and save a layer for the clip.
    AntiAliasWithSaveLayer,
}

/// Blend mode for compositing operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum BlendMode {
    /// Replaces destination with zero (fully transparent).
    Clear,
    /// Replaces destination.
    Src,
    /// Preserves destination.
    Dst,
    /// Source over destination (default).
    #[default]
    SrcOver,
    /// Destination over source.
    DstOver,
    /// Source where destination exists.
    SrcIn,
    /// Destination where source exists.
    DstIn,
    /// Source where destination doesn't exist.
    SrcOut,
    /// Destination where source doesn't exist.
    DstOut,
    /// Source atop destination.
    SrcAtop,
    /// Destination atop source.
    DstAtop,
    /// XOR of source and destination.
    Xor,
    /// Sum of source and destination.
    Plus,
    /// Modulate (multiply).
    Modulate,
    /// Screen blend mode.
    Screen,
    /// Overlay blend mode.
    Overlay,
    /// Darken blend mode.
    Darken,
    /// Lighten blend mode.
    Lighten,
    /// Color dodge.
    ColorDodge,
    /// Color burn.
    ColorBurn,
    /// Hard light.
    HardLight,
    /// Soft light.
    SoftLight,
    /// Difference.
    Difference,
    /// Exclusion.
    Exclusion,
    /// Multiply.
    Multiply,
    /// Hue.
    Hue,
    /// Saturation.
    Saturation,
    /// Color.
    Color,
    /// Luminosity.
    Luminosity,
}

// ============================================================================
// ABSTRACT RESOURCE TRAITS
// ============================================================================

/// Abstract image resource for painting.
///
/// This trait abstracts over different image implementations
/// (GPU textures, CPU bitmaps, etc.).
pub trait PaintImage: Send + Sync {
    /// Returns the width in pixels.
    fn width(&self) -> u32;

    /// Returns the height in pixels.
    fn height(&self) -> u32;

    /// Returns the size as a tuple.
    fn size(&self) -> (u32, u32) {
        (self.width(), self.height())
    }
}

/// Abstract paragraph (laid out text) for painting.
///
/// This trait abstracts over different text layout implementations.
pub trait PaintParagraph: Send + Sync {
    /// Returns the width of the paragraph.
    fn width(&self) -> f32;

    /// Returns the height of the paragraph.
    fn height(&self) -> f32;

    /// Returns the minimum intrinsic width.
    fn min_intrinsic_width(&self) -> f32;

    /// Returns the maximum intrinsic width.
    fn max_intrinsic_width(&self) -> f32;

    /// Returns the alphabetic baseline.
    fn alphabetic_baseline(&self) -> f32;

    /// Returns the ideographic baseline.
    fn ideographic_baseline(&self) -> f32;
}

/// Abstract shader for painting.
pub trait PaintShader: Send + Sync {
    /// Returns the shader type name for debugging.
    fn shader_type(&self) -> &'static str;
}

/// Abstract color filter for painting.
pub trait PaintColorFilter: Send + Sync {
    /// Returns the filter type name for debugging.
    fn filter_type(&self) -> &'static str;
}

// ============================================================================
// PRELUDE
// ============================================================================

/// Prelude for painting traits.
pub mod prelude {
    pub use super::{
        BlendMode, CacheHint, Caching, ClipBehavior, Effects, Layering, PaintColorFilter,
        PaintImage, PaintParagraph, PaintShader, Painter,
    };
}
