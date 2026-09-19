//! [`TextPainter`]: lays an inline span out against a width constraint,
//! answers size / baseline / caret / hit-test queries on the result, and
//! paints it — the shape a `RenderParagraph` drives.
//!
//! - `measure` — `layout` and the queries over its cached metrics.
//! - `paint` — `paint` and the cursor queries.

use std::sync::Arc;

use flui_types::{
    geometry::{Offset, Pixels, Size},
    typography::{InlineSpan, TextAlign, TextDirection},
};

use crate::text_layout::TextLayout;

pub mod baseline;
pub mod measure;
pub mod paint;

pub use baseline::TextBaseline;

/// Default font size when none is specified.
pub(crate) const DEFAULT_FONT_SIZE: f32 = 14.0;

/// What a property change invalidates — the shaped/paint split.
///
/// Flutter cannot update paint attributes without recreating the engine
/// paragraph ("no API to only make those updates",
/// text_painter.dart:1335-1352): a color change re-shapes. flui's
/// [`TextPainter::set_text`] diffs the old and new span trees
/// ([`InlineSpan::layout_affecting_eq`]) and reports which half
/// actually changed; a paint-only change KEEPS the shaped layout —
/// metrics, baselines, and cursor geometry stay valid, and the next
/// paint re-emits draw commands with the new attributes at zero
/// reshape cost.
///
/// The variants are ordered by severity, so a prop-diff over several
/// properties can fold with `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Invalidation {
    /// Nothing observable changed.
    None,
    /// Only paint attributes changed (colors, shadows): repaint with
    /// the existing shaped layout.
    Paint,
    /// Glyph geometry changed: re-shape before the next paint.
    Layout,
}

/// A painter that lays out and paints text.
///
/// This is the primary interface for measuring and rendering text in
/// FLUI. Wraps a styled text span and provides layout and painting
/// capabilities.
///
/// # Lifecycle
///
/// 1. Create with [`TextPainter::new`] or builder methods.
/// 2. Set text and configuration.
/// 3. Call [`layout`](TextPainter::layout) to compute metrics.
/// 4. Read metrics like [`width`](TextPainter::width),
///    [`height`](TextPainter::height).
/// 5. Call [`paint`](TextPainter::paint) to render.
#[derive(Debug)]
pub struct TextPainter {
    /// The styled text to paint.
    pub(super) text: Option<InlineSpan>,

    /// How text should be aligned horizontally.
    pub(super) text_align: TextAlign,

    /// The default text direction.
    pub(super) text_direction: Option<TextDirection>,

    /// Text scaling factor for accessibility.
    pub(super) text_scale_factor: f32,

    /// Maximum number of lines before truncation.
    pub(super) max_lines: Option<u32>,

    /// Ellipsis string for overflow.
    pub(super) ellipsis: Option<String>,

    /// Cached layout result.
    pub(super) layout_cache: Option<TextLayoutCache>,
}

/// Cached layout information.
#[derive(Debug)]
pub(super) struct TextLayoutCache {
    /// The width constraint used for layout.
    /// The font database generation the layout was shaped against; a face
    /// registered since makes the same text shape differently.
    pub(super) font_generation: u64,
    pub(super) min_width: f32,
    /// The max width constraint used for layout.
    pub(super) max_width: f32,
    /// Computed size after layout.
    pub(super) size: Size<Pixels>,
    /// Distance to alphabetic baseline.
    pub(super) alphabetic_baseline: f32,
    /// Distance to ideographic baseline.
    pub(super) ideographic_baseline: f32,
    /// Whether layout did overflow.
    pub(super) did_exceed_max_lines: bool,
    /// Computed paint offset based on alignment.
    pub(super) paint_offset: Offset<Pixels>,
    /// The underlying text layout for cursor/hit testing.
    pub(super) layout: Arc<TextLayout>,

    /// Precomputed min intrinsic width (narrowest unbreakable run).
    /// Computed once during `layout()` — O(1) access for intrinsics queries.
    /// Parley-inspired: shape-once, query-many.
    pub(super) min_intrinsic_width: f32,
    /// Precomputed max intrinsic width (single-line width).
    /// Computed once during `layout()` — O(1) access for intrinsics queries.
    pub(super) max_intrinsic_width: f32,
}

/// Intermediate layout metrics returned by `compute_layout_metrics`.
pub(super) struct LayoutMetrics {
    pub(super) size: Size<Pixels>,
    pub(super) alphabetic_baseline: f32,
    pub(super) ideographic_baseline: f32,
    pub(super) did_exceed_max_lines: bool,
    pub(super) paint_offset: Offset<Pixels>,
}

impl Default for TextPainter {
    fn default() -> Self {
        Self::new()
    }
}

impl TextPainter {
    /// Creates a new text painter with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: None,
            text_align: TextAlign::Start,
            text_direction: None,
            text_scale_factor: 1.0,
            max_lines: None,
            ellipsis: None,
            layout_cache: None,
        }
    }

    // ===== Builder API =====

    /// Creates a text painter with the given text span.
    #[must_use]
    pub fn with_text(mut self, text: impl Into<InlineSpan>) -> Self {
        self.set_text(Some(text.into()));
        self
    }

    /// Sets the text direction.
    #[must_use]
    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.set_text_direction(Some(direction));
        self
    }

    /// Sets the text alignment.
    #[must_use]
    pub fn with_text_align(mut self, align: TextAlign) -> Self {
        self.set_text_align(align);
        self
    }

    /// Sets the text scale factor.
    #[must_use]
    pub fn with_text_scale_factor(mut self, factor: f32) -> Self {
        self.set_text_scale_factor(factor);
        self
    }

    /// Sets the maximum number of lines.
    #[must_use]
    pub fn with_max_lines(mut self, max_lines: Option<u32>) -> Self {
        self.set_max_lines(max_lines);
        self
    }

    /// Sets the ellipsis string.
    #[must_use]
    pub fn with_ellipsis(mut self, ellipsis: Option<String>) -> Self {
        self.set_ellipsis(ellipsis);
        self
    }

    // ===== Getters =====

    /// Returns the styled text to paint.
    #[inline]
    #[must_use]
    pub fn text(&self) -> Option<&InlineSpan> {
        self.text.as_ref()
    }

    /// Whether [`layout`](Self::layout) has run and a cached result is
    /// available — guards the methods (paint, baseline, cursor) that
    /// otherwise panic when queried before layout.
    #[inline]
    #[must_use]
    pub fn has_layout(&self) -> bool {
        self.layout_cache.is_some()
    }

    /// Returns the text alignment.
    #[inline]
    #[must_use]
    pub fn text_align(&self) -> TextAlign {
        self.text_align
    }

    /// Returns the text direction.
    #[inline]
    #[must_use]
    pub fn text_direction(&self) -> Option<TextDirection> {
        self.text_direction
    }

    /// Returns the text scale factor.
    #[inline]
    #[must_use]
    pub fn text_scale_factor(&self) -> f32 {
        self.text_scale_factor
    }

    /// Returns the maximum number of lines.
    #[inline]
    #[must_use]
    pub fn max_lines(&self) -> Option<u32> {
        self.max_lines
    }

    /// Returns the ellipsis string.
    #[inline]
    #[must_use]
    pub fn ellipsis(&self) -> Option<&str> {
        self.ellipsis.as_deref()
    }

    // ===== Setters =====

    /// Sets the text to paint and reports what the change invalidates.
    ///
    /// The shaped/paint split: when the new span tree differs only in
    /// paint attributes (colors, shadows), the shaped layout is KEPT —
    /// size, baselines, and cursor geometry remain valid, and the next
    /// [`paint`](TextPainter::paint) emits the new attributes with zero
    /// reshape cost. Only a layout-affecting change (text content, font
    /// selection, sizes, spacing) drops the cache, and only then must
    /// [`layout`](TextPainter::layout) run again before painting. The
    /// caller (a render object) routes the returned [`Invalidation`] to
    /// its mark-needs-layout / mark-needs-paint decision.
    pub fn set_text(&mut self, text: Option<InlineSpan>) -> Invalidation {
        if self.text == text {
            return Invalidation::None;
        }
        let layout_preserved = match (&self.text, &text) {
            // A span colour is baked into the shaped layout (only the root
            // colour rides on the paragraph command), so a recolour of one
            // span is a layout change here even though the geometry is not.
            (Some(old), Some(new)) => {
                old.layout_affecting_eq(new) && self.span_colors(old) == self.span_colors(new)
            }
            // Appearing/disappearing text is always a layout change.
            _ => false,
        };
        self.text = text;
        if layout_preserved {
            Invalidation::Paint
        } else {
            self.mark_needs_layout();
            Invalidation::Layout
        }
    }

    /// Sets the text alignment.
    ///
    /// Alignment is a PAINT offset over the shaped lines, not a shaping
    /// input: the cached layout is kept and only its paint offset is
    /// recomputed (Flutter bakes alignment into the paragraph and
    /// re-shapes here).
    pub fn set_text_align(&mut self, align: TextAlign) -> Invalidation {
        if self.text_align == align {
            return Invalidation::None;
        }
        self.text_align = align;
        // Two-step to satisfy the borrow checker: compute from the
        // cache's stored extents, then write back.
        let recomputed = self
            .layout_cache
            .as_ref()
            .map(|cache| self.compute_paint_offset(cache.size.width.0, cache.max_width));
        if let (Some(cache), Some(offset)) = (&mut self.layout_cache, recomputed) {
            cache.paint_offset = offset;
        }
        Invalidation::Paint
    }

    /// Sets the text direction.
    pub fn set_text_direction(&mut self, direction: Option<TextDirection>) {
        if self.text_direction != direction {
            self.text_direction = direction;
            self.mark_needs_layout();
        }
    }

    /// Sets the text scale factor.
    pub fn set_text_scale_factor(&mut self, factor: f32) {
        if (self.text_scale_factor - factor).abs() > f32::EPSILON {
            self.text_scale_factor = factor;
            self.mark_needs_layout();
        }
    }

    /// Sets the maximum number of lines.
    pub fn set_max_lines(&mut self, max_lines: Option<u32>) {
        if self.max_lines != max_lines {
            self.max_lines = max_lines;
            self.mark_needs_layout();
        }
    }

    /// Sets the ellipsis string.
    pub fn set_ellipsis(&mut self, ellipsis: Option<String>) {
        if self.ellipsis != ellipsis {
            self.ellipsis = ellipsis;
            self.mark_needs_layout();
        }
    }

    // ===== Lifecycle =====

    /// Invalidates the layout cache.
    pub fn mark_needs_layout(&mut self) {
        self.layout_cache = None;
    }
}
