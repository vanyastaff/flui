//! What a paragraph paints besides its glyphs: span backgrounds, the
//! shadows spans cast (ADR-0309 §2) and the lines their decorations draw
//! (ADR-0310).

use std::sync::Arc;

use flui_foundation::geometry::Offset;

use super::decoration::Drawn;
use crate::canvas::Canvas;
use crate::display_list::BlurSigma;
use crate::paint::Paint;
use crate::parley_text::{DecoratedStretch, ParagraphLayout};
use crate::styling::{Color, Shadow};
use crate::text_layout::SpanPaint;
use crate::typography::{TextDecoration, TextStyle};

/// Per paragraph span, the background it fills, the shadows it casts and
/// the decoration it draws.
///
/// Built from the spans' merged styles where the painter takes its text
/// (a layout, or a `set_text` that keeps the layout), so a paint does not
/// walk the span tree. A paragraph that paints none of them has none, and
/// records its paragraph op alone.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpanDecor {
    /// The background of each paragraph span, in shaping order.
    backgrounds: Box<[Option<Color>]>,
    /// Each distinct shadow list, in the order of the first span that casts
    /// it.
    shadows: Box<[Casting]>,
    /// The decoration of each paragraph span, in shaping order; empty when
    /// no span draws one.
    decorations: Box<[Option<TextDecoration>]>,
}

/// One shadow list and the spans that cast it.
#[derive(Clone, Debug, PartialEq)]
struct Casting {
    list: Box<[Shadow<f64>]>,
    /// Paragraph span indices, ascending.
    casters: Vec<usize>,
}

impl SpanDecor {
    /// What `spans` (the merged styles the paragraph is shaped from) paint
    /// besides their glyphs, or `None` when that is nothing.
    pub(super) fn of(spans: &[(String, Option<TextStyle>)]) -> Option<Self> {
        let mut backgrounds: Vec<Option<Color>> = Vec::with_capacity(spans.len());
        let mut decorations: Vec<Option<TextDecoration>> = Vec::with_capacity(spans.len());
        let mut shadows: Vec<Casting> = Vec::new();
        for (index, (_, style)) in spans.iter().enumerate() {
            let paint = style.as_ref().map(SpanPaint::of).unwrap_or_default();
            backgrounds.push(paint.background);
            decorations.push(paint.decoration.filter(|decoration| !decoration.is_none()));
            let Some(list) = paint.shadows.filter(|list| !list.is_empty()) else {
                continue;
            };
            match shadows.iter_mut().find(|cast| cast.list.as_ref() == list) {
                Some(cast) => cast.casters.push(index),
                None => shadows.push(Casting {
                    list: list.into(),
                    casters: vec![index],
                }),
            }
        }
        if decorations.iter().all(Option::is_none) {
            decorations.clear();
        }
        if backgrounds.iter().all(Option::is_none) && shadows.is_empty() && decorations.is_empty() {
            return None;
        }
        Some(Self {
            backgrounds: backgrounds.into_boxed_slice(),
            shadows: shadows.into_boxed_slice(),
            decorations: decorations.into_boxed_slice(),
        })
    }

    /// The stretches the spans' decorations are drawn along, over glyphs
    /// whose root colour is `root`.
    pub(super) fn decorated(
        &self,
        layout: &ParagraphLayout,
        root: Color,
    ) -> Vec<DecoratedStretch<Drawn>> {
        if self.decorations.is_empty() {
            return Vec::new();
        }
        layout.decorated_stretches(&|site| {
            let decoration = self.decorations.get(site.span).copied().flatten()?;
            Drawn::of(decoration, site.glyph, root)
        })
    }

    /// Records the span backgrounds, then each shadow in list order, for a
    /// paragraph laid out as `layout` with its top-left at `offset`.
    ///
    /// A shadow is skipped when it is transparent or its blur radius is not
    /// finite; [`Canvas::draw_paragraph_shadow`] refuses one whose offset
    /// sum is not finite.
    pub(super) fn paint_backgrounds_and_shadows(
        &self,
        canvas: &mut Canvas,
        layout: &ParagraphLayout,
        offset: Offset<f64>,
    ) {
        if self.backgrounds.iter().any(Option::is_some) {
            let fill = |span: usize| self.backgrounds.get(span).copied().flatten();
            for (rect, color) in layout.backgrounds(&fill) {
                canvas.draw_rect(rect.translate_offset(offset), &Paint::fill(color));
            }
        }
        for Casting { list, casters } in &self.shadows {
            let casts = |span: usize| casters.binary_search(&span).is_ok();
            let Some(silhouette) = layout.silhouette(&casts) else {
                continue;
            };
            let silhouette = Arc::new(silhouette);
            for shadow in list {
                if shadow.color.is_transparent() {
                    continue;
                }
                let Some(sigma) = BlurSigma::from_blur_radius(shadow.blur_radius) else {
                    continue;
                };
                let at = Offset::new(offset.dx + shadow.offset.dx, offset.dy + shadow.offset.dy);
                canvas.draw_paragraph_shadow(&silhouette, at, shadow.color, sigma);
            }
        }
    }
}
