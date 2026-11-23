//! Moving recorded IR by whole device pixels (ADR-0304 §9).
//!
//! A chunk's IR depends on its transform's linear part and on the fraction
//! of its translation; the whole pixels of the translation, its
//! `PixelOrigin`, only move it. A kept recording replays at another origin
//! by adding the difference, a `PixelShift`, to every device position it
//! holds. Each `Shift` impl destructures its instance whole, so a field
//! added to one has to say here whether it is a device position. Positions in
//! a shape's local space, colours, texel coordinates and the SDF clip (the
//! entry's, which does not move with the chunk) stay as recorded.
//!
//! Two layouts place an instance: a baked one, whose bounds are device
//! pixels under an identity linear part and a zero translation, and an
//! affine one, whose translation carries the device placement. An instance
//! with an identity linear part and a zero translation is moved as a baked
//! one either way: both draw it at its bounds.
//!
//! The shift is a whole number below 2^24, so it is exact in `f32`, and
//! adding it to a whole or dyadic position of that range is exact. Any other
//! position rounds once, within one `f32` ulp of what recording at the new
//! origin computes.
//!
//! A glyph on the device grid is placed in whole pixels and a subpixel bin
//! from an offset that holds no whole pixel of the translation: the
//! translation's fraction, `split_translation`, plus the linear part's image
//! of the paragraph's position (`ShapedRun::placed_glyphs`). A replay shares
//! those bits with a recording at the new origin, so its glyphs are the
//! ones that recording places, at every magnitude the key admits, as long as
//! the translation the paragraph is drawn under is the chunk's own. A
//! translation pushed inside the chunk is composed with the chunk's in `f64`
//! at the device magnitude, so its fraction can round to other bits at
//! another whole pixel, and a glyph at a rounding boundary can then land a
//! bin or a row from where a fresh recording puts it.

use crate::record::effects::{
    LinearGradientInstance, RadialGradientInstance, ShadowInstance, SweepGradientInstance,
};
use crate::record::instancing::{
    ArcInstance, CircleInstance, GlyphInstance, RectInstance, TextureInstance,
};
use crate::record::vertex::Vertex;

/// The whole device pixels of a chunk's translation, `floor(t)` on each axis.
/// [`crate::record::cache::RasterKey::of`] admits translations below 2^23,
/// so each axis fits an `f32` exactly and two of them differ by less than
/// 2^24.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PixelOrigin {
    x: i64,
    y: i64,
}

impl PixelOrigin {
    /// The origin of a translation already bounded below 2^23 pixels.
    pub(crate) fn of_bounded(tx: f64, ty: f64) -> Self {
        debug_assert!(tx.abs() < 8_388_608.0 && ty.abs() < 8_388_608.0);
        Self {
            x: split_translation(tx).0 as i64,
            y: split_translation(ty).0 as i64,
        }
    }

    /// The whole device pixels of the origin, `(x, y)`.
    #[cfg(test)]
    pub(crate) fn pixels(self) -> (i64, i64) {
        (self.x, self.y)
    }

    /// The move from `self` to `to`; `None` when they are the same, so a
    /// replay in place copies its IR bit for bit (adding `0.0` would turn
    /// `-0.0` into `0.0`).
    pub(crate) fn shift_to(self, to: Self) -> Option<PixelShift> {
        // Each axis is below 2^23 in magnitude, so the difference is below
        // 2^24 and exact as `f32`.
        let (dx, dy) = (to.x - self.x, to.y - self.y);
        (dx != 0 || dy != 0).then_some(PixelShift {
            dx: dx as f32,
            dy: dy as f32,
        })
    }
}

/// A device translation `t` as its whole pixels and the rest, `floor(t)` and
/// a fraction in `[0, 1)`: the fraction the recording cache keys a chunk by
/// and the one text is placed from, so a recording and its replay at another
/// whole pixel compute the same bits.
pub(crate) fn split_translation(t: f64) -> (f64, f64) {
    let whole = t.floor();
    // `t - floor(t)` is exact but for a `t` in (-1/2, 0), where it rounds,
    // and to 1.0 when `-t` is at most 2^-54; clamped below 1, so that `t`
    // keys apart from a whole one. A whole `t` of either sign has the
    // fraction `+0.0`.
    (whole, (t - whole).min(1.0_f64.next_down()))
}

/// A move by whole device pixels, right and down; never zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PixelShift {
    dx: f32,
    dy: f32,
}

impl PixelShift {
    fn apply(self, position: &mut [f32]) {
        position[0] += self.dx;
        position[1] += self.dy;
    }
}

/// IR that holds device positions and moves with its chunk.
pub(crate) trait Shift: Copy {
    /// `self` drawn `by` further right and down.
    #[must_use]
    fn shifted(self, by: PixelShift) -> Self;
}

/// The device rectangle `[x, y, w, h]` that a recording's glyphs on the
/// device grid cover, in whole pixels, which moves with them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridExtent(pub(crate) [f32; 4]);

impl Shift for GridExtent {
    fn shifted(mut self, by: PixelShift) -> Self {
        by.apply(&mut self.0);
        self
    }
}

const IDENTITY: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// Moves the placement of an instance in one of the two layouts: `bounds`
/// when `transform` is the identity and `translate` is zero, `translate`
/// otherwise.
fn place(bounds: &mut [f32; 4], transform: [f32; 4], translate: &mut [f32; 4], by: PixelShift) {
    if transform == IDENTITY && translate[0] == 0.0 && translate[1] == 0.0 {
        by.apply(bounds);
    } else {
        by.apply(translate);
    }
}

impl Shift for RectInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        let Self {
            bounds,
            color: _,
            corner_radii: _,
            transform,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform_translate,
        } = &mut self;
        place(bounds, *transform, transform_translate, by);
        self
    }
}

impl Shift for CircleInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // The centre is always placed by the translation.
        let Self {
            center_radius: _,
            color: _,
            transform: _,
            transform_translate,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for ArcInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // The centre is always placed by the translation.
        let Self {
            center_radius: _,
            angles: _,
            color: _,
            transform: _,
            transform_translate,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for ShadowInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // Recorded with the transform baked in: bounds and rectangle are
        // device pixels, the offset relative to them.
        let Self {
            bounds,
            rect_pos,
            rect_size: _,
            corner_radius: _,
            padding1: _,
            shadow_offset: _,
            blur_sigma: _,
            padding2: _,
            shadow_color: _,
        } = &mut self;
        by.apply(bounds);
        by.apply(rect_pos);
        self
    }
}

impl Shift for LinearGradientInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // `with_transform` keeps bounds and the gradient line local and puts
        // the device placement in the translation.
        let Self {
            bounds: _,
            linear_parameter: _,
            corner_radii: _,
            stop_count: _,
            stop_offset: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform: _,
            transform_translate,
            padding: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for RadialGradientInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        let Self {
            bounds: _,
            center: _,
            radius: _,
            tile_mode: _,
            corner_radii: _,
            stop_count: _,
            stop_offset: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform: _,
            transform_translate,
            focal: _,
            padding2: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for SweepGradientInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        let Self {
            bounds: _,
            center: _,
            angles: _,
            corner_radii: _,
            stop_count: _,
            stop_offset: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            transform: _,
            transform_translate,
            padding: _,
        } = &mut self;
        by.apply(transform_translate);
        self
    }
}

impl Shift for GlyphInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // A uniform CTM records the device rectangle with no origin; any
        // other records raster-space rectangles under a device origin.
        let Self {
            dst_rect,
            atlas: _,
            color: _,
            transform,
            origin,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
        } = &mut self;
        place(dst_rect, *transform, origin, by);
        self
    }
}

impl Shift for TextureInstance {
    fn shifted(mut self, by: PixelShift) -> Self {
        // The origin is a device position; the axes and texel coordinates
        // are not.
        let Self {
            origin_and_x,
            src_uv: _,
            tint: _,
            axis_y: _,
            clip_rrect: _,
            clip_kind: _,
            clip_device_to_local: _,
            clip_local_origin: _,
            original_image_uv: _,
        } = &mut self;
        by.apply(origin_and_x);
        self
    }
}

impl Shift for Vertex {
    fn shifted(mut self, by: PixelShift) -> Self {
        // Tessellated with the transform baked in; `tex_coord` is a texture
        // coordinate.
        let Self {
            position,
            color: _,
            tex_coord: _,
        } = &mut self;
        by.apply(position);
        self
    }
}

#[cfg(test)]
mod tests {
    use std::mem::offset_of;

    use super::{
        ArcInstance, CircleInstance, GlyphInstance, IDENTITY, LinearGradientInstance, PixelOrigin,
        RadialGradientInstance, RectInstance, ShadowInstance, Shift, SweepGradientInstance,
        TextureInstance, Vertex,
    };

    /// `T` with every word a distinct whole float.
    fn patterned<T: bytemuck::Pod>() -> T {
        let mut item = T::zeroed();
        let words: &mut [f32] = bytemuck::cast_slice_mut(bytemuck::bytes_of_mut(&mut item));
        for (index, word) in (0_u16..).zip(words.iter_mut()) {
            *word = 10.0 + f32::from(index);
        }
        item
    }

    /// The words `item` shifted by `(3, -2)` changes: each one's byte offset
    /// and how far it moved.
    fn moved<T: Shift + bytemuck::Pod>(item: T) -> Vec<(usize, f32)> {
        let by = PixelOrigin { x: 1, y: 5 }
            .shift_to(PixelOrigin { x: 4, y: 3 })
            .expect("the origins differ");
        let shifted = item.shifted(by);
        let before: &[f32] = bytemuck::cast_slice(bytemuck::bytes_of(&item));
        let after: &[f32] = bytemuck::cast_slice(bytemuck::bytes_of(&shifted));
        before
            .iter()
            .zip(after)
            .enumerate()
            .filter(|(_, (was, is))| was.to_bits() != is.to_bits())
            .map(|(index, (was, is))| (index * 4, is - was))
            .collect()
    }

    /// The words a shift by `(3, -2)` moves: the x and y of each position at
    /// `offsets`.
    fn positions(offsets: &[usize]) -> Vec<(usize, f32)> {
        let mut words: Vec<(usize, f32)> = offsets
            .iter()
            .flat_map(|&offset| [(offset, 3.0), (offset + 4, -2.0)])
            .collect();
        words.sort_by_key(|&(offset, _)| offset);
        words
    }

    /// Each instance type moves exactly its device positions, every other
    /// word untouched: a placement by translation unless the instance is in
    /// the baked layout, whose bounds are device pixels.
    #[test]
    fn a_shift_moves_exactly_the_device_positions() {
        let baked_rect = RectInstance {
            transform: IDENTITY,
            transform_translate: [0.0; 4],
            ..patterned()
        };
        let uniform_glyph = GlyphInstance {
            transform: IDENTITY,
            origin: [0.0; 4],
            ..patterned()
        };
        type Row = (&'static str, Vec<(usize, f32)>, Vec<usize>);
        let rows: [Row; 13] = [
            (
                "an affine rect",
                moved(patterned::<RectInstance>()),
                vec![offset_of!(RectInstance, transform_translate)],
            ),
            (
                "a baked rect",
                moved(baked_rect),
                vec![offset_of!(RectInstance, bounds)],
            ),
            (
                "a circle",
                moved(patterned::<CircleInstance>()),
                vec![offset_of!(CircleInstance, transform_translate)],
            ),
            (
                "an arc",
                moved(patterned::<ArcInstance>()),
                vec![offset_of!(ArcInstance, transform_translate)],
            ),
            (
                "a shadow",
                moved(patterned::<ShadowInstance>()),
                vec![
                    offset_of!(ShadowInstance, bounds),
                    offset_of!(ShadowInstance, rect_pos),
                ],
            ),
            (
                "a linear gradient",
                moved(patterned::<LinearGradientInstance>()),
                vec![offset_of!(LinearGradientInstance, transform_translate)],
            ),
            (
                "a radial gradient",
                moved(patterned::<RadialGradientInstance>()),
                vec![offset_of!(RadialGradientInstance, transform_translate)],
            ),
            (
                "a sweep gradient",
                moved(patterned::<SweepGradientInstance>()),
                vec![offset_of!(SweepGradientInstance, transform_translate)],
            ),
            (
                "an affine glyph",
                moved(patterned::<GlyphInstance>()),
                vec![offset_of!(GlyphInstance, origin)],
            ),
            (
                "a glyph on the device grid",
                moved(uniform_glyph),
                vec![offset_of!(GlyphInstance, dst_rect)],
            ),
            (
                "an image",
                moved(patterned::<TextureInstance>()),
                vec![offset_of!(TextureInstance, origin_and_x)],
            ),
            (
                "a tessellated vertex",
                moved(patterned::<Vertex>()),
                vec![offset_of!(Vertex, position)],
            ),
            (
                "a rect at the device origin",
                moved(RectInstance {
                    bounds: [0.0, 0.0, 4.0, 4.0],
                    ..baked_rect
                }),
                vec![offset_of!(RectInstance, bounds)],
            ),
        ];
        for (row, words, offsets) in rows {
            assert_eq!(words, positions(&offsets), "{row}");
        }
    }
}
