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

use std::sync::Arc;

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, RSuperellipse, Rect, Size},
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
#[non_exhaustive]
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

    /// Clip to a rounded superellipse (Flutter `RSuperellipse`).
    ///
    /// Same shape carrier as [`Self::ClipRRect`]; the *intent* is the
    /// rounded-superellipse (iOS-squircle) corner curve, which has a
    /// smoother falloff than the elliptical arcs used by `RRect`. Exact
    /// rendering is backend-dependent: the `CommandRenderer::clip_rsuperellipse`
    /// default falls back to an `RRect` approximation built from the
    /// superellipse's outer rect plus per-corner radii, and a backend may
    /// override with a real superellipse SDF for pixel-perfect parity (see
    /// `flui-engine::wgpu::layer_render::get_or_generate_superellipse_path`
    /// for the path-tessellation route used by `ClipSuperellipseLayer`).
    /// Matches Flutter's `Canvas.clipRSuperellipse` and
    /// `ClipContext.clipRSuperellipseAndPaint` at the command-vocabulary
    /// level.
    ClipRSuperellipse {
        /// Rounded superellipse to clip to.
        rsuperellipse: RSuperellipse,
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
        paint: Arc<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw a rectangle.
    DrawRect {
        /// Rectangle to draw.
        rect: Rect<Pixels>,
        /// Paint style.
        paint: Arc<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw a rounded rectangle.
    DrawRRect {
        /// Rounded rectangle to draw.
        rrect: RRect,
        /// Paint style.
        paint: Arc<Paint>,
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
        paint: Arc<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw an oval (ellipse).
    DrawOval {
        /// Bounding rectangle.
        rect: Rect<Pixels>,
        /// Paint style.
        paint: Arc<Paint>,
        /// Transform at recording time.
        transform: Matrix4,
    },

    /// Draw an arbitrary path.
    DrawPath {
        /// Path to draw.
        path: Path,
        /// Paint style.
        paint: Arc<Paint>,
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
        paint: Arc<Paint>,
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
        /// Wrap width for line breaking. `None` = unbounded (no wrapping).
        /// Passed to the GPU text renderer so glyphon respects the same
        /// line-breaking constraints as the cosmic-text layout cache.
        wrap_width: Option<f32>,
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
        paint: Option<Arc<Paint>>,
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
        paint: Option<Arc<Paint>>,
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
        paint: Option<Arc<Paint>>,
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
        paint: Option<Arc<Paint>>,
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
        paint: Arc<Paint>,
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
        paint: Arc<Paint>,
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
        paint: Arc<Paint>,
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
        paint: Arc<Paint>,
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
        paint: Arc<Paint>,
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
        paint: Option<Arc<Paint>>,
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
        paint: Arc<Paint>,
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

// ============================================================================
// FROZEN CONTRACT GUARD (Core.0 N11)
// ============================================================================
//
// `DrawCommand` is the frozen wire contract between the producer
// (`flui-painting` `Canvas`/`DisplayList`) and the consumer (`flui-engine`
// `CommandRenderer` + `dispatch_command`). It is `#[non_exhaustive]` so
// downstream crates absorb additive change gracefully — but ADDING, REMOVING,
// or RENAMING a variant is a coordinated cross-track change, not a local edit.
//
// The exhaustive match below is the tripwire. `#[non_exhaustive]` is a no-op
// inside this defining crate, so the compiler enforces exhaustiveness here:
//   - add a variant  -> this match fails to compile ("non-exhaustive patterns")
//   - remove/rename   -> this match fails to compile ("no variant named …")
// Either way the contract change cannot land silently.
//
// CHANGE PROTOCOL (see docs/designs/2026-06-30-scene-drawcommand-contract.md):
//   1. Update the design doc + bump its contract version + add a changelog line.
//   2. Add/rename the arm here and update FROZEN_DRAWCOMMAND_VARIANT_COUNT.
//   3. Add the matching `render_*` method in flui-engine `traits.rs`
//      (`CommandRenderer`) and a dispatch arm in `flui-engine/src/commands.rs`.
//   4. Re-run the full gate so any backend missing the new arm fails loudly.
#[cfg(test)]
mod contract_freeze {
    use super::DrawCommand;

    /// Frozen count of `DrawCommand` variants. Bump only via the change
    /// protocol in the module comment above.
    const FROZEN_DRAWCOMMAND_VARIANT_COUNT: usize = 31;

    /// Exhaustive — NO wildcard arm. This is the compile-time freeze guard:
    /// the function only exists to force the exhaustiveness check; the
    /// returned discriminant name is a convenience for the count assertion.
    fn contract_discriminant(cmd: &DrawCommand) -> &'static str {
        match cmd {
            DrawCommand::ClipRect { .. } => "ClipRect",
            DrawCommand::ClipRRect { .. } => "ClipRRect",
            DrawCommand::ClipRSuperellipse { .. } => "ClipRSuperellipse",
            DrawCommand::ClipPath { .. } => "ClipPath",
            DrawCommand::DrawLine { .. } => "DrawLine",
            DrawCommand::DrawRect { .. } => "DrawRect",
            DrawCommand::DrawRRect { .. } => "DrawRRect",
            DrawCommand::DrawCircle { .. } => "DrawCircle",
            DrawCommand::DrawOval { .. } => "DrawOval",
            DrawCommand::DrawPath { .. } => "DrawPath",
            DrawCommand::DrawText { .. } => "DrawText",
            DrawCommand::DrawTextSpan { .. } => "DrawTextSpan",
            DrawCommand::DrawImage { .. } => "DrawImage",
            DrawCommand::DrawImageRepeat { .. } => "DrawImageRepeat",
            DrawCommand::DrawImageNineSlice { .. } => "DrawImageNineSlice",
            DrawCommand::DrawImageFiltered { .. } => "DrawImageFiltered",
            DrawCommand::DrawTexture { .. } => "DrawTexture",
            DrawCommand::DrawShadow { .. } => "DrawShadow",
            DrawCommand::DrawGradient { .. } => "DrawGradient",
            DrawCommand::DrawGradientRRect { .. } => "DrawGradientRRect",
            DrawCommand::ShaderMask { .. } => "ShaderMask",
            DrawCommand::BackdropFilter { .. } => "BackdropFilter",
            DrawCommand::DrawArc { .. } => "DrawArc",
            DrawCommand::DrawDRRect { .. } => "DrawDRRect",
            DrawCommand::DrawPoints { .. } => "DrawPoints",
            DrawCommand::DrawVertices { .. } => "DrawVertices",
            DrawCommand::DrawColor { .. } => "DrawColor",
            DrawCommand::DrawPaint { .. } => "DrawPaint",
            DrawCommand::DrawAtlas { .. } => "DrawAtlas",
            DrawCommand::SaveLayer { .. } => "SaveLayer",
            DrawCommand::RestoreLayer { .. } => "RestoreLayer",
        }
    }

    #[test]
    fn drawcommand_contract_is_frozen() {
        // The exhaustive match in `contract_discriminant` is the real guard
        // (it fails to compile if the variant set changes). This assertion
        // pins the count as a second, human-readable signal and keeps the
        // helper from being dead code.
        assert_eq!(
            FROZEN_DRAWCOMMAND_VARIANT_COUNT, 31,
            "DrawCommand contract count changed — follow the change protocol in \
             the module comment + docs/designs/2026-06-30-scene-drawcommand-contract.md"
        );
        // Touch the guard so it is exercised, not merely compiled.
        let _ = contract_discriminant as fn(&DrawCommand) -> &'static str;
    }
}
