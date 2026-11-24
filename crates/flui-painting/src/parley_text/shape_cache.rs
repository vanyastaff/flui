//! Laid-out paragraphs kept by everything they were laid out from, so a
//! paragraph laid out again shares the finished layout instead of shaping it.
//!
//! Flutter's text takes a similar shortcut: Skia's `ParagraphImpl::layout`
//! looks the paragraph's text, placeholders and styles up in its font
//! collection's `ParagraphCache` (128 entries, least recently used out) and
//! breaks the shaped runs it finds at the new width. Here an entry is the
//! shaping already broken into lines and aligned, with its text and span
//! records, shared through an [`Arc`], so a hit copies no glyph and breaks no
//! line; the width, the alignment and the line limit are therefore part of the
//! key, and a paragraph laid out at another width is shaped again. The entry
//! also keeps the paragraph its first painter built for the display list, which
//! the later painters share.
//!
//! A paragraph is kept only the second time it is laid out. Text seen once (a
//! counter, a clock, a field being typed into, the rows of a long list
//! scrolled past) costs its hash and a look through the remembered hashes,
//! allocates nothing, and does not push out the paragraphs that come back.
//!
//! The entries are bounded by count and by an estimate of the bytes their
//! layouts, paintings and keys hold ([`held_bytes`]); a font registered on the
//! collection, or a [`reclaim`](crate::TextContext::reclaim), drops them all.

use std::hash::Hasher as _;
use std::mem::{size_of, size_of_val};
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use flui_foundation::TextDirection;
use parley::Layout;

use super::shape::{ParagraphLayout, ParagraphSpec, SpanBrush, SpanInfo};
use crate::display_list::ShapedParagraph;
use crate::styling::Color;
use crate::text_layout::TextLayoutResult;
use crate::typography::{TextAlign, TextStyle};

/// How many laid-out paragraphs a context keeps, as Skia's `ParagraphCache`.
const MAX_ENTRIES: usize = 128;

/// How many bytes the kept paragraphs hold at most, by [`held_bytes`]. A
/// short label holds about 3 KiB, so the count bounds a cache of labels
/// first and this bounds one of paragraphs.
const MAX_BYTES: usize = 512 * 1024;

/// The largest paragraph kept, by [`held_bytes`]: about 400 characters of
/// text on a few lines, with the painting it keeps. A longer one is rarely
/// laid out twice the same, and would push out many short ones.
const MAX_ENTRY_BYTES: usize = 32 * 1024;

/// How many paragraphs laid out once are remembered by hash, waiting for the
/// second layout that admits them. A paragraph that comes back after more
/// than this many others is shaped each time and never kept.
const SEEN: usize = 64;

/// What Parley 0.13 keeps per character of a layout's text: a `Character`
/// (12 bytes), a `ShapedCluster` (24) and a `Glyph` (16), each reserved per
/// character before shaping. Measured with `size_of` on `parley_engine`
/// 0.13.0; a Parley upgrade measures them again.
const CHARACTER_BYTES: usize = 12 + 24 + 16;

/// A glyph past one per character, in a script that shapes a character into
/// several glyphs.
const GLYPH_BYTES: usize = 16;

/// What Parley 0.13 keeps per run: its `ShapedRun` (176 bytes), its
/// `RunData` (about 84) and its `LayoutItem` (24), rounded up for the font
/// instance and the per-run settings beside them.
const RUN_BYTES: usize = 320;

/// What Parley 0.13 keeps per line: its `LineData` (about 112 bytes).
const LINE_BYTES: usize = 112;

/// What Parley 0.13 keeps per run on a line: its `LineItemData` (40 bytes).
const LINE_ITEM_BYTES: usize = 40;

/// What Parley 0.13 keeps per resolved style: its `Style` (76 bytes) and its
/// `StyleMetrics`, rounded up.
const STYLE_BYTES: usize = 128;

/// What shaping a paragraph produced and nothing changes afterwards: the
/// Parley layout, the text it holds, the span records and the colour each
/// paint run was shaped with.
#[derive(Default)]
pub(super) struct Shaping {
    pub(super) layout: Layout<SpanBrush>,
    /// The text the layout holds, an appended ellipsis included.
    pub(super) text: String,
    pub(super) spans: Vec<SpanInfo>,
    /// The colour each paint run paints with, by the run's index in its
    /// glyph runs' [`SpanBrush`], as the spans gave them.
    pub(super) run_colors: Box<[Option<Color>]>,
}

/// A shaping the cache keeps, with the paragraph it was first painted as.
pub(super) struct KeptShaping {
    shaping: Shaping,
    /// The paragraph built from the shaping for paint, over the root colour
    /// it was built for: a later layout with the shaped run colours under that
    /// root shares it instead of collecting the glyphs again.
    painted: OnceLock<Painted>,
}

struct Painted {
    root: Option<Color>,
    paragraph: Arc<ShapedParagraph>,
}

/// A paragraph's shaping: its own, or one it shares with the shape cache.
///
/// A paragraph is shaped into a shaping of its own, which allocates no `Arc`;
/// the shaping moves into one only when the cache keeps the paragraph. An
/// `Arc` and not an `Rc`: a painter keeps its layout, and a render object
/// holding the painter moves between threads.
#[expect(
    clippy::large_enum_variant,
    reason = "an own shaping is what most paragraphs hold: boxing it would allocate on the shapings the cache does not keep"
)]
pub(super) enum HeldShaping {
    Own(Shaping),
    Shared(Arc<KeptShaping>),
}

impl HeldShaping {
    /// The shaping, shared from now on: one of its own moves into an `Arc`.
    pub(super) fn share(&mut self) -> Arc<KeptShaping> {
        let kept = match std::mem::replace(self, Self::Own(Shaping::default())) {
            Self::Own(shaping) => Arc::new(KeptShaping {
                shaping,
                painted: OnceLock::new(),
            }),
            Self::Shared(kept) => kept,
        };
        *self = Self::Shared(Arc::clone(&kept));
        kept
    }

    /// The layout's buffers, when nothing else holds them.
    pub(super) fn into_unshared(self) -> Option<Layout<SpanBrush>> {
        match self {
            Self::Own(shaping) => Some(shaping.layout),
            Self::Shared(kept) => Arc::into_inner(kept).map(|kept| kept.shaping.layout),
        }
    }

    /// The paragraph a kept shaping was first painted as, when that was under
    /// `root`; otherwise the one `paint` builds, which a kept shaping not yet
    /// painted remembers.
    pub(super) fn painted(
        &self,
        root: Option<Color>,
        paint: impl FnOnce() -> ShapedParagraph,
    ) -> Arc<ShapedParagraph> {
        let Self::Shared(kept) = self else {
            return Arc::new(paint());
        };
        if let Some(painted) = kept.painted.get() {
            if painted.root == root {
                return Arc::clone(&painted.paragraph);
            }
            return Arc::new(paint());
        }
        let paragraph = Arc::new(paint());
        // A painting another holder of the shaping set first stays; this one
        // is built from the same shaping and root.
        let _ = kept.painted.set(Painted {
            root,
            paragraph: Arc::clone(&paragraph),
        });
        paragraph
    }
}

impl Deref for HeldShaping {
    type Target = Shaping;

    fn deref(&self) -> &Shaping {
        match self {
            Self::Own(shaping) => shaping,
            Self::Shared(kept) => &kept.shaping,
        }
    }
}

/// A paragraph the cache does not hold: what [`ShapeCache::offer`] needs to
/// keep it once it is laid out. A paragraph kept after a registration that
/// came while it was shaped carries the generation read before it, and the
/// next [`find`](ShapeCache::find) drops it with the rest.
pub(super) struct Miss {
    hash: u64,
}

/// A kept paragraph's [`ParagraphSpec`], owned.
struct ShapeKey {
    spans: Box<[(String, Option<TextStyle>)]>,
    default_style: Option<TextStyle>,
    font_size: f32,
    font_weight_adjustment: i32,
    max_width: Option<f32>,
    min_width: f32,
    text_align: TextAlign,
    line_height: Option<f32>,
    direction: TextDirection,
    max_lines: Option<usize>,
    ellipsis: Option<Box<str>>,
}

impl ShapeKey {
    /// The spec's fields, every one named, so that a field added to
    /// [`ParagraphSpec`] does not compile until the key holds it.
    fn of(spec: &ParagraphSpec<'_>) -> Self {
        let ParagraphSpec {
            spans,
            default_style,
            font_size,
            font_weight_adjustment,
            max_width,
            min_width,
            text_align,
            line_height,
            direction,
            max_lines,
            ellipsis,
        } = *spec;
        Self {
            spans: spans.into(),
            default_style: default_style.cloned(),
            font_size,
            font_weight_adjustment,
            max_width,
            min_width,
            text_align,
            line_height,
            direction,
            max_lines,
            ellipsis: ellipsis.map(Box::from),
        }
    }

    fn matches(&self, spec: &ParagraphSpec<'_>) -> bool {
        let ParagraphSpec {
            spans,
            default_style,
            font_size,
            font_weight_adjustment,
            max_width,
            min_width,
            text_align,
            line_height,
            direction,
            max_lines,
            ellipsis,
        } = *spec;
        // Sizes by their bits: a validated spec holds no NaN, and the bits
        // tell 0.0 from -0.0 as the layout may.
        self.font_size.to_bits() == font_size.to_bits()
            && self.font_weight_adjustment == font_weight_adjustment
            && self.max_width.map(f32::to_bits) == max_width.map(f32::to_bits)
            && self.min_width.to_bits() == min_width.to_bits()
            && self.text_align == text_align
            && self.line_height.map(f32::to_bits) == line_height.map(f32::to_bits)
            && self.direction == direction
            && self.max_lines == max_lines
            && self.ellipsis.as_deref() == ellipsis
            && self.default_style.as_ref() == default_style
            && *self.spans == *spans
    }

    /// The heap and inline bytes the key holds, styles counted by their size
    /// alone.
    fn bytes(&self) -> usize {
        let span = size_of::<(String, Option<TextStyle>)>();
        size_of::<Self>()
            + self
                .spans
                .iter()
                .map(|(text, _)| span + text.len())
                .sum::<usize>()
            + self.ellipsis.as_deref().map_or(0, str::len)
    }
}

/// A hash of the spec's text and sizes. Styles, alignment and direction are
/// left to [`ShapeKey::matches`]: two paragraphs that differ only there share
/// a hash and are told apart by the comparison.
fn spec_hash(spec: &ParagraphSpec<'_>) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_usize(spec.spans.len());
    for (text, _) in spec.spans {
        hasher.write_usize(text.len());
        hasher.write(text.as_bytes());
    }
    hasher.write_u32(spec.font_size.to_bits());
    hasher.write_i32(spec.font_weight_adjustment);
    hasher.write_u32(spec.max_width.map_or(u32::MAX, f32::to_bits));
    hasher.write_u32(spec.min_width.to_bits());
    hasher.write_u32(spec.line_height.map_or(u32::MAX, f32::to_bits));
    hasher.write_usize(spec.max_lines.unwrap_or(usize::MAX));
    match spec.ellipsis {
        Some(ellipsis) => {
            hasher.write_usize(ellipsis.len());
            hasher.write(ellipsis.as_bytes());
        }
        None => hasher.write_usize(usize::MAX),
    }
    // Zero marks an empty slot of `ShapeCache::seen`.
    hasher.finish().max(1)
}

/// An estimate of the bytes `paragraph`'s layout holds, from what Parley's
/// public API counts (characters, glyphs, runs, lines) and the record sizes
/// of Parley 0.13, with the text and span records the paragraph holds beside
/// it and the [`ShapedParagraph`] its painting keeps. Lengths, not
/// capacities: a buffer may hold up to twice its length.
fn held_bytes(paragraph: &ParagraphLayout) -> usize {
    let layout: &Layout<SpanBrush> = &paragraph.shaping.layout;
    let characters = paragraph.shaping.text.chars().count();
    let mut glyphs = 0;
    // A run that spans lines counts once per line it is on, as its line item
    // does: an overestimate of its run records.
    let mut line_runs = 0;
    for line in layout.lines() {
        for run in line.runs() {
            line_runs += 1;
            glyphs += run
                .clusters()
                .map(|cluster| cluster.glyphs().count())
                .sum::<usize>();
        }
    }
    size_of::<Layout<SpanBrush>>()
        + characters * CHARACTER_BYTES
        + glyphs.saturating_sub(characters) * GLYPH_BYTES
        + line_runs * (RUN_BYTES + LINE_ITEM_BYTES)
        + layout.len() * LINE_BYTES
        + (paragraph.shaping.spans.len() + 1) * STYLE_BYTES
        + size_of::<ParagraphLayout>()
        + paragraph.shaping.text.len()
        + size_of_val(paragraph.shaping.spans.as_slice())
        + size_of_val(&*paragraph.shaping.run_colors)
        + painted_bytes(paragraph, glyphs, line_runs)
}

/// What the [`ShapedParagraph`] a kept paragraph is painted as holds: a
/// glyph per glyph and a run record per line run of its kept lines, the
/// text, a baseline per line, a description per span and a face.
fn painted_bytes(paragraph: &ParagraphLayout, glyphs: usize, line_runs: usize) -> usize {
    use crate::display_list::{FontFace, ShapedGlyph, SpanDescription};
    size_of::<ShapedParagraph>()
        + 2 * size_of::<usize>()
        + glyphs * size_of::<ShapedGlyph>()
        + line_runs * size_of::<crate::display_list::RunData>()
        + paragraph.shaping.text.len()
        + paragraph.shaping.layout.len() * size_of::<f32>()
        + paragraph.shaping.spans.len() * size_of::<SpanDescription>()
        + size_of::<FontFace>()
}

struct Entry {
    hash: u64,
    key: ShapeKey,
    /// Laid out and validated; its layout is shared.
    paragraph: ParagraphLayout,
    metrics: TextLayoutResult,
    /// [`held_bytes`] of the paragraph and the key's bytes.
    bytes: usize,
    /// The cache's clock when this entry was last found or kept.
    used: u64,
}

/// The laid-out paragraphs of one [`TextContext`](crate::TextContext), valid
/// for one generation of its font collection.
pub(crate) struct ShapeCache {
    /// The collection generation the entries were shaped at.
    generation: u64,
    entries: Vec<Entry>,
    bytes: usize,
    clock: u64,
    /// Hashes of paragraphs laid out once and not kept, oldest overwritten
    /// first; zero is an empty slot. Inline, so remembering allocates nothing.
    seen: [u64; SEEN],
    next_seen: usize,
}

impl Default for ShapeCache {
    fn default() -> Self {
        Self {
            generation: 0,
            entries: Vec::new(),
            bytes: 0,
            clock: 0,
            seen: [0; SEEN],
            next_seen: 0,
        }
    }
}

impl ShapeCache {
    /// The paragraph `spec` names, laid out at `generation`, with its metrics;
    /// or what [`offer`](Self::offer) needs to keep it once it is laid out.
    /// A generation other than the entries' drops them all first: a face
    /// registered since may shape the same text differently.
    pub(super) fn find(
        &mut self,
        generation: u64,
        spec: &ParagraphSpec<'_>,
    ) -> Result<(ParagraphLayout, TextLayoutResult), Miss> {
        self.retire_older_than(generation);
        let hash = spec_hash(spec);
        let miss = Miss { hash };
        if self.entries.is_empty() {
            return Err(miss);
        }
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.hash == hash && entry.key.matches(spec))
        else {
            return Err(miss);
        };
        self.clock += 1;
        entry.used = self.clock;
        Ok((entry.paragraph.share(), entry.metrics.clone()))
    }

    /// Offers `paragraph`, just laid out from `spec` and validated, after
    /// `miss`. It is kept on its second layout, within the bounds, evicting
    /// the least recently used; its layout is then shared with the cache. An
    /// evicted layout nothing else holds becomes `spare` when there is none.
    pub(super) fn offer(
        &mut self,
        miss: Miss,
        spec: &ParagraphSpec<'_>,
        paragraph: &mut ParagraphLayout,
        metrics: &TextLayoutResult,
        spare: &mut Option<Layout<SpanBrush>>,
    ) {
        let Some(slot) = self.seen.iter().position(|&seen| seen == miss.hash) else {
            self.seen[self.next_seen] = miss.hash;
            self.next_seen = (self.next_seen + 1) % SEEN;
            return;
        };
        self.seen[slot] = 0;
        let paragraph_bytes = held_bytes(paragraph);
        if paragraph_bytes > MAX_ENTRY_BYTES {
            return;
        }
        let key = ShapeKey::of(spec);
        let bytes = paragraph_bytes + key.bytes();
        while !self.entries.is_empty()
            && (self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES)
        {
            self.evict_least_recent(spare);
        }
        self.clock += 1;
        self.bytes += bytes;
        self.entries.push(Entry {
            hash: miss.hash,
            key,
            paragraph: paragraph.share(),
            metrics: metrics.clone(),
            bytes,
            used: self.clock,
        });
    }

    /// Drops every kept paragraph and remembered hash.
    pub(crate) fn clear(&mut self) {
        *self = Self {
            generation: self.generation,
            ..Self::default()
        };
    }

    fn retire_older_than(&mut self, generation: u64) {
        if generation != self.generation {
            self.clear();
            self.generation = generation;
        }
    }

    fn evict_least_recent(&mut self, spare: &mut Option<Layout<SpanBrush>>) {
        let Some(oldest) = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.used)
            .map(|(index, _)| index)
        else {
            return;
        };
        let entry = self.entries.swap_remove(oldest);
        self.bytes -= entry.bytes;
        if spare.is_none() {
            *spare = entry.paragraph.shaping.into_unshared();
        }
    }
}
