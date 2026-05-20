//! `DrawCommand` -- the closed-enum vocabulary of paint operations
//! that `flui-engine`'s wgpu backend pattern-matches for GPU lowering.
//!
//! Mythos chain U5 extracted these from the 2,434-LOC
//! `display_list.rs` god module. The 29 variants are the entire
//! compositor draw operation vocabulary; adding a 30th is a
//! coordinated change in `flui-painting` + `flui-engine`
//! (+ optionally `flui-rendering`).
//!
//! Deliberately the same shape as `flui-layer::Layer` enum
//! (see flui-layer Mythos chain Mapping decisions #1). The reason
//! is identical: arbitrary trait-object commands would force a
//! `Box<dyn Drawable>` boundary the wgpu backend cannot translate.

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, Rect, Size},
    painting::{Image, Path},
    styling::Color,
    typography::{InlineSpan, TextStyle},
};

use super::{ColorFilter, ImageRepeat};
use crate::display_list::{
    BlendMode, Clip, ClipOp, DisplayList, FilterQuality, ImageFilter, Paint, PointMode, Shader,
    TextureId,
};

/// A single drawing command recorded by Canvas.
///
/// Each variant contains all information needed to execute the
/// command later, including the transform matrix at the time of
/// recording.
///
/// # Transform Field
///
/// Every command stores the active `Matrix4` transform when it was
/// recorded. The GPU backend applies this transform when executing
/// the command.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DrawCommand {
    // === Clipping Commands ===
    /// Clip to a rectangle.
    ClipRect {
        /// Rectangle to clip to.
        rect: Rect<Pixels>,
        /// Set operation (Intersect or Difference).
        clip_op: ClipOp,
        /// Anti-aliasing behavior.
        clip_behavior: Clip,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Clip to a rounded rectangle.
    ClipRRect {
        /// Rounded rectangle to clip to.
        rrect: RRect,
        /// Set operation (Intersect or Difference).
        clip_op: ClipOp,
        /// Anti-aliasing behavior.
        clip_behavior: Clip,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Clip to an arbitrary path.
    ClipPath {
        /// Path to clip to.
        path: Path,
        /// Set operation (Intersect or Difference).
        clip_op: ClipOp,
        /// Anti-aliasing behavior.
        clip_behavior: Clip,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Primitive Drawing Commands ===
    /// Draw a line.
    DrawLine {
        /// Start point.
        p1: Point<Pixels>,
        /// End point.
        p2: Point<Pixels>,
        /// Paint style (color, stroke width, etc.).
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw a rectangle.
    DrawRect {
        /// Rectangle to draw.
        rect: Rect<Pixels>,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw a rounded rectangle.
    DrawRRect {
        /// Rounded rectangle to draw.
        rrect: RRect,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw a circle.
    DrawCircle {
        /// Center point.
        center: Point<Pixels>,
        /// Radius.
        radius: Pixels,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw an oval (ellipse).
    DrawOval {
        /// Bounding rectangle.
        rect: Rect<Pixels>,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw an arbitrary path.
    DrawPath {
        /// Path to draw.
        path: Path,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Text ===
    /// Draw text.
    DrawText {
        /// Text content.
        text: String,
        /// Position offset.
        offset: Offset<Pixels>,
        /// Pre-computed size of the text (for bounds calculation).
        size: Size<Pixels>,
        /// Text style (font, size, etc.).
        style: TextStyle,
        /// Paint style (color, etc.).
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw rich text with inline spans.
    DrawTextSpan {
        /// Rich text span (with nested styles).
        span: InlineSpan,
        /// Position offset.
        offset: Offset<Pixels>,
        /// Text scale factor for accessibility.
        text_scale_factor: f64,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Image ===
    /// Draw an image.
    DrawImage {
        /// Image.
        image: Image,
        /// Destination rectangle.
        dst: Rect<Pixels>,
        /// Optional paint (for tinting, etc.).
        paint: Option<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw an image with repeat (tiling).
    DrawImageRepeat {
        /// Image to tile.
        image: Image,
        /// Destination rectangle to fill.
        dst: Rect<Pixels>,
        /// How to repeat the image.
        repeat: ImageRepeat,
        /// Optional paint (for tinting, opacity, etc.).
        paint: Option<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw an image with 9-slice/9-patch scaling.
    DrawImageNineSlice {
        /// Image to draw.
        image: Image,
        /// Center slice rectangle within the image (in image coords).
        center_slice: Rect<Pixels>,
        /// Destination rectangle.
        dst: Rect<Pixels>,
        /// Optional paint (for tinting, opacity, etc.).
        paint: Option<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw an image with a color filter.
    DrawImageFiltered {
        /// Image to draw.
        image: Image,
        /// Destination rectangle.
        dst: Rect<Pixels>,
        /// Color filter to apply.
        filter: ColorFilter,
        /// Optional paint (for additional effects).
        paint: Option<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Texture ===
    /// Draw a GPU texture referenced by ID.
    DrawTexture {
        /// GPU texture identifier.
        texture_id: TextureId,
        /// Destination rectangle.
        dst: Rect<Pixels>,
        /// Source rectangle within the texture (None = entire texture).
        src: Option<Rect<Pixels>>,
        /// Filter quality for texture sampling.
        filter_quality: FilterQuality,
        /// Opacity (0.0 = transparent, 1.0 = opaque).
        opacity: f32,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Effects ===
    /// Draw a shadow.
    DrawShadow {
        /// Path casting shadow.
        path: Path,
        /// Shadow color.
        color: Color,
        /// Elevation (blur amount).
        elevation: f32,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Gradient Drawing Commands ===
    /// Draw a gradient-filled rectangle.
    DrawGradient {
        /// Rectangle to fill.
        rect: Rect<Pixels>,
        /// Gradient shader.
        shader: Shader,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw a gradient-filled rounded rectangle.
    DrawGradientRRect {
        /// Rounded rectangle to fill.
        rrect: RRect,
        /// Gradient shader.
        shader: Shader,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Apply a shader as a mask to child content.
    ShaderMask {
        /// Child content to be masked (recorded commands).
        child: Box<DisplayList>,
        /// Shader specification (gradient type, colors, etc.).
        shader: Shader,
        /// Bounds of the masked region.
        bounds: Rect<Pixels>,
        /// Blend mode for final compositing.
        blend_mode: BlendMode,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Backdrop filter effect (frosted glass, blur).
    BackdropFilter {
        /// Child content to render on top of filtered backdrop.
        child: Option<Box<DisplayList>>,
        /// Image filter to apply (blur, color adjustments, etc.).
        filter: ImageFilter,
        /// Bounds for backdrop capture.
        bounds: Rect<Pixels>,
        /// Blend mode for final compositing.
        blend_mode: BlendMode,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Advanced Primitives ===
    /// Draw an arc segment.
    DrawArc {
        /// Bounding rectangle for the ellipse.
        rect: Rect<Pixels>,
        /// Start angle in radians.
        start_angle: f32,
        /// Sweep angle in radians.
        sweep_angle: f32,
        /// Whether to draw from center (pie slice) or just the arc.
        use_center: bool,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw difference between two rounded rectangles (ring/border).
    DrawDRRect {
        /// Outer rounded rectangle.
        outer: RRect,
        /// Inner rounded rectangle.
        inner: RRect,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw a sequence of points.
    DrawPoints {
        /// Point drawing mode.
        mode: PointMode,
        /// Points to draw.
        points: Vec<Point<Pixels>>,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw custom vertices with optional colors and texture
    /// coordinates.
    DrawVertices {
        /// Vertex positions.
        vertices: Vec<Point<Pixels>>,
        /// Optional vertex colors (must match vertices length).
        colors: Option<Vec<Color>>,
        /// Optional texture coordinates (must match vertices length).
        tex_coords: Option<Vec<Point<Pixels>>>,
        /// Triangle indices (groups of 3).
        indices: Vec<u16>,
        /// Paint style.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Fill entire canvas with a color (respects clipping).
    DrawColor {
        /// Color to fill with.
        color: Color,
        /// Blend mode.
        blend_mode: BlendMode,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Fill entire canvas with a Paint (color, shader, blend mode).
    DrawPaint {
        /// Paint to fill with (color, shader, blend mode, etc.).
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw multiple sprites from a texture atlas.
    DrawAtlas {
        /// Source image (atlas texture).
        image: Image,
        /// Source rectangles in atlas (sprite locations).
        sprites: Vec<Rect<Pixels>>,
        /// Destination transforms for each sprite.
        transforms: Vec<Matrix4>,
        /// Optional colors to blend with each sprite.
        colors: Option<Vec<Color>>,
        /// Blend mode.
        blend_mode: BlendMode,
        /// Optional paint for additional effects.
        paint: Option<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    // === Layer Commands ===
    /// Save the current canvas state and create a new compositing
    /// layer.
    SaveLayer {
        /// Bounds of the layer (None = unbounded).
        bounds: Option<Rect<Pixels>>,
        /// Paint to apply when compositing the layer.
        paint: Paint,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Restore the canvas state and composite the saved layer.
    RestoreLayer {
        /// Transform at recording time (for consistency).
        transform: Matrix4,
    },
}

/// Categories of drawing commands.
///
/// Used by `DrawCommand::kind()` for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandKind {
    /// Drawing commands (shapes, text, images).
    Draw,
    /// Clipping commands.
    Clip,
    /// Effect commands (shader mask, backdrop filter).
    Effect,
    /// Layer commands (save/restore layer).
    Layer,
}
