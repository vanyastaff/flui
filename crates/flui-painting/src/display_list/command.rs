//! `DrawCommand` — one recorded paint operation: the absolute transform it
//! was recorded under and the [`DrawOp`] itself, the closed vocabulary
//! `flui-engine` matches exhaustively when lowering to the GPU. Adding a variant is a
//! coordinated change in both crates: the engine's match has no wildcard,
//! so a new variant is a compile error there until its `render_*` arm
//! exists. Same shape and reason as `flui_layer::Layer`: a trait-object
//! command would be a `Box<dyn Drawable>` the backend cannot translate.

use std::sync::Arc;

use flui_types::{
    geometry::{Matrix4, Offset, Pixels, Point, RRect, RSuperellipse, Rect},
    painting::{Image, Path},
    styling::Color,
};

use super::{ColorFilter, ImageRepeat};
use crate::display_list::{BlendMode, Clip, ClipOp, FilterQuality, Paint, PointMode, TextureId};
use crate::text_layout::TextLayout;

/// One recorded paint operation: the canvas transform at recording time
/// and the operation itself.
///
/// The transform is absolute (the full CTM), so every command is
/// self-describing — its bounds, a damage query, a snapshot line, and a
/// re-stamp under another transform (`Canvas::draw_picture`) all read one
/// command without replaying state. Clips are the one thing that scope:
/// [`DrawOp::Save`]/[`DrawOp::Restore`] bracket them.
#[derive(Debug, Clone)]
pub struct DrawCommand {
    /// The canvas transform at recording time.
    pub transform: Matrix4,
    /// The operation.
    pub op: DrawOp,
}

impl DrawCommand {
    /// A command recorded under the identity transform.
    #[must_use]
    pub const fn untransformed(op: DrawOp) -> Self {
        Self {
            transform: Matrix4::IDENTITY,
            op,
        }
    }
}

/// The closed vocabulary of paint operations.
#[derive(Debug, Clone)]
pub enum DrawOp {
    // === Clipping Commands ===
    /// Clip to a rectangle.
    ClipRect {
        /// Rectangle to clip to.
        rect: Rect<Pixels>,
        /// Set operation (Intersect or Difference).
        clip_op: ClipOp,
        /// Anti-aliasing behavior.
        clip_behavior: Clip,
    },

    /// Clip to a rounded rectangle.
    ClipRRect {
        /// Rounded rectangle to clip to.
        rrect: RRect,
        /// Set operation (Intersect or Difference).
        clip_op: ClipOp,
        /// Anti-aliasing behavior.
        clip_behavior: Clip,
    },

    /// Clip to a rounded superellipse (Flutter `RSuperellipse`).
    ///
    /// Same shape carrier as [`Self::ClipRRect`]; the *intent* is the
    /// rounded-superellipse (iOS-squircle) corner curve, which has a
    /// smoother falloff than the elliptical arcs used by `RRect`. Exact
    /// rendering is backend-dependent: the `CommandRenderer::clip_rsuperellipse`
    /// default falls back to an `RRect` approximation built from the
    /// superellipse's outer rect plus per-corner radii, and a backend may
    /// override with a real superellipse SDF for pixel-perfect parity, which
    /// the wgpu backend does. Note the direction of that approximation: the
    /// rrect built from the same outer rect and radii is INSCRIBED in the
    /// squircle, so it clips more, not less.
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
    },

    /// Clip to an arbitrary path.
    ClipPath {
        /// Path to clip to.
        path: Path,
        /// Set operation (Intersect or Difference).
        clip_op: ClipOp,
        /// Anti-aliasing behavior.
        clip_behavior: Clip,
    },

    // === Primitive Drawing Commands ===
    /// Draw a line.
    Line {
        /// Start point.
        p1: Point<Pixels>,
        /// End point.
        p2: Point<Pixels>,
        /// Paint style (color, stroke width, etc.).
        paint: Arc<Paint>,
    },

    /// Draw a rectangle.
    Rect {
        /// Rectangle to draw.
        rect: Rect<Pixels>,
        /// Paint style.
        paint: Arc<Paint>,
    },

    /// Draw a rounded rectangle.
    RRect {
        /// Rounded rectangle to draw.
        rrect: RRect,
        /// Paint style.
        paint: Arc<Paint>,
    },

    /// Draw a circle.
    Circle {
        /// Center point.
        center: Point<Pixels>,
        /// Radius.
        radius: Pixels,
        /// Paint style.
        paint: Arc<Paint>,
    },

    /// Draw an oval (ellipse).
    Oval {
        /// Bounding rectangle.
        rect: Rect<Pixels>,
        /// Paint style.
        paint: Arc<Paint>,
    },

    /// Draw an arbitrary path.
    Path {
        /// Path to draw.
        path: Path,
        /// Paint style.
        paint: Arc<Paint>,
    },

    // === Text ===
    /// Draw a shaped paragraph.
    ///
    /// The layout is what the recorder measured — `TextPainter::paint` hands
    /// over the very `Arc` its cache holds — so what the engine rasterises
    /// is, by identity, what was laid out: line breaks, `max_lines`, the
    /// ellipsis, per-span faces. `color` is the paragraph's root colour; a
    /// span whose colour differs carries its own in the layout.
    Paragraph {
        /// The shaped text.
        layout: Arc<TextLayout>,
        /// Top-left of the paragraph's box.
        offset: Offset<Pixels>,
        /// The colour of every glyph that has no span colour of its own.
        color: Color,
    },

    // === Image ===
    /// Draw an image.
    Image {
        /// Image.
        image: Image,
        /// Destination rectangle.
        dst: Rect<Pixels>,
        /// Optional paint (for tinting, etc.).
        paint: Option<Arc<Paint>>,
    },

    /// Draw an image with repeat (tiling).
    ImageRepeat {
        /// Image to tile.
        image: Image,
        /// Destination rectangle to fill.
        dst: Rect<Pixels>,
        /// How to repeat the image.
        repeat: ImageRepeat,
        /// Optional paint (for tinting, opacity, etc.).
        paint: Option<Arc<Paint>>,
    },

    /// Draw an image with 9-slice/9-patch scaling.
    ImageNineSlice {
        /// Image to draw.
        image: Image,
        /// Center slice rectangle within the image (in image coords).
        center_slice: Rect<Pixels>,
        /// Destination rectangle.
        dst: Rect<Pixels>,
        /// Optional paint (for tinting, opacity, etc.).
        paint: Option<Arc<Paint>>,
    },

    /// Draw an image with a color filter.
    ImageFiltered {
        /// Image to draw.
        image: Image,
        /// Destination rectangle.
        dst: Rect<Pixels>,
        /// Color filter to apply.
        filter: ColorFilter,
        /// Optional paint (for additional effects).
        paint: Option<Arc<Paint>>,
    },

    // === Texture ===
    /// Draw a GPU texture referenced by ID.
    Texture {
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
    },

    // === Effects ===
    /// Draw a shadow.
    Shadow {
        /// Path casting shadow.
        path: Path,
        /// Shadow color.
        color: Color,
        /// Elevation (blur amount).
        elevation: f32,
    },

    // === Gradient Drawing Commands ===

    // === Advanced Primitives ===
    /// Draw an arc segment.
    Arc {
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
    },

    /// Draw difference between two rounded rectangles (ring/border).
    DRRect {
        /// Outer rounded rectangle.
        outer: RRect,
        /// Inner rounded rectangle.
        inner: RRect,
        /// Paint style.
        paint: Arc<Paint>,
    },

    /// Draw a sequence of points.
    Points {
        /// Point drawing mode.
        mode: PointMode,
        /// Points to draw.
        points: Vec<Point<Pixels>>,
        /// Paint style.
        paint: Arc<Paint>,
    },

    /// Draw custom vertices with optional colors and texture
    /// coordinates.
    Vertices {
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
    },

    /// Fill entire canvas with a color (respects clipping).
    Color {
        /// Color to fill with.
        color: Color,
        /// Blend mode.
        blend_mode: BlendMode,
    },

    /// Fill entire canvas with a Paint (color, shader, blend mode).
    Paint {
        /// Paint to fill with (color, shader, blend mode, etc.).
        paint: Arc<Paint>,
    },

    /// Draw multiple sprites from a texture atlas.
    Atlas {
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
    },

    // === Layer Commands ===
    /// Save the current canvas state and create a new compositing
    /// layer.
    SaveLayer {
        /// Bounds of the layer (None = unbounded).
        bounds: Option<Rect<Pixels>>,
        /// Paint to apply when compositing the layer.
        paint: Arc<Paint>,
    },

    /// Restore the canvas state and composite the saved layer.
    RestoreLayer,

    /// Push the backend's transform + clip state.
    ///
    /// The counterpart to [`DrawOp::Restore`], and the plain-state
    /// sibling of [`DrawOp::SaveLayer`] — no offscreen target, no
    /// compositing, just a scope marker.
    ///
    /// It exists because a clip has to be *undoable*. `ClipRect`/`ClipRRect`
    /// and friends narrow the backend's current clip, and without a marker
    /// saying when that narrowing ends, a clip applied for one subtree keeps
    /// applying to every command recorded after it — including siblings that
    /// never asked to be clipped.
    Save,

    /// Pop the transform + clip state pushed by the matching
    /// [`DrawOp::Save`].
    ///
    /// Emitted by `Canvas::restore` for a plain `save()`. A `save_layer()` is
    /// closed by [`DrawOp::RestoreLayer`] instead — that path composites
    /// an offscreen target and manages its own state — so exactly one of the
    /// two is recorded per balanced pair, never both.
    Restore,
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::{DrawCommand, DrawOp};

    /// The wire type has a size budget: every recorded command is one of
    /// these in a `Vec`, and every consumer walks that `Vec` once per frame.
    ///
    /// `DrawOp` is two cache lines; the fattest variant is `ImageFiltered`,
    /// whose inline `ColorFilter::Matrix` is a 5×4 `f32` matrix (80 bytes).
    /// A new variant or field that pushes past this budget boxes its payload
    /// instead (as `Paragraph` already carries its layout behind an `Arc`).
    /// `DrawCommand` adds the 64-byte `Matrix4`.
    #[test]
    fn draw_command_fits_its_budget() {
        assert!(
            size_of::<DrawOp>() <= 128,
            "DrawOp is {} bytes; the budget is 128",
            size_of::<DrawOp>()
        );
        assert!(
            size_of::<DrawCommand>() <= 192,
            "DrawCommand is {} bytes; the budget is 192",
            size_of::<DrawCommand>()
        );
    }
}
