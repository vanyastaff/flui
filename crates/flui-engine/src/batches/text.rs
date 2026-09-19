//! Paragraph recording: shaped glyphs → atlas slots → glyph instances.
//!
//! A paragraph reaches the batcher already shaped ([`TextLayout`], ADR-0065);
//! recording it is placing each glyph in device pixels, fetching (or
//! rasterising) its bitmap through the [`GlyphAtlas`], and pushing one quad
//! per inked glyph into the segment's glyph batch — with the paint state
//! every other batch gets: the layer opacity, the active scissor run, and
//! the SDF clip, so a rounded clip rounds text exactly as it rounds the
//! rect behind it.

use flui_painting::TextLayout;
use flui_types::{
    geometry::{Pixels, Point},
    styling::Color,
};

use super::DrawBatcher;
use crate::{
    command_ir::{DrawItem, DrawSegment, Phase},
    glyph_atlas::GlyphAtlas,
    instancing::GlyphInstance,
    state_stack::GpuStateStack,
};

impl DrawBatcher {
    /// Records `layout` with its top-left at `position` (local pixels) under
    /// the painter's current state.
    ///
    /// Glyphs are rasterised at `font_size × max_scale` and cached under
    /// it, so a 2× display gets a 2× raster. Under a uniform CTM (the
    /// device-pixel ratio, a translation) the paragraph's origin goes
    /// through the CTM and each glyph is snapped to the device grid there —
    /// hinted, pixel-exact. Under a rotated or anisotropic CTM the glyphs
    /// are placed in the paragraph's raster space and each quad carries the
    /// CTM's linear part divided by the raster scale, so the bitmap is
    /// resampled into the transformed shape rather than drawn upright at the
    /// larger axis's size (`anisotropic_scale_squashes_glyphs_on_one_axis`).
    ///
    /// `color` paints every glyph the layout did not colour itself; a span
    /// colour on the glyph wins. Both are scaled by `opacity`.
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
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        atlas: &mut GlyphAtlas,
        opacity: f32,
        layout: &TextLayout,
        position: Point<Pixels>,
        color: Color,
    ) {
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
                origin: [origin.x.0, origin.y.0],
            },
        };
        let raster_origin = match placement {
            Placement::Uniform => (origin.x.0, origin.y.0),
            Placement::Affine { .. } => (0.0, 0.0),
        };
        let mut began = false;

        for glyph in layout.placed_glyphs(raster_origin, scale) {
            let Some(slot) = atlas.slot(glyph.key) else {
                continue;
            };
            if slot.is_empty() {
                continue;
            }
            let x = glyph.x + slot.left;
            let y = glyph.y - slot.top;
            let (w, h) = (slot.size[0] as i32, slot.size[1] as i32);
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

            if !began {
                // One seal decision per paragraph: every glyph of it lands in
                // the same segment, in record order.
                Self::begin_phase(segment, draw_order, Phase::Glyph);
                began = true;
            }
            let _ = segment.glyph_batch.add(instance);
            DrawSegment::push_scissor_region(&mut segment.glyph_scissors, scissor);
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
fn outside_scissor(scissor: Option<(u32, u32, u32, u32)>, x: i32, y: i32, w: i32, h: i32) -> bool {
    let Some((sx, sy, sw, sh)) = scissor else {
        return false;
    };
    let (sx, sy) = (sx as i64, sy as i64);
    let (sr, sb) = (sx + sw as i64, sy + sh as i64);
    let (x, y) = (x as i64, y as i64);
    x + w as i64 <= sx || y + h as i64 <= sy || x >= sr || y >= sb
}

#[cfg(test)]
mod tests {
    use super::outside_scissor;

    #[test]
    fn a_quad_touching_the_scissor_edge_is_inside_and_one_past_it_is_outside() {
        let scissor = Some((10, 10, 20, 20));
        assert!(
            !outside_scissor(scissor, 29, 29, 5, 5),
            "overlaps the corner pixel"
        );
        assert!(
            outside_scissor(scissor, 30, 10, 5, 5),
            "starts on the right edge"
        );
        assert!(
            outside_scissor(scissor, 0, 0, 10, 10),
            "ends on the top-left edge"
        );
        assert!(
            !outside_scissor(None, -100, -100, 1, 1),
            "no scissor: nothing is outside"
        );
    }
}
