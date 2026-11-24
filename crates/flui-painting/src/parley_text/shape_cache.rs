//! Laid-out paragraphs kept by everything they were laid out from, so a
//! paragraph laid out again shares the finished layout instead of shaping it.
//!
//! Flutter's text takes a similar shortcut: Skia's `ParagraphImpl::layout`
//! looks the paragraph's text, placeholders and styles up in its font
//! collection's `ParagraphCache` (128 entries, least recently used out) and
//! breaks the shaped runs it finds at the new width. Here an entry is the
//! layout already broken into lines and aligned, shared through an [`Arc`],
//! so a hit copies no glyph and breaks no line; the width, the alignment and
//! the line limit are therefore part of the key, and a paragraph laid out at
//! another width is shaped again.
//!
//! A paragraph is kept only the second time it is laid out. Text seen once (a
//! counter, a clock, a field being typed into, the rows of a long list
//! scrolled past) costs its hash and a look through the remembered hashes,
//! allocates nothing, and does not push out the paragraphs that come back.
//!
//! The entries are bounded by count and by an estimate of the bytes their
//! layouts and keys hold ([`held_bytes`]); a font registered on the
//! collection, or a [`reclaim`](crate::TextContext::reclaim), drops them all.

use std::hash::Hasher as _;
use std::mem::{size_of, size_of_val};
use std::ops::Deref;
use std::sync::Arc;

use flui_foundation::TextDirection;
use parley::Layout;

use super::shape::{ParagraphLayout, ParagraphSpec, SpanBrush};
use crate::text_layout::TextLayoutResult;
use crate::typography::{TextAlign, TextStyle};

/// How many laid-out paragraphs a context keeps, as Skia's `ParagraphCache`.
const MAX_ENTRIES: usize = 128;

/// How many bytes the kept paragraphs hold at most, by [`held_bytes`]. A
/// short label holds about 3 KiB, so the count bounds a cache of labels
/// first and this bounds one of paragraphs.
const MAX_BYTES: usize = 512 * 1024;

/// The largest paragraph kept, by [`held_bytes`]: about 500 characters of
/// text on a few lines. A longer one is rarely laid out twice the same, and
/// would push out many short ones.
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

/// A paragraph's layout: its own, or one it shares with the shape cache.
///
/// A paragraph is laid out into a layout of its own, which allocates no
/// `Arc`; the layout moves into one only when the cache keeps the paragraph.
/// An `Arc` and not an `Rc`: a painter keeps its layout, and a render object
/// holding the painter moves between threads.
#[expect(
    clippy::large_enum_variant,
    reason = "an own layout is what most paragraphs hold: boxing it would allocate on the shapings the cache does not keep"
)]
pub(super) enum HeldLayout {
    Own(Layout<SpanBrush>),
    Shared(Arc<Layout<SpanBrush>>),
}

impl HeldLayout {
    /// The layout, shared from now on: one of its own moves into an `Arc`.
    pub(super) fn share(&mut self) -> Arc<Layout<SpanBrush>> {
        let layout = match std::mem::replace(self, Self::Own(Layout::default())) {
            Self::Own(layout) => Arc::new(layout),
            Self::Shared(layout) => layout,
        };
        *self = Self::Shared(Arc::clone(&layout));
        layout
    }

    /// The layout's buffers, when nothing else holds them.
    pub(super) fn into_unshared(self) -> Option<Layout<SpanBrush>> {
        match self {
            Self::Own(layout) => Some(layout),
            Self::Shared(layout) => Arc::into_inner(layout),
        }
    }
}

impl Deref for HeldLayout {
    type Target = Layout<SpanBrush>;

    fn deref(&self) -> &Layout<SpanBrush> {
        match self {
            Self::Own(layout) => layout,
            Self::Shared(layout) => layout,
        }
    }
}

/// A paragraph the cache does not hold: what [`ShapeCache::offer`] needs to
/// keep it once it is laid out.
pub(super) struct Miss {
    hash: u64,
    /// The collection generation read before the paragraph was shaped.
    generation: u64,
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
/// it. Lengths, not capacities: a buffer may hold up to twice its length.
fn held_bytes(paragraph: &ParagraphLayout) -> usize {
    let layout: &Layout<SpanBrush> = &paragraph.layout;
    let characters = paragraph.text.chars().count();
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
        + (paragraph.spans.len() + 1) * STYLE_BYTES
        + size_of::<ParagraphLayout>()
        + paragraph.text.len()
        + size_of_val(paragraph.spans.as_slice())
        + size_of_val(&*paragraph.run_colors)
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
        let miss = Miss { hash, generation };
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
    /// A collection whose generation moved while the paragraph was shaped
    /// (`generation`, read after) keeps nothing.
    pub(super) fn offer(
        &mut self,
        miss: Miss,
        generation: u64,
        spec: &ParagraphSpec<'_>,
        paragraph: &mut ParagraphLayout,
        metrics: &TextLayoutResult,
        spare: &mut Option<Layout<SpanBrush>>,
    ) {
        if generation != miss.generation {
            return;
        }
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
            *spare = entry.paragraph.layout.into_unshared();
        }
    }
}
