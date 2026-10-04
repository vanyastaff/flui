//! Paragraph recording: shaped glyphs → atlas slots → glyph instances.
//!
//! A paragraph reaches the batcher already shaped ([`ShapedParagraph`],
//! ADR-0065, ADR-0092 §4); recording it is handing each run's face to the
//! atlas's rasterizer, placing each glyph in device pixels, fetching (or
//! rasterising) its bitmap through the [`TextAtlas`], and pushing one quad
//! per inked glyph into the segment's glyph batch — with the paint state
//! every other batch gets: the layer opacity, the active scissor run, and
//! the SDF clip, so a rounded clip rounds text exactly as it rounds the
//! rect behind it.

use flui_foundation::geometry::Point;
use flui_painting::ShapedParagraph;
use flui_painting::styling::Color;

use super::DrawBatcher;
use crate::{
    command_ir::{DrawItem, DrawRun, DrawSegment},
    glyph_atlas::TextAtlas,
    instancing::GlyphInstance,
    state_stack::GpuStateStack,
};

impl DrawBatcher {
    /// Records `paragraph` with its top-left at `position` (local pixels)
    /// under the painter's current state.
    ///
    /// Glyphs are rasterised at `font_size × max_scale` and cached under
    /// it, so a 2× display gets a 2× raster. Under a uniform CTM (the
    /// device-pixel ratio, a translation) the paragraph's origin goes
    /// through the CTM and each glyph is snapped to the device grid there —
    /// hinted, pixel-exact. Under a rotated or anisotropic CTM the glyphs
    /// are placed in the paragraph's raster space and each quad carries the
    /// CTM's linear part divided by the raster scale, so the bitmap is
    /// resampled into the transformed shape rather than drawn upright at the
    /// larger axis's size.
    ///
    /// `color` paints every glyph whose run carries no span colour; a span
    /// colour wins. Both are scaled by `opacity`.
    ///
    /// A run whose face the rasterizer cannot register (its blob holds no
    /// face at the run's index) is not drawn; the rest of the paragraph is.
    ///
    /// Glyphs whose quad lies entirely outside the active scissor are not
    /// recorded; a fully clipped paragraph costs its placement walk and
    /// nothing on the GPU.
    #[expect(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state/atlas are disjoint WgpuPainter fields"
    )]
    pub(in super::super) fn draw_paragraph(
        segment: &mut DrawSegment,
        _draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        atlas: &mut TextAtlas,
        opacity: f32,
        paragraph: &ShapedParagraph,
        position: Point<f64>,
        color: Color,
    ) {
        if segment.recording_result().is_err() {
            return;
        }

        let scale = state.max_scale();
        let scissor = state.current_scissor();
        let origin = state.apply_transform(position);
        // Uniform: the linear part is `scale × I` (up to float noise), so the
        // raster space IS device space and glyphs snap to the device grid.
        // Otherwise the quads carry `linear / scale` and the paragraph's
        // device origin, and the raster-space origin is zero.
        let placement = match uniform_linear(state, scale) {
            None => Placement::Uniform,
            Some(linear) => Placement::Affine {
                linear,
                origin: [(origin.x as f32), (origin.y as f32)],
            },
        };
        let raster_origin = match placement {
            Placement::Uniform => (origin.x, origin.y),
            Placement::Affine { .. } => (0.0, 0.0),
        };
        let raster_origin = (raster_origin.0 as f32, raster_origin.1 as f32);

        for run in paragraph.runs() {
            // The face first, in a statement of its own: the placement below
            // borrows the atlas for each glyph's slot.
            let key = match atlas.rasterizer_mut().fonts_mut().prepare_run(&run) {
                Ok(key) => key,
                Err(error) => {
                    tracing::warn!(?error, face = ?run.face().key(), "a run's face is not drawn");
                    continue;
                }
            };
            for glyph in run.placed_glyphs(key, raster_origin, scale) {
                let Some(slot) = atlas.slot(glyph.key) else {
                    continue;
                };
                if slot.is_empty() {
                    continue;
                }
                // A representable placed origin can have bitmap bearings
                // outside i32. Keep the quad wide until scissor exclusion.
                let x = i64::from(glyph.x) + i64::from(slot.left);
                let y = i64::from(glyph.y) - i64::from(slot.top);
                let (w, h) = (i64::from(slot.size[0]), i64::from(slot.size[1]));
                if matches!(placement, Placement::Uniform) && outside_scissor(scissor, x, y, w, h) {
                    continue;
                }

                let mut fill = glyph.color.unwrap_or(color);
                if opacity < 1.0 {
                    fill = Color::rgba(fill.r, fill.g, fill.b, (f32::from(fill.a) * opacity) as u8);
                }
                let mut instance = GlyphInstance::new(
                    [x as f32, y as f32, w as f32, h as f32],
                    slot.texel,
                    slot.color_page,
                    fill,
                );
                if let Placement::Affine { linear, origin } = placement {
                    instance = instance.with_affine(linear, origin);
                }
                let instance = state.apply_active_clip(instance);

                let _ = segment.glyph_batch.add(instance);
                segment.record_run(
                    DrawRun::Glyph(
                        segment.glyph_batch.len().saturating_sub(1)..segment.glyph_batch.len(),
                    ),
                    state.clip_chain(),
                );
                DrawSegment::push_scissor_region(&mut segment.glyph_scissors, scissor);
            }
        }
    }
}

/// How a paragraph's quads are placed: see [`DrawBatcher::draw_paragraph`].
#[derive(Clone, Copy)]
enum Placement {
    Uniform,
    Affine { linear: [f32; 4], origin: [f32; 2] },
}

/// `None` when the CTM's linear part is `scale × I` (a uniform scale, which
/// is what the device-pixel ratio and every plain translation produce);
/// otherwise the linear part divided by `scale`, column-major.
fn uniform_linear(state: &GpuStateStack, scale: f32) -> Option<[f32; 4]> {
    let m = state.current_transform();
    let linear = [m.x_axis.x, m.x_axis.y, m.y_axis.x, m.y_axis.y];
    let uniform = [scale, 0.0, 0.0, scale];
    let is_uniform = linear
        .iter()
        .zip(uniform)
        .all(|(a, b)| (a - b).abs() <= 1e-4 * scale.max(1.0));
    if is_uniform || scale <= 0.0 {
        return None;
    }
    Some(linear.map(|v| v / scale))
}

/// Whether a `w × h` quad at `(x, y)` shares no pixel with `scissor`.
fn outside_scissor(scissor: Option<(u32, u32, u32, u32)>, x: i64, y: i64, w: i64, h: i64) -> bool {
    let Some((sx, sy, sw, sh)) = scissor else {
        return false;
    };
    let (sx, sy) = (sx as i64, sy as i64);
    let (sr, sb) = (sx + sw as i64, sy + sh as i64);
    x + w <= sx || y + h <= sy || x >= sr || y >= sb
}
