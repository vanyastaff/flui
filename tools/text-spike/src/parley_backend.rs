//! Verified against docs.rs/parley/0.11.1 (the real latest published
//! version at spike time, per `cargo info parley` -- our initial "0.7"
//! Cargo.toml guess was stale by 4 minor releases; see
//! `.rust-studio/specs/b7-text-stack-spike/plan.md` §6/§8). No flui crate
//! uses parley today, so there was no in-repo call site to ground this
//! against; every signature below was fetched from docs.rs, not recalled
//! from training data. API drift found versus the earlier 0.7-era draft:
//! - `ranged_builder` gained a trailing `quantize: bool` parameter.
//! - `StyleProperty::FontStack` no longer exists; font family now goes
//!   through `StyleProperty::FontFamily(FontFamily<'a>)`.
//! - `Layout::align` takes only `(Alignment, AlignmentOptions)` -- no
//!   `max_width` parameter (wrapping width is set once, by
//!   `break_all_lines`, not re-passed to `align`).
//! - `GlyphRun::glyphs()` is a direct method; no need to go through
//!   `.run().glyphs()`.
//! Line height is intentionally left at its default: `StyleProperty`'s
//! `LineHeight` variant now wraps a `LineHeight` type (not a bare `f32`
//! multiplier) whose exact constructors weren't worth resolving for a
//! spike that doesn't measure vertical metrics.
//!
//! Two shaping modes, added after a methodology review (see
//! `docs/research/text-stack-2026.md`'s Risk section): parley's own
//! `shape_item` (verified by reading parley-0.11.1's vendored source,
//! `src/shape/mod.rs:474`) computes `item_text[..segment_start_offset]
//! .chars().count()` per font segment, where `item_text` spans parley's
//! own bidi/script-analysis "item" -- and a plain multi-line string with
//! no script or bidi-level change (e.g. one script repeated for 10,000
//! lines) analyses as a SINGLE item covering the whole input, since `\n`
//! alone is not an item boundary. `shape()` below is exactly that: the
//! whole document handed to one `ranged_builder`/`build` call, which is
//! NOT how a real UI uses parley (Xilem/Masonry/Blitz, and FLUI's own
//! architecture, build one `Layout` per paragraph/`RenderParagraph`, not
//! one per document) and is the setup that produces the quadratic blowup
//! documented in the report. `shape_per_paragraph()` is the honest
//! equivalent of cosmic-text's own `Buffer` (which shapes each
//! `BidiParagraphs`-delimited line independently in `set_rich_text_impl`)
//! and of real parley usage: one `Layout` per `\n`-delimited paragraph,
//! reusing the same `FontContext`/`LayoutContext` across paragraphs the
//! way a realm-owned shaping session would.
//!
//! `shape_per_paragraph_retained()` exists for a fair memory comparison
//! (second methodology review): cosmic-text's single `Buffer::shape_until_scroll`
//! call builds and holds every paragraph's shaped `BufferLine` data
//! simultaneously before this spike's harness ever measures peak RSS, so
//! its measured peak reflects "N paragraphs' worth of shaped state alive
//! at once." A loop that builds and immediately drops one `Layout` per
//! paragraph never holds more than ~1 paragraph's data at a time, so its
//! peak RSS reflects a much smaller footprint regardless of document
//! size -- not because parley is more memory-efficient per paragraph, but
//! because the two measurements were retaining different amounts of
//! state. `shape_per_paragraph_retained` keeps every built `Layout` in a
//! `Vec` that the caller holds alive until after sampling peak RSS,
//! mirroring cosmic-text's retention.

use parley::layout::{Alignment, AlignmentOptions, PositionedLayoutItem};
use parley::style::{FontFamily, StyleProperty};
use parley::{FontContext, Layout, LayoutContext};

use crate::ShapeResult;

pub struct ParleyBackend {
    font_cx: FontContext,
    layout_cx: LayoutContext<()>,
}

impl ParleyBackend {
    pub fn new() -> Self {
        Self {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }

    /// Shapes the whole input as a single `Layout` -- NOT a UI scenario for
    /// multi-paragraph text (see module docs); kept only as the "what if
    /// you hand parley the entire document at once" data point.
    pub fn shape(&mut self, text: &str, max_width: Option<f32>) -> ShapeResult {
        self.shape_one(text, max_width).0
    }

    /// Splits `text` on `\n` and shapes each non-empty paragraph as its
    /// own `Layout`, reusing `font_cx`/`layout_cx` across paragraphs --
    /// the realistic per-`RenderParagraph` usage pattern. Each `Layout`
    /// is dropped as soon as its counts are folded in, so this is the
    /// right call for TIMING but not for a memory comparison against
    /// cosmic-text's retain-everything `Buffer` -- see
    /// `shape_per_paragraph_retained`.
    pub fn shape_per_paragraph(&mut self, text: &str, max_width: Option<f32>) -> ShapeResult {
        let mut line_count = 0usize;
        let mut glyph_count = 0usize;
        for paragraph in text.split('\n') {
            if paragraph.is_empty() {
                continue;
            }
            let (result, _layout) = self.shape_one(paragraph, max_width);
            line_count += result.line_count;
            glyph_count += result.glyph_count;
        }
        ShapeResult {
            line_count,
            glyph_count,
        }
    }

    /// Same as `shape_per_paragraph`, but returns every built `Layout`
    /// instead of dropping it -- the caller must keep the returned `Vec`
    /// alive until after sampling peak RSS for the memory number to be
    /// comparable to cosmic-text's `Buffer`, which retains every
    /// paragraph's shaped state simultaneously by construction.
    pub fn shape_per_paragraph_retained(
        &mut self,
        text: &str,
        max_width: Option<f32>,
    ) -> (ShapeResult, Vec<Layout<()>>) {
        let mut line_count = 0usize;
        let mut glyph_count = 0usize;
        let mut layouts = Vec::new();
        for paragraph in text.split('\n') {
            if paragraph.is_empty() {
                continue;
            }
            let (result, layout) = self.shape_one(paragraph, max_width);
            line_count += result.line_count;
            glyph_count += result.glyph_count;
            layouts.push(layout);
        }
        (
            ShapeResult {
                line_count,
                glyph_count,
            },
            layouts,
        )
    }

    fn shape_one(&mut self, text: &str, max_width: Option<f32>) -> (ShapeResult, Layout<()>) {
        let display_scale = 1.0_f32;
        let quantize = true;
        let mut builder =
            self.layout_cx
                .ranged_builder(&mut self.font_cx, text, display_scale, quantize);
        builder.push_default(StyleProperty::FontSize(16.0));
        builder.push_default(StyleProperty::FontFamily(FontFamily::from("system-ui")));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(max_width);
        layout.align(Alignment::Start, AlignmentOptions::default());

        let mut line_count = 0usize;
        let mut glyph_count = 0usize;
        for line in layout.lines() {
            line_count += 1;
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                    glyph_count += glyph_run.glyphs().count();
                }
            }
        }
        (
            ShapeResult {
                line_count,
                glyph_count,
            },
            layout,
        )
    }
}
