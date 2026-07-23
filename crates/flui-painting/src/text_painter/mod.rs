// PORT-TARGET: flui-widgets::RichText, flui-widgets::TextField
//! Text measurement and painting.
//!
//! Provides [`TextPainter`], which measures and paints styled text.
//! Rust equivalent of Flutter's `TextPainter` class.
//!
//! # Concern split
//!
//! The 990-LOC `text_painter.rs` god module was split into a
//! `text_painter/` directory. Four files:
//!
//! - This module (`mod.rs`) -- `TextPainter` struct + `TextLayoutCache` +
//!   `LayoutMetrics` + `Default` + builder API + getters + setters +
//!   `mark_needs_layout` / `did_layout` lifecycle.
//! - [`baseline`] -- `TextBaseline` enum.
//! - [`measure`]  -- `layout` + `compute_layout_metrics` +
//!   `compute_paint_offset` + `size`/`width`/`height` +
//!   `compute_distance_to_actual_baseline` + `did_exceed_max_lines`.
//! - [`paint`]    -- `paint` + cursor methods (`get_offset_for_caret`,
//!   `get_position_for_offset`, `get_line_metrics`,
//!   `get_boxes_for_selection`, `get_word_boundary`).

use flui_types::{
    geometry::{Offset, Pixels, Size},
    typography::{
        InlineSpan, PlaceholderDimensions, StrutStyle, TextAlign, TextDirection,
        TextHeightBehavior, TextWidthBasis,
    },
};

use crate::text_layout::TextLayout;

pub mod baseline;
pub mod measure;
pub mod paint;

pub use baseline::TextBaseline;

/// Default font size when none is specified.
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

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
///
/// # Thread Safety
///
/// `TextPainter` is `Send` but not `Sync` due to mutable layout
/// state.
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

    /// Strut style for consistent line height.
    pub(super) strut_style: Option<StrutStyle>,

    /// How to measure text width.
    pub(super) text_width_basis: TextWidthBasis,

    /// Text height behavior.
    pub(super) text_height_behavior: Option<TextHeightBehavior>,

    /// Placeholder dimensions for inline widgets.
    pub(super) placeholder_dimensions: Vec<PlaceholderDimensions>,

    /// Cached layout result.
    pub(super) layout_cache: Option<TextLayoutCache>,
}

/// Cached layout information.
#[derive(Debug)]
pub(super) struct TextLayoutCache {
    /// The width constraint used for layout.
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
    pub(super) layout: TextLayout,
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
            strut_style: None,
            text_width_basis: TextWidthBasis::Parent,
            text_height_behavior: None,
            placeholder_dimensions: Vec::new(),
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

    /// Sets the strut style.
    #[must_use]
    pub fn with_strut_style(mut self, style: Option<StrutStyle>) -> Self {
        self.set_strut_style(style);
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

    /// Returns the strut style.
    #[inline]
    #[must_use]
    pub fn strut_style(&self) -> Option<&StrutStyle> {
        self.strut_style.as_ref()
    }

    /// Returns the text width basis.
    #[inline]
    #[must_use]
    pub fn text_width_basis(&self) -> TextWidthBasis {
        self.text_width_basis
    }

    /// Returns the text height behavior.
    #[inline]
    #[must_use]
    pub fn text_height_behavior(&self) -> Option<&TextHeightBehavior> {
        self.text_height_behavior.as_ref()
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
            (Some(old), Some(new)) => old.layout_affecting_eq(new),
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

    /// Sets the strut style.
    pub fn set_strut_style(&mut self, style: Option<StrutStyle>) {
        if self.strut_style != style {
            self.strut_style = style;
            self.mark_needs_layout();
        }
    }

    /// Sets the text width basis.
    pub fn set_text_width_basis(&mut self, basis: TextWidthBasis) {
        if self.text_width_basis != basis {
            self.text_width_basis = basis;
            self.mark_needs_layout();
        }
    }

    /// Sets the text height behavior.
    pub fn set_text_height_behavior(&mut self, behavior: Option<TextHeightBehavior>) {
        if self.text_height_behavior != behavior {
            self.text_height_behavior = behavior;
            self.mark_needs_layout();
        }
    }

    /// Sets placeholder dimensions for inline widgets.
    pub fn set_placeholder_dimensions(&mut self, dimensions: Vec<PlaceholderDimensions>) {
        if self.placeholder_dimensions != dimensions {
            self.placeholder_dimensions = dimensions;
            self.mark_needs_layout();
        }
    }

    // ===== Lifecycle =====

    /// Invalidates the layout cache.
    pub fn mark_needs_layout(&mut self) {
        self.layout_cache = None;
    }

    /// Returns true if layout has been computed.
    #[inline]
    #[must_use]
    pub fn did_layout(&self) -> bool {
        self.layout_cache.is_some()
    }
}
