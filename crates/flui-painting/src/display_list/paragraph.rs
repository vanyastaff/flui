//! [`ShapedParagraph`]: a shaped, line-broken paragraph as the display list
//! carries it (ADR-0092 §4).
//!
//! It names no shaper. A run names its face by a [`FontBlob`] the paragraph
//! holds, so a paragraph replayed from a retained picture frames later still
//! carries the bytes its keys name; there is no per-frame table to miss it
//! (painting `ARCHITECTURE.md`, mapping decision 18). Glyph positions are in
//! logical pixels relative to the paragraph's top-left; the device grid, the
//! subpixel bin and the raster size are applied at placement
//! ([`ShapedRun::placed_glyphs`]), where the device transform is known.

use std::fmt;
use std::ops::Range;

use flui_foundation::geometry::{Rect, Size};

use crate::glyphs::{FaceKey, FontBytes, GlyphKey, PlacedGlyph, RunKey, SubpixelBin, Synthesis};
use crate::styling::Color;

/// A font file's bytes and the id that names them in a [`FaceKey`].
///
/// The id is unique among the blobs alive in the process, and stays with
/// these bytes while any holder keeps them: a paragraph, or the raster side's
/// registry.
#[derive(Clone)]
pub struct FontBlob {
    id: u64,
    bytes: FontBytes,
}

impl FontBlob {
    /// A blob with id `id` over `bytes`. The shaper mints the id; two blobs
    /// over different bytes never share one while both are alive.
    #[must_use]
    pub(crate) fn new(id: u64, bytes: FontBytes) -> Self {
        Self { id, bytes }
    }

    /// The id a [`FaceKey`] names these bytes by.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The font file's bytes.
    #[must_use]
    pub fn bytes(&self) -> &FontBytes {
        &self.bytes
    }
}

impl fmt::Debug for FontBlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontBlob")
            .field("id", &self.id)
            .field("len", &(*self.bytes).as_ref().len())
            .finish()
    }
}

/// One face: a blob and the face index inside it (a collection file holds
/// several).
#[derive(Clone, Debug)]
pub struct FontFace {
    blob: FontBlob,
    index: u32,
}

impl FontFace {
    pub(crate) fn new(blob: FontBlob, index: u32) -> Self {
        Self { blob, index }
    }

    /// The key a glyph of this face names it by.
    #[must_use]
    pub fn key(&self) -> FaceKey {
        FaceKey {
            blob_id: self.blob.id,
            index: self.index,
        }
    }

    /// The font file the face is in.
    #[must_use]
    pub fn blob(&self) -> &FontBlob {
        &self.blob
    }
}

/// One glyph of a run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// The glyph index inside the run's face.
    pub id: u16,
    /// The glyph's pen position along its line, in logical pixels from the
    /// paragraph's left edge, offsets and alignment applied.
    pub x: f32,
    /// The glyph's vertical offset from its line's baseline, in logical
    /// pixels, positive down.
    pub dy: f32,
}

/// What a run holds besides its glyphs; indices into the paragraph's
/// tables.
#[derive(Clone, Debug)]
pub(crate) struct RunData {
    pub(crate) face: u32,
    pub(crate) font_size: f32,
    pub(crate) coords: Range<u32>,
    pub(crate) synthesis: Synthesis,
    pub(crate) color: Option<Color>,
    pub(crate) baseline: f32,
    pub(crate) glyphs: Range<u32>,
}

/// A shaped, line-broken paragraph: what `DrawOp::Paragraph` carries and the
/// engine rasterizes.
///
/// Built from the one layout that measured the paragraph
/// (`TextPainter::layout`), so its size, lines and glyphs are what was laid
/// out: line breaks, `max_lines`, the ellipsis, per-span faces.
#[derive(Clone)]
pub struct ShapedParagraph {
    pub(crate) text: Box<str>,
    pub(crate) size: Size<f64>,
    pub(crate) baselines: Box<[f32]>,
    pub(crate) ink: Option<Rect<f64>>,
    pub(crate) faces: Box<[FontFace]>,
    pub(crate) runs: Box<[RunData]>,
    pub(crate) glyphs: Box<[ShapedGlyph]>,
    pub(crate) coords: Box<[i16]>,
    pub(crate) spans: Box<[String]>,
}

impl ShapedParagraph {
    /// The text the paragraph was shaped from, an ellipsis included.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The paragraph's laid-out box, in logical pixels.
    #[must_use]
    pub fn size(&self) -> Size<f64> {
        self.size
    }

    /// How many lines the paragraph keeps; at least one.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.baselines.len().max(1)
    }

    /// A box covering every pixel the glyphs can ink, relative to the
    /// paragraph's top-left, or `None` when a face gives no bounds to take it
    /// from.
    ///
    /// The laid-out box is where lines sit, not where glyphs end: under a
    /// line height tighter than the face's ascent and descent, or for a
    /// swash, an oblique overhang, a synthetic bold or a stacked mark, the
    /// ink reaches past it. A damage extent is built from this, since a rect
    /// that misses a glyph's pixel leaves it stale on a partial repaint.
    #[must_use]
    pub fn ink_bounds(&self) -> Option<Rect<f64>> {
        self.ink
    }

    /// One line per span naming what reached the shaper (family, weight,
    /// style, size, and a span colour where one differs from the root's), so
    /// a snapshot that prints them moves when a span is restyled.
    #[must_use]
    pub fn describe_spans(&self) -> &[String] {
        &self.spans
    }

    /// The glyph runs, line by line, in visual order within each line.
    pub fn runs(&self) -> impl ExactSizeIterator<Item = ShapedRun<'_>> + '_ {
        self.runs.iter().map(move |data| ShapedRun {
            paragraph: self,
            data,
        })
    }
}

impl fmt::Debug for ShapedParagraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShapedParagraph")
            .field("text", &self.text)
            .field("size", &self.size)
            .field("lines", &self.line_count())
            .field("runs", &self.runs.len())
            .field("glyphs", &self.glyphs.len())
            .finish_non_exhaustive()
    }
}

/// One run of a [`ShapedParagraph`]: glyphs of one face, size, variation
/// instance, synthesis and colour on one line.
#[derive(Clone, Copy)]
pub struct ShapedRun<'a> {
    paragraph: &'a ShapedParagraph,
    data: &'a RunData,
}

impl<'a> ShapedRun<'a> {
    /// The run's face.
    #[must_use]
    pub fn face(&self) -> &'a FontFace {
        &self.paragraph.faces[self.data.face as usize]
    }

    /// The font file the run's face is in.
    #[must_use]
    pub fn blob(&self) -> &'a FontBlob {
        &self.face().blob
    }

    /// The normalized variation coordinates, synthesis variations included;
    /// empty for the default instance.
    #[must_use]
    pub fn coords(&self) -> &'a [i16] {
        let range = self.data.coords.start as usize..self.data.coords.end as usize;
        &self.paragraph.coords[range]
    }

    /// The run's glyphs, in visual order.
    #[must_use]
    pub fn glyphs(&self) -> &'a [ShapedGlyph] {
        let range = self.data.glyphs.start as usize..self.data.glyphs.end as usize;
        &self.paragraph.glyphs[range]
    }

    /// Every glyph of the run placed in device pixels, keyed for an atlas.
    ///
    /// `key` is what the raster side's registry returned for this run
    /// ([`FontRegistry::prepare_run`](crate::glyphs::FontRegistry::prepare_run)).
    /// `origin` is the paragraph's top-left in device pixels and `scale` the
    /// device-pixel ratio. A glyph's pen position goes to
    /// `origin.x + x × scale`, split into a whole column and a quarter-pixel
    /// bin; its baseline row is `round(baseline × scale)` plus the truncated
    /// `origin.y + dy × scale`; its bitmap is rasterized at
    /// `font_size × scale`. These are the rules cosmic-text placed glyphs
    /// with, so a glyph lands on the device pixel it landed on there.
    pub fn placed_glyphs(
        &self,
        key: RunKey,
        origin: (f32, f32),
        scale: f32,
    ) -> impl Iterator<Item = PlacedGlyph> + 'a {
        let size = self.data.font_size * scale;
        let synthesis = self.data.synthesis;
        let color = self.data.color;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a device row fits i32; rounding is the baseline rule"
        )]
        let baseline = (self.data.baseline * scale).round() as i32;
        self.glyphs().iter().map(move |glyph| {
            let (x, bin) = SubpixelBin::split(glyph.x.mul_add(scale, origin.0));
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a device row fits i32; truncation is the vertical hinting rule"
            )]
            let dy = glyph.dy.mul_add(scale, origin.1).trunc() as i32;
            PlacedGlyph {
                key: GlyphKey::new(key.face, glyph.id, size, bin)
                    .with_variation(key.variation)
                    .with_synthesis(synthesis),
                x,
                y: baseline + dy,
                color,
            }
        })
    }
}

impl fmt::Debug for ShapedRun<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShapedRun")
            .field("face", &self.face().key())
            .field("font_size", &self.data.font_size)
            .field("synthesis", &self.data.synthesis)
            .field("glyphs", &self.glyphs().len())
            .finish_non_exhaustive()
    }
}
